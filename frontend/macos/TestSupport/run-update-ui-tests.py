# This Source Code Form is subject to the terms of the Mozilla Public
# License, v. 2.0. If a copy of the MPL was not distributed with this
# file, You can obtain one at https://mozilla.org/MPL/2.0/.

"""Own the update fixture's files outside the sandboxed XCUITest Runner."""
import hmac
import ctypes
from http.server import BaseHTTPRequestHandler, HTTPServer
import json
import os
from pathlib import Path
import secrets
import shutil
import signal
import subprocess
import sys
import tempfile
import threading
from urllib.parse import parse_qs, urlsplit
import stat
import socket
import struct
import time


HEADER = """#!/bin/sh
# This Source Code Form is subject to the terms of the Mozilla Public
# License, v. 2.0. If a copy of the MPL was not distributed with this
# file, You can obtain one at https://mozilla.org/MPL/2.0/.

"""
CORE = HEADER + '''DIR="$(dirname "$0")"
n=$(cat "$DIR/count" 2>/dev/null || echo 0)
n=$((n+1))
echo "$n" > "$DIR/count"
if [ "$n" -eq 1 ]; then
    echo "$$" > "$DIR/serving.pid"
else
    echo "$$" > "$DIR/pending.pid.staged"
    mv "$DIR/pending.pid.staged" "$DIR/pending.pid"
    while [ ! -e "$DIR/allow-start" ]; do sleep 0.01; done
fi
exec "$DIR/blueice-core-real" "$@"
'''
LAUNCHER = HEADER + '''DIR="$(dirname "$0")"
echo "$$" > "$DIR/launcher.pid"
exec "$DIR/blueice-launcher" "$@" --auto-update-secs 1 > "$DIR/launcher.log" 2>&1
'''


def executable(path, contents):
    path.write_text(contents)
    path.chmod(0o700)


def alive(pid):
    try:
        os.kill(pid, 0)
        return True
    except ProcessLookupError:
        return False


def interrupted(signum, frame):
    raise SystemExit(128 + signum)


def read_core_representation(runtime, context, window, tab):
    """Read shared core state through the ordinary scoped protocol only."""
    with socket.socket(socket.AF_UNIX, socket.SOCK_STREAM) as connection:
        connection.settimeout(2)
        connection.connect(str(runtime / "browser.sock"))
        def exchange(command, request, scoped):
            if scoped:
                command = {"BrowserContext": {"Command": {"context_id": context,
                    "message": {"Window": {"Command": {"window_id": window, "message": command}}}}}}
            envelope = {"request_id": request, "message": command}
            if scoped:
                envelope["tab_id"] = tab
            data = json.dumps(envelope).encode()
            connection.sendall(struct.pack("<I", len(data)) + data)
            deadline = time.monotonic() + 2
            def read_exactly(count):
                result = bytearray()
                while len(result) < count:
                    remaining = deadline - time.monotonic()
                    if remaining <= 0:
                        raise TimeoutError("core observation deadline")
                    connection.settimeout(remaining)
                    piece = connection.recv(count - len(result))
                    if not piece:
                        raise EOFError("core observation disconnected")
                    result.extend(piece)
                return result
            for _ in range(64):
                count, = struct.unpack("<I", read_exactly(4))
                assert 0 < count <= 16 * 1024 * 1024
                reply = json.loads(read_exactly(count))
                if reply.get("request_id") != request:
                    continue
                assert not scoped or reply.get("tab_id") == tab
                assert isinstance(reply.get("message"), dict) and "Error" not in reply["message"]
                return reply["message"]
            raise TimeoutError("core observation reply count")
        assert exchange({"Hello": {"protocol_version": 2}}, 1, False) == {"Hello": {"protocol_version": 2}}
        message = exchange("GetRepresentation", 2, True)
        assert message["Representation"]["tab_id"] == tab
        return message["Representation"]


assert len(sys.argv) >= 3, "Provide the real service bundle and xcodebuild command"
bundle = Path(sys.argv[1]).resolve(strict=True)
assert bundle.name == "BlueIce.app" and bundle.is_dir()
command = sys.argv[2:]
root = Path(tempfile.mkdtemp(prefix="bi-update-ui-server-", dir="/private/tmp"))
root.chmod(0o700)
directory = root / "BlueIce.app/Contents/MacOS"
token = secrets.token_hex(32)
server = thread = child = None
exit_code = 1
finished = False
signal.signal(signal.SIGINT, interrupted)
signal.signal(signal.SIGTERM, interrupted)


