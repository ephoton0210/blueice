#!/usr/bin/env python3
# This Source Code Form is subject to the terms of the Mozilla Public
# License, v. 2.0. If a copy of the MPL was not distributed with this
# file, You can obtain one at https://mozilla.org/MPL/2.0/.
"""Read the real owned GUI/Core viewport and representation without changing it."""
import argparse
import ctypes
import json
import os
from pathlib import Path
import socket
import stat
import struct
import subprocess
import time

parser = argparse.ArgumentParser()
parser.add_argument('runtime', type=Path)
parser.add_argument('launcher', type=int)
parser.add_argument('application', type=int)
parser.add_argument('bundle', type=Path)
parser.add_argument('--context', type=int, default=1)
parser.add_argument('--window', type=int, default=1)
parser.add_argument('--tab', type=int, default=1)
args = parser.parse_args()
assert args.launcher > 1 and args.application > 1
assert all(0 < n < 2**64 for n in (args.context,args.window,args.tab))
bundle = args.bundle.resolve(strict=True)
runtime = args.runtime.resolve(strict=True)
assert bundle.name == 'BlueIce.app'
assert str(runtime).startswith('/private/tmp/bi-')
info = runtime.stat()
assert info.st_uid == os.getuid() and stat.S_IMODE(info.st_mode) == 0o700
assert stat.S_ISSOCK((runtime/'browser.sock').stat().st_mode)
libproc = ctypes.CDLL('/usr/lib/libproc.dylib')
def executable(pid):
    value = ctypes.create_string_buffer(4096)
    assert libproc.proc_pidpath(pid,value,len(value)) > 0
    return Path(os.fsdecode(value.value)).resolve(strict=True)
def verify():
    assert executable(args.application) == bundle/'Contents/MacOS/BlueIce'
    assert executable(args.launcher) == bundle/'Contents/MacOS/blueice-launcher'
    parent = subprocess.check_output(['/bin/ps','-p',str(args.launcher),'-o','ppid='],text=True,timeout=2)
    assert int(parent.strip()) == args.application
    assert os.getpgid(args.launcher) == args.launcher
verify()
with socket.socket(socket.AF_UNIX,socket.SOCK_STREAM) as connection:
    connection.settimeout(2)
    connection.connect(str(runtime/'browser.sock'))
    def exchange(command,request,scoped):
        if scoped:
            command = {'BrowserContext':{'Command':{'context_id':args.context,'message':{'Window':{'Command':{'window_id':args.window,'message':command}}}}}}
        envelope = {'request_id':request,'message':command}
        if scoped: envelope['tab_id'] = args.tab
        data = json.dumps(envelope).encode()
        connection.sendall(struct.pack('<I',len(data))+data)
        deadline = time.monotonic()+2
        def read(size):
            result = bytearray()
            while len(result)<size:
                remaining = deadline-time.monotonic()
                if remaining <= 0: raise TimeoutError('Observation deadline')
                connection.settimeout(remaining)
                chunk = connection.recv(size-len(result))
                if not chunk: raise EOFError('Owned Core disconnected')
                result.extend(chunk)
            return result
        for _ in range(64):
            size, = struct.unpack('<I',read(4)); assert 0<size<=16*1024*1024
            reply = json.loads(read(size))
            if reply.get('request_id') != request: continue
            assert not scoped or reply.get('tab_id') == args.tab
            assert isinstance(reply.get('message'),dict) and 'Error' not in reply['message']
            return reply['message']
        raise TimeoutError('Observation reply limit')
    assert exchange({'Hello':{'protocol_version':2}},1,False) == {'Hello':{'protocol_version':2}}
    viewport = exchange('GetViewportState',2,True)['ViewportState']
    representation = exchange('GetRepresentation',3,True)['Representation']
    assert viewport['tab_id'] == args.tab and representation['tab_id'] == args.tab
verify()
print(json.dumps({'scope':'Read-only actual owned Core state','application':args.application,
                  'launcher':args.launcher,'viewport':viewport,'representation':representation},sort_keys=True))
