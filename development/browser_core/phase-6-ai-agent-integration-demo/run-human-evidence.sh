#!/usr/bin/env bash
# This Source Code Form is subject to the terms of the Mozilla Public
# License, v. 2.0. If a copy of the MPL was not distributed with this
# file, You can obtain one at https://mozilla.org/MPL/2.0/.

# Operator-driven Phase 6 evidence run. The model is already serving on a
# credential-free loopback endpoint; this script owns only the demo site,
# launcher, human frontend, and agent it starts. It retains every artifact.
set -euo pipefail

usage() {
    printf 'usage: %s <ollama|huggingface|llamacpp> <model> <loopback-v1-base> [new-output-dir] [demo-port]\n' "$0" >&2
    printf 'The model server must already be running. The script pauses until a person sees the BlueIce window.\n' >&2
}

if [[ ${1:-} == --help || ${1:-} == -h ]]; then
    usage
    exit 0
fi
if (( $# < 3 || $# > 5 )); then
    usage
    exit 2
fi

provider=$1
model=$2
model_base=$3
demo_port=${5:-4312}
case "$provider" in
    ollama) provider_args=(--ollama-base "$model_base") ;;
    huggingface) provider_args=(--huggingface-base "$model_base") ;;
    llamacpp) provider_args=(--llamacpp-base "$model_base") ;;
    *) usage; exit 2 ;;
esac
if [[ -z $model || ! $demo_port =~ ^[0-9]+$ ]] || (( demo_port < 1 || demo_port > 65535 )); then
    usage
    exit 2
fi

script_dir=$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)
repo_root=$(cd "$script_dir/../../.." && pwd)
bin_dir=$repo_root/target/debug
if [[ -n ${4:-} ]]; then
    run_dir=$4
    mkdir -m 700 -- "$run_dir"
else
    run_dir=$(mktemp -d -t blueice-phase6-human.XXXXXX)
fi
run_dir=$(cd "$run_dir" && pwd)
mkdir -m 700 -- "$run_dir/config"
launcher_socket=$run_dir/launcher.sock
transcript=$run_dir/agent.jsonl
evidence_dir=$run_dir/mcp-png
site_pid=
launcher_pid=
frontend_pid=
agent_pid=

# The launcher is put in its own process group. If the ordinary IPC Shutdown
# fails, the fallback affects only this script's verified launcher tree, not
# other BlueIce sessions or the operator's local model server.
stop_launcher_group() {
    if [[ -n $launcher_pid ]] && kill -0 "$launcher_pid" 2>/dev/null; then
        python3 - "$launcher_pid" <<'PY'
import os
import signal
import sys

try:
    os.killpg(int(sys.argv[1]), signal.SIGTERM)
except ProcessLookupError:
    pass
PY
    fi
}

shutdown_shared_core() {
    if [[ ! -S $launcher_socket ]]; then
        return
    fi
    python3 - "$launcher_socket" <<'PY' || true
import json
import socket
import struct
import sys
import time

path = sys.argv[1]
with socket.socket(socket.AF_UNIX, socket.SOCK_STREAM) as peer:
    peer.settimeout(2)
    peer.connect(path)

    def send(message):
        payload = json.dumps({"message": message}, separators=(",", ":")).encode()
        peer.sendall(struct.pack("<I", len(payload)) + payload)

    def read_exact(size):
        result = bytearray()
        while len(result) < size:
            chunk = peer.recv(size - len(result))
            if not chunk:
                raise EOFError("launcher closed during shutdown handshake")
            result.extend(chunk)
        return bytes(result)

    # Keep this operator-only helper in lockstep with blueice-ipc's v1
    # length-prefixed ClientEnvelope; do not send Shutdown after a mismatch.
    send({"Hello": {"protocol_version": 1}})
    deadline = time.monotonic() + 2
    while time.monotonic() < deadline:
        length = struct.unpack("<I", read_exact(4))[0]
        if not 0 < length <= 8 * 1024 * 1024:
            raise ValueError("invalid launcher handshake frame")
        reply = json.loads(read_exact(length))
        message = reply.get("message")
        if message == {"Hello": {"protocol_version": 1}}:
            break
        if isinstance(message, dict) and "Error" in message:
            raise ValueError(f"launcher refused the shutdown handshake: {message!r}")
    else:
        raise TimeoutError("launcher did not confirm the shutdown handshake")
    send("Shutdown")
PY
}

