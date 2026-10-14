# This Source Code Form is subject to the terms of the Mozilla Public
# License, v. 2.0. If a copy of the MPL was not distributed with this
# file, You can obtain one at https://mozilla.org/MPL/2.0/.

"""Owned loopback SFTP fixture; never changes system or user SSH settings."""
import argparse, getpass, json, os, select, signal, socket, subprocess, sys, time
from pathlib import Path

PASSPHRASE = "blueice-local-key-fixture"
PAYLOAD = "BlueIce authenticated SFTP 下載 fixture\n".encode()
parser = argparse.ArgumentParser()
parser.add_argument("--root", required=True)
parser.add_argument("--smoke", action="store_true")
args = parser.parse_args()
root = Path(args.root)
assert root.is_dir() and not root.is_symlink() and root.stat().st_uid == os.getuid()
assert root.stat().st_mode & 0o777 == 0o700
server = None
log = None

def command(values, **kwargs):
    return subprocess.run(values, check=True, capture_output=True, timeout=20, **kwargs)

def interrupt(signum, frame):
    raise SystemExit(128 + signum)

signal.signal(signal.SIGTERM, interrupt)
signal.signal(signal.SIGINT, interrupt)
try:
    host, key = root / "host", root / "private-key"
    command(["/usr/bin/ssh-keygen", "-q", "-t", "ed25519", "-N", "", "-f", str(host)])
    command(["/usr/bin/ssh-keygen", "-q", "-t", "rsa", "-b", "2048", "-m", "PEM", "-N", PASSPHRASE, "-f", str(key)])
    authorized = root / "authorized_keys"
    authorized.write_bytes(Path(str(key) + ".pub").read_bytes())
    authorized.chmod(0o600)
    with socket.socket() as listener:
        listener.bind(("127.0.0.1", 0))
        port = listener.getsockname()[1]
    known = root / "known_hosts"
    known.write_text("[127.0.0.1]:" + str(port) + " " + Path(str(host) + ".pub").read_text())
    payload = root / "payload.txt"
    payload.write_bytes(PAYLOAD)
    username = getpass.getuser()
    config = root / "sshd_config"
    # The fixture root and files remain 0700/0600. StrictModes is disabled only
    # in this private daemon because /private/tmp has a shared writable parent.
    # The client still requires the exact generated known-host key and public key.
    config.write_text("\n".join([
        "Port " + str(port), "ListenAddress 127.0.0.1", "HostKey " + str(host),
        "PidFile " + str(root / "pid"), "AuthorizedKeysFile " + str(authorized),
        "PasswordAuthentication no", "KbdInteractiveAuthentication no", "UsePAM no",
        "StrictModes no", "PubkeyAuthentication yes", "AuthenticationMethods publickey",
        "PermitRootLogin no", "AllowUsers " + username, "DisableForwarding yes",
        "X11Forwarding no", "PrintMotd no", "UseDNS no", "LogLevel ERROR",
        "ForceCommand internal-sftp", "Subsystem sftp internal-sftp",
    ]) + "\n")
    command(["/usr/sbin/sshd", "-t", "-f", str(config)])
    log = (root / "server.log").open("wb")
    server = subprocess.Popen(["/usr/sbin/sshd", "-D", "-e", "-f", str(config)],
                              stdout=log, stderr=subprocess.STDOUT, start_new_session=True)
    ready = False
    for _ in range(50):
        if server.poll() is not None:
            break
        try:
            with socket.create_connection(("127.0.0.1", port), timeout=0.2):
                ready = True
                break
        except OSError:
            time.sleep(0.1)
    assert ready, "Owned SFTP fixture did not listen"
    if args.smoke:
        clear = root / "smoke-key"
        clear.write_bytes(key.read_bytes())
        clear.chmod(0o600)
        command(["/usr/bin/ssh-keygen", "-p", "-P", PASSPHRASE, "-N", "", "-f", str(clear)])
        output = root / "smoke-output.txt"
        command(["/usr/bin/sftp", "-F", "/dev/null", "-b", "-", "-P", str(port),
                 "-i", str(clear), "-o", "IdentityAgent=none", "-o", "IdentitiesOnly=yes",
                 "-o", "StrictHostKeyChecking=yes", "-o", "UserKnownHostsFile=" + str(known),
                 "-o", "GlobalKnownHostsFile=/dev/null", username + "@127.0.0.1"],
                input=('get "' + str(payload) + '" "' + str(output) + '"\n').encode())
        assert output.read_bytes() == PAYLOAD
        print(json.dumps({"smoke": "passed", "bytes": len(PAYLOAD)}), flush=True)
    else:
        print(json.dumps({"port": port, "username": username, "private_key": str(key),
                          "known_hosts": str(known), "url": "sftp://" + username + "@127.0.0.1:" + str(port) + str(payload),
                          "server_pid": server.pid}), flush=True)
        while server.poll() is None:
            readable, _, _ = select.select([sys.stdin], [], [], 0.2)
            if readable and not sys.stdin.buffer.read(1):
                break
finally:
    if server is not None:
        if server.poll() is None:
            os.killpg(server.pid, signal.SIGTERM)
            try:
                server.wait(timeout=5)
            except subprocess.TimeoutExpired:
                os.killpg(server.pid, signal.SIGKILL)
                server.wait()
    if log is not None:
        log.close()
