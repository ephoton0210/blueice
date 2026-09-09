#!/usr/bin/env python3
# This Source Code Form is subject to the terms of the Mozilla Public
# License, v. 2.0. If a copy of the MPL was not distributed with this
# file, You can obtain one at https://mozilla.org/MPL/2.0/.

"""Fault injection only: exercises the parent protocol's fail-closed behavior."""
import json
import os
import struct
import sys

mode = os.environ["BLUEJS_FAULT_MODE"]


def send(data):
    sys.stdout.buffer.write(struct.pack("<I", len(data)) + data)
    sys.stdout.buffer.flush()


if mode == "early_exit":
    sys.exit(0)
send(b"bad" if mode == "bad_ready" else b"bluejs-regexp-worker/1")
if mode == "after_ready_exit":
    sys.exit(0)
while True:
    length = sys.stdin.buffer.read(4)
    if not length:
        break
    request = json.loads(sys.stdin.buffer.read(struct.unpack("<I", length)[0]))
    if mode == "malformed":
        send(b"{}")
    elif mode == "oversized":
        sys.stdout.buffer.write(struct.pack("<I", 16 * 1024 * 1024 + 1))
        sys.stdout.buffer.flush()
    elif mode == "invalid_range":
        send(b'{"Found":{"captures":[{"start":2,"end":1}],"names":[]}}')
    elif mode == "compile_reply":
        send(b'{"Found":null}')
    elif mode == "match_reply":
        send(b'"Compiled"')
