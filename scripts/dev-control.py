#!/usr/bin/env python3
"""A tiny client for the launcher's operator-control socket (the same protocol
the MCP server uses), for use without an MCP client.

  dev-control.py [--socket PATH] status
  dev-control.py [--socket PATH] settings
  dev-control.py [--socket PATH] propose-nice N     propose a new priority (the person must still approve)
  dev-control.py [--socket PATH] proposal ID
  dev-control.py [--socket PATH] cutover

It can only do what the socket allows: read status, propose (never apply)
settings changes, and trigger a cutover. It cannot approve anything.
"""
import json, os, socket, struct, sys

args = sys.argv[1:]
path = os.path.join(os.environ.get("BLUEICE_DEV_DIR", "/tmp/blueice-dev"), "ctl.sock")
if args[:1] == ["--socket"]:
    path, args = args[1], args[2:]

def ask(request):
    s = socket.socket(socket.AF_UNIX)
    s.settimeout(30)
    s.connect(path)
    body = json.dumps(request).encode()
    s.sendall(struct.pack("<I", len(body)) + body)
    def read(n):
        out = b""
        while len(out) < n:
            chunk = s.recv(n - len(out))
            if not chunk:
                raise SystemExit("the launcher closed the connection")
            out += chunk
        return out
    return json.loads(read(struct.unpack("<I", read(4))[0]))

if not args:
    sys.exit(__doc__)
cmd = args[0]
if cmd == "status":
    print(json.dumps(ask("Status"), indent=1))
elif cmd == "settings":
    print(json.dumps(ask("InspectAssistantSettings"), indent=1))
elif cmd == "cutover":
    print(json.dumps(ask("Cutover"), indent=1))
elif cmd == "proposal":
    print(json.dumps(ask({"AssistantProposalStatus": {"id": int(args[1])}}), indent=1))
elif cmd == "propose-nice":
    current = ask("InspectAssistantSettings")["AssistantSettingsInForce"]["settings"]
    current["nice"] = int(args[1])
    print(json.dumps(ask({"ProposeAssistantSettings": {"settings": current}}), indent=1))
else:
    sys.exit(__doc__)