cleanup() {
    local pid
    if [[ -n $agent_pid ]] && kill -0 "$agent_pid" 2>/dev/null; then
        kill "$agent_pid" 2>/dev/null || true
        wait "$agent_pid" 2>/dev/null || true
    fi
    if [[ -n $frontend_pid ]] && kill -0 "$frontend_pid" 2>/dev/null; then
        kill "$frontend_pid" 2>/dev/null || true
        wait "$frontend_pid" 2>/dev/null || true
    fi
    if [[ -n $launcher_pid ]]; then
        if kill -0 "$launcher_pid" 2>/dev/null; then
            shutdown_shared_core
            for (( pid = 0; pid < 50; pid++ )); do
                if ! kill -0 "$launcher_pid" 2>/dev/null; then
                    break
                fi
                sleep 0.1
            done
        fi
        stop_launcher_group
        wait "$launcher_pid" 2>/dev/null || true
    fi
    if [[ -n $site_pid ]] && kill -0 "$site_pid" 2>/dev/null; then
        kill "$site_pid" 2>/dev/null || true
        wait "$site_pid" 2>/dev/null || true
    fi
    rm -f -- "$launcher_socket" "$run_dir/control.sock"
    printf 'Phase 6 artifacts retained at %s\n' "$run_dir" >&2
}
trap cleanup EXIT
trap 'exit 130' INT
trap 'exit 143' TERM

printf 'Building the local BlueIce binaries...\n'
(
    cd "$repo_root"
    cargo build -p blueice-launcher -p blueice-frontend-reference -p blueice-engine \
        -p blueice-bluejs -p blueice-ai-gatekeeper -p blueice-mcp-server --bins
)

# Validate the same provider URL that the real driver will use, and fail
# before opening the human window if its local server is not listening.
"$bin_dir/blueice-phase6-agent" --provider "$provider" "${provider_args[@]}" \
    --model "$model" --demo-url "http://127.0.0.1:$demo_port/index.html" \
    --launcher-socket "$launcher_socket" --mcp-server "$bin_dir/blueice-mcp-server" \
    --transcript "$transcript" --evidence-dir "$evidence_dir" --preflight-only

python3 -m http.server "$demo_port" --bind 127.0.0.1 \
    --directory "$script_dir/demo-site" >"$run_dir/site.log" 2>&1 &
site_pid=$!
site_matches() {
    curl --noproxy '*' --fail --silent --max-time 1 \
        "http://127.0.0.1:$demo_port/index.html" \
        | cmp -s - "$script_dir/demo-site/index.html"
}
for (( attempt = 0; attempt < 50; attempt++ )); do
    if ! kill -0 "$site_pid" 2>/dev/null; then
        printf 'The loopback demo site could not start; see %s/site.log\n' "$run_dir" >&2
        exit 1
    fi
    if site_matches; then
        break
    fi
    sleep 0.1
done
if ! kill -0 "$site_pid" 2>/dev/null || ! site_matches; then
    printf 'The loopback demo site did not become ready.\n' >&2
    exit 1
fi

XDG_CONFIG_HOME="$run_dir/config" python3 - "$bin_dir/blueice-launcher" \
    --socket "$launcher_socket" --control-socket "$run_dir/control.sock" \
    --frame-dir "$run_dir/frames" >"$run_dir/launcher.log" 2>&1 <<'PY' &
import os
import sys

os.setsid()
os.execv(sys.argv[1], sys.argv[1:])
PY
launcher_pid=$!
for (( attempt = 0; attempt < 150; attempt++ )); do
    if ! kill -0 "$launcher_pid" 2>/dev/null; then
        printf 'The launcher exited early; see %s/launcher.log\n' "$run_dir" >&2
        exit 1
    fi
    if [[ -S $launcher_socket ]]; then
        break
    fi
    sleep 0.1
done
if [[ ! -S $launcher_socket ]]; then
    printf 'The launcher did not become ready; see %s/launcher.log\n' "$run_dir" >&2
    exit 1
fi

"$bin_dir/blueice-frontend" --socket "$launcher_socket" --show-generation \
    about:credits >"$run_dir/frontend.log" 2>&1 &
