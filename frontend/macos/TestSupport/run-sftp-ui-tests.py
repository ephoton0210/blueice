# This Source Code Form is subject to the terms of the Mozilla Public
# License, v. 2.0. If a copy of the MPL was not distributed with this
# file, You can obtain one at https://mozilla.org/MPL/2.0/.

"""Own a loopback SFTP daemon outside the sandboxed UI runner for one test run."""
import json
import os
from pathlib import Path
import selectors
import shutil
import signal
import subprocess
import sys
import tempfile


def alive_group(pid):
    try:
        os.killpg(pid, 0)
        return True
    except ProcessLookupError:
        return False


def interrupted(signum, frame):
    raise SystemExit(128 + signum)


signal.signal(signal.SIGINT, interrupted)
signal.signal(signal.SIGTERM, interrupted)
command = sys.argv[1:]
assert command, "Provide the xcodebuild test command"
root = Path(tempfile.mkdtemp(prefix="bi-sftp-ui-server-", dir="/private/tmp"))
root.chmod(0o700)
helper = None
server_pid = None
exit_code = 1
try:
    with (root / "helper.log").open("wb") as diagnostics:
        helper = subprocess.Popen(
            [sys.executable, str(Path(__file__).with_name("sftp-fixture-server.py")), "--root", str(root)],
            stdin=subprocess.PIPE, stdout=subprocess.PIPE, stderr=diagnostics,
            start_new_session=True,
        )
        with selectors.DefaultSelector() as selector:
            selector.register(helper.stdout, selectors.EVENT_READ)
            assert selector.select(15), "SFTP fixture readiness timed out"
        line = helper.stdout.readline(8193)
        assert line and len(line) <= 8192, "SFTP helper stopped before readiness"
        ready = json.loads(line)
        assert 0 < ready["port"] <= 65535 and ready["server_pid"] > 1
        assert all(ready[k].startswith(str(root) + "/") for k in ("private_key", "known_hosts"))
        server_pid = ready["server_pid"]
        metadata = root / "ready.json"
        metadata.write_text(json.dumps(ready) + "\n")
        metadata.chmod(0o600)
        print("SFTP_FIXTURE_STARTED=" + json.dumps({"root": str(root), "helper_pid": helper.pid, "server_pid": server_pid}), flush=True)
        exit_code = subprocess.call(command + ["BLUEICE_SFTP_FIXTURE_METADATA=" + str(metadata)])
finally:
    if helper is not None:
        helper.stdin.close()
        try:
            helper.wait(timeout=8)
        except subprocess.TimeoutExpired:
            os.killpg(helper.pid, signal.SIGTERM)
            helper.wait(timeout=8)
        if server_pid is not None and alive_group(server_pid):
            os.killpg(server_pid, signal.SIGTERM)
            raise RuntimeError("SFTP helper did not reap its owned daemon")
        if exit_code:
            for name in ("helper.log", "server.log"):
                path = root / name
                if path.exists():
                    print("SFTP_FIXTURE_DIAGNOSTICS " + name + ": " + path.read_text(errors="replace")[-16384:].replace("blueice-local-key-fixture", "<fixture passphrase>"), flush=True)
    shutil.rmtree(root)
    clean = {"root_removed": not root.exists(), "helper_exit": helper.returncode if helper else None,
             "helper_group_gone": helper is None or not alive_group(helper.pid),
             "server_group_gone": server_pid is None or not alive_group(server_pid)}
    print("SFTP_FIXTURE_CLEANUP=" + json.dumps(clean), flush=True)
    assert clean["root_removed"] and clean["helper_group_gone"] and clean["server_group_gone"]
    assert helper is None or clean["helper_exit"] == 0, "Owned SFTP helper failed"
raise SystemExit(exit_code)
