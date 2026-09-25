#!/usr/bin/env bash
# Starts a full BlueIce stack for debugging, with fixed socket paths so Claude
# Code's MCP server (see .mcp.json) can attach to it, and with every debug aid on.
#
#   scripts/dev-stack.sh start [--no-window]   build, then start
#   scripts/dev-stack.sh status                what is running, plus the launcher's own status
#   scripts/dev-stack.sh logs                  follow the launcher trace
#   scripts/dev-stack.sh stop
#
# Everything lives under $BLUEICE_DEV_DIR (default /tmp/blueice-dev):
#   rv.sock, ctl.sock   the rendezvous and operator-control sockets
#   trace.log           the launcher's event trace (BLUEICE_TRACE)
#   launcher.log        the launcher's stderr, including core/frontend messages
#   snapshots/native-window.png   what the native window is drawing (BLUEICE_FRONTEND_SNAPSHOT)
#   assistant-settings.json       the assistant's settings file
set -euo pipefail

root="$(cd "$(dirname "$0")/.." && pwd)"
dir="${BLUEICE_DEV_DIR:-/tmp/blueice-dev}"
bin="$root/target/debug"
pidfile="$dir/launcher.pid"

running() { [[ -f "$pidfile" ]] && kill -0 "$(cat "$pidfile")" 2>/dev/null; }

start() {
  local window=1
  [[ "${1:-}" == "--no-window" ]] && window=0
  if running; then echo "already running (pid $(cat "$pidfile")); stop it first"; exit 1; fi
  (cd "$root" && cargo build -p blueice-launcher -p blueice-engine -p blueice-ai-assistant \
      -p blueice-ai-gatekeeper -p blueice-frontend-reference -p blueice-mcp-server -p blueice-bluejs)
  mkdir -p "$dir/snapshots"
  chmod 700 "$dir"
  rm -f "$dir"/rv.sock "$dir"/ctl.sock "$dir/trace.log"
  if [[ ! -f "$dir/assistant-settings.json" ]]; then
    cat > "$dir/assistant-settings.json" <<'JSON'
{"version":1,"backend":"loopback",
 "loopback":{"provider":"llamacpp","base_url":"http://127.0.0.1:8080/v1/","model":"local"},
 "candle":null,"idle_timeout_secs":600,"max_resident_mb":null,"nice":10}
JSON
    chmod 600 "$dir/assistant-settings.json"
  fi

  # WSLg's Wayland connection drops intermittently ("Io error: Broken pipe"),
  # so default to the X11 backend there. BLUEICE_DEV_WAYLAND=1 opts back in.
  local display_env=()
  if [[ -z "${BLUEICE_DEV_WAYLAND:-}" ]] && grep -qi microsoft /proc/version 2>/dev/null; then
    display_env=(WAYLAND_DISPLAY= WINIT_UNIX_BACKEND=x11)
  fi

  local args=(--socket "$dir/rv.sock" --control-socket "$dir/ctl.sock"
              --frame-dir "$dir/frames" --width 900 --height 600
              --assistant-settings "$dir/assistant-settings.json")
  [[ $window -eq 1 ]] && args+=(--trusted-frontend)

  env "${display_env[@]}" RUST_BACKTRACE=full \
      BLUEICE_TRACE="$dir/trace.log" \
      BLUEICE_FRONTEND_SNAPSHOT="$dir/snapshots" \
      "$bin/blueice-launcher" "${args[@]}" > "$dir/launcher.log" 2>&1 &
  echo $! > "$pidfile"
  for _ in $(seq 1 100); do
    [[ -S "$dir/rv.sock" && -S "$dir/ctl.sock" ]] && break
    sleep 0.1
  done
  if [[ -S "$dir/ctl.sock" ]]; then
    echo "up: launcher pid $(cat "$pidfile")"
    echo "  rendezvous $dir/rv.sock   control $dir/ctl.sock"
    echo "  trace      $dir/trace.log   log $dir/launcher.log"
    echo "  snapshot   $dir/snapshots/native-window.png"
  else
    echo "the launcher did not come up; last log lines:"; tail -20 "$dir/launcher.log"; exit 1
  fi
}

stop() {
  if running; then
    kill "$(cat "$pidfile")" 2>/dev/null || true
    for _ in $(seq 1 50); do running || break; sleep 0.1; done
    running && kill -9 "$(cat "$pidfile")" 2>/dev/null || true
  fi
  rm -f "$pidfile" "$dir"/rv.sock "$dir"/ctl.sock
  echo stopped
}

case "${1:-}" in
  start) start "${2:-}" ;;
  stop) stop ;;
  status)
    if running; then echo "launcher running, pid $(cat "$pidfile")"; else echo "launcher not running"; fi
    [[ -S "$dir/ctl.sock" ]] && python3 "$root/scripts/dev-control.py" --socket "$dir/ctl.sock" status || true ;;
  logs) tail -n 50 -F "$dir/trace.log" ;;
  *) sed -n '2,12p' "$0"; exit 2 ;;
esac