frontend_pid=$!
sleep 2
if ! kill -0 "$frontend_pid" 2>/dev/null; then
    printf 'The human frontend exited early; see %s/frontend.log\n' "$run_dir" >&2
    exit 1
fi

printf '\nThe BlueIce window should be visible with an SRC/TAB/GEN badge.\n'
printf 'When you can see it, press Enter here to start the local AI run.\n'
if ! IFS= read -r _confirmation; then
    printf 'A person must confirm that the visible window is ready.\n' >&2
    exit 1
fi

"$bin_dir/blueice-phase6-agent" --provider "$provider" "${provider_args[@]}" \
    --model "$model" --demo-url "http://127.0.0.1:$demo_port/index.html" \
    --launcher-socket "$launcher_socket" --mcp-server "$bin_dir/blueice-mcp-server" \
    --transcript "$transcript" --evidence-dir "$evidence_dir" \
    --highlight-hold-seconds 90 >"$run_dir/agent.log" 2>&1 &
agent_pid=$!
notified=0
while kill -0 "$agent_pid" 2>/dev/null; do
    if (( notified == 0 )) && [[ -f $transcript ]] \
        && rg --quiet '"kind":"highlight_hold"' "$transcript"; then
        notified=1
        printf '\nThe agent has highlighted the Name field. Capture ONLY the BlueIce window now.\n'
        printf 'On macOS, use Shift-Command-4, then Space, then click that window.\n'
        printf 'The highlighted frame is held for 90 seconds. Save the PNG for comparison.\n\n'
    fi
    sleep 1
done
if wait "$agent_pid"; then
    agent_pid=
else
    agent_pid=
    printf 'The local AI run failed; see %s/agent.log and the retained transcript.\n' "$run_dir" >&2
    exit 1
fi

printf '\nAgent evidence frames (compare the highlighted SRC/TAB/GEN with the human screenshot):\n'
python3 - "$transcript" <<'PY'
import json
import sys

highlight = None
highlight_png = None
with open(sys.argv[1], encoding="utf-8") as transcript:
    for line in transcript:
        entry = json.loads(line)
        kind = entry.get("kind")
        if kind not in ("highlight_frame", "evidence_saved"):
            continue
        data = entry["data"]
        identity = (data["frame_source"], data["tab_id"], data["generation"])
        print(f"{kind}: SRC:{identity[0]:016X} TAB:{identity[1]} GEN:{identity[2]} {data.get('path', '')}")
        if kind == "highlight_frame":
            highlight = identity
        elif highlight is not None:
            highlight_png = identity
if highlight is None or highlight_png != highlight:
    raise SystemExit("the highlighted snapshot and retained MCP PNG do not match")
PY
printf '\nPath to your independently captured BlueIce-window PNG (Enter if none): '
IFS= read -r human_png || true
if [[ -n ${human_png:-} ]]; then
    if [[ ! -f $human_png ]]; then
        printf 'No screenshot at %s; the human evidence is still incomplete.\n' "$human_png" >&2
        exit 1
    fi
    python3 - "$human_png" <<'PY'
import sys

with open(sys.argv[1], "rb") as screenshot:
    if screenshot.read(8) != b"\x89PNG\r\n\x1a\n":
        raise SystemExit("the supplied human-window screenshot is not a PNG")
PY
    if [[ $human_png != "$run_dir/human-window.png" ]]; then
        cp -n -- "$human_png" "$run_dir/human-window.png"
    fi
    printf 'Read the badge FROM THAT WINDOW PNG and type it as SRC:16HEX TAB:number GEN:number: '
    if ! IFS= read -r observed_badge; then
        printf 'No badge transcription was supplied; the human evidence is still incomplete.\n' >&2
        exit 1
    fi
    python3 "$script_dir/verify-human-evidence.py" "$transcript" \
        "$run_dir/human-window.png" "$observed_badge" \
        "$run_dir/human-evidence-report.json"
    shasum -a 256 "$run_dir/human-window.png" "$evidence_dir"/*.png \
        >"$run_dir/evidence.sha256"
    printf 'Copied screenshot, badge report, and hashes to %s; visually audit that the PNG actually shows the BlueIce window.\n' "$run_dir"
else
    printf 'No human screenshot supplied; Phase 6 same-window evidence remains open.\n'
fi