class Handler(BaseHTTPRequestHandler):
    def setup(self):
        self.request.settimeout(5)
        super().setup()

    def log_message(self, format, *args):
        pass

    def do_POST(self):
        global finished
        if not hmac.compare_digest(self.headers.get("Authorization", ""), "Bearer " + token):
            self.send_error(403)
            return
        if self.headers.get("Content-Length", "0") != "0":
            self.send_error(400)
            return
        route = urlsplit(self.path).path
        if route in ("/runtime", "/hover"):
            try:
                query = parse_qs(urlsplit(self.path).query, strict_parsing=True)
                assert set(query) == ({"launcher", "application"} if route == "/runtime" else {"launcher", "application", "context", "window", "tab"})
                assert all(len(values) == 1 and values[0].isdecimal() for values in query.values())
                launcher, application = int(query["launcher"][0]), int(query["application"][0])
                assert launcher > 1 and application > 1
                libproc = ctypes.CDLL("/usr/lib/libproc.dylib")
                def executable_path(pid):
                    value = ctypes.create_string_buffer(4096)
                    assert libproc.proc_pidpath(pid, value, len(value)) > 0
                    return Path(os.fsdecode(value.value)).resolve(strict=True)
                def parent_pid():
                    value = subprocess.check_output(["/bin/ps", "-p", str(launcher), "-o", "ppid="], text=True, timeout=2)
                    return int(value.strip())
                assert executable_path(application) == bundle / "Contents/MacOS/BlueIce"
                assert executable_path(launcher) == bundle / "Contents/MacOS/blueice-launcher"
                assert parent_pid() == application
                names = subprocess.check_output(["/usr/sbin/lsof", "-n", "-P", "-a", "-p", str(launcher), "-U", "-Fn"], text=True, timeout=2)
                sockets = {line[1:] for line in names.splitlines()
                           if line.startswith("n/private/tmp/bi-") and line.endswith("/browser.sock")}
                assert len(sockets) == 1
                socket = Path(sockets.pop())
                assert stat.S_ISSOCK(socket.stat().st_mode)
                runtime = socket.parent.resolve(strict=True)
                info = runtime.stat()
                assert info.st_uid == os.getuid() and stat.S_IMODE(info.st_mode) == 0o700
                assert parent_pid() == application
                if route == "/hover":
                    context, window, tab = (int(query[name][0]) for name in ("context", "window", "tab"))
                    assert all(0 < value < 2**64 for value in (context, window, tab))
                    data = json.dumps(read_core_representation(runtime, context, window, tab)).encode()
                else:
                    data = json.dumps({"runtime": str(runtime), "launcher": launcher, "application": application}).encode()
                self.send_response(200)
                self.send_header("Content-Length", str(len(data)))
                self.end_headers()
                self.wfile.write(data)
            except (AssertionError, OSError, ValueError, KeyError, EOFError, subprocess.SubprocessError) as error:
                self.send_error(400, "Owned runtime lookup failed: " + type(error).__name__)
            return
        if self.path == "/update" and not finished:
            staged = directory / "blueice-core.staged"
            executable(staged, CORE + "# stable replacement build\n")
            os.replace(staged, directory / "blueice-core")
        elif self.path == "/finish":
            finished = True
        else:
            self.send_error(404)
            return
        self.send_response(200)
        self.send_header("Content-Length", "0")
        self.end_headers()


try:
    shutil.copytree(bundle, root / "BlueIce.app")
    (directory / "blueice-core").rename(directory / "blueice-core-real")
    executable(directory / "blueice-core", CORE)
    executable(directory / "blueice-launcher-wrapper", LAUNCHER)
    check = subprocess.run([str(directory / "blueice-launcher"), "--help"],
                           stdout=subprocess.DEVNULL, stderr=subprocess.PIPE, timeout=10)
    # This launcher deliberately has no help command. Its parser rejection
    # proves the private executable can run without starting any service.
    assert check.returncode == 1 and "unrecognized argument: --help" in check.stderr.decode(errors="replace"), check.stderr.decode(errors="replace")
    server = HTTPServer(("127.0.0.1", 0), Handler)
    thread = threading.Thread(target=server.serve_forever)
    thread.start()
    metadata = root / "ready.json"
    metadata.write_text(json.dumps({"port": server.server_port, "token": token,
                                   "launcher": str(directory / "blueice-launcher-wrapper")}) + "\n")
    metadata.chmod(0o600)
    child = subprocess.Popen(command + ["BLUEICE_UPDATE_FIXTURE_METADATA=" + str(metadata)],
                             start_new_session=True)
    print("AUTOMATIC_UPDATE_FIXTURE_SERVER_STARTED=" + json.dumps({"root": str(root), "pid": os.getpid(),
                                                                 "child_pid": child.pid, "child_group": child.pid}), flush=True)
    exit_code = child.wait()
finally:
    if child is not None and child.poll() is None:
        os.killpg(child.pid, signal.SIGTERM)
        try:
            child.wait(timeout=10)
        except subprocess.TimeoutExpired:
            os.killpg(child.pid, signal.SIGKILL)
            child.wait(timeout=10)
    if server is not None:
        if thread is not None and thread.is_alive():
            server.shutdown()
        server.server_close()
    if thread is not None:
        thread.join(timeout=5)
        assert not thread.is_alive(), "Update fixture server did not stop"
    owned = {}
    for name in ("launcher", "serving", "pending"):
        marker = directory / (name + ".pid")
        if marker.exists():
            value = int(marker.read_text().strip())
            assert value > 1
            owned[name] = value
    remaining = [pid for pid in owned.values() if alive(pid)]
    group_gone = "launcher" not in owned or not alive(-owned["launcher"])
    if exit_code and (directory / "launcher.log").exists():
        print("AUTOMATIC_UPDATE_FIXTURE_SERVER_DIAGNOSTICS=" +
              (directory / "launcher.log").read_text(errors="replace")[-16384:], flush=True)
    shutil.rmtree(root)
    cleanup = {"root_removed": not root.exists(), "server_thread_gone": thread is None or not thread.is_alive(),
               "owned_pids": owned, "remaining_owned_pids": remaining, "owned_group_gone": group_gone,
               "command_group_gone": child is None or not alive(-child.pid)}
    print("AUTOMATIC_UPDATE_FIXTURE_SERVER_CLEANUP=" + json.dumps(cleanup), flush=True)
    assert cleanup["root_removed"] and cleanup["server_thread_gone"] and not remaining and group_gone and cleanup["command_group_gone"]
raise SystemExit(exit_code)
