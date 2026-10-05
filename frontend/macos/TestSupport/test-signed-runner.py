#!/usr/bin/env python3
# This Source Code Form is subject to the terms of the Mozilla Public
# License, v. 2.0. If a copy of the MPL was not distributed with this
# file, You can obtain one at https://mozilla.org/MPL/2.0/.

"""Verify signature preservation, repair and argument/exit forwarding on macOS."""

import hashlib
import pathlib
import shutil
import subprocess
import tempfile
import time


def main():
    runner = pathlib.Path(__file__).with_name("run-signed-test.sh")
    with tempfile.TemporaryDirectory(prefix="blueice-signed-runner-") as directory:
        directory = pathlib.Path(directory)
        executable = directory / "fixture"
        shutil.copyfile("/bin/sh", executable)
        executable.chmod(0o755)
        subprocess.run(["/usr/bin/codesign", "--verify", "--strict", str(executable)], check=True)
        before = hashlib.sha256(executable.read_bytes()).hexdigest()
        child = subprocess.Popen([str(runner), str(executable), "-c", 'test "$1" = "argument with spaces"',
                                  "fixture", "argument with spaces"])
        try:
            deadline = time.monotonic() + 10
            while child.poll() is None:
                assert hashlib.sha256(executable.read_bytes()).hexdigest() == before, (
                    "A valid executable must retain its existing signature and bytes"
                )
                assert time.monotonic() < deadline, "Runner did not finish the signature check"
                time.sleep(0.01)
            # macOS may reject a relocated platform executable. This part
            # verifies unchanged signing bytes, not approval of the copy.
            assert hashlib.sha256(executable.read_bytes()).hexdigest() == before
        finally:
            if child.poll() is None:
                child.terminate()
            child.wait()
        print("Valid signature bytes preserved", flush=True)
        subprocess.run([str(runner), "/bin/sh", "-c", 'test "$1" = "argument with spaces"',
                        "fixture", "argument with spaces"], check=True)
        assert subprocess.run([str(runner), "/bin/sh", "-c", "exit 53"]).returncode == 53
        print("Argument boundaries and child exit status preserved", flush=True)

        subprocess.run(["/usr/bin/codesign", "--remove-signature", str(executable)], check=True)
        unsigned = hashlib.sha256(executable.read_bytes()).hexdigest()
        child = subprocess.Popen([str(runner), str(executable), "-c", "exit 0"])
        try:
            deadline = time.monotonic() + 10
            while subprocess.run(["/usr/bin/codesign", "--verify", "--strict", str(executable)],
                                 stdout=subprocess.DEVNULL, stderr=subprocess.DEVNULL).returncode != 0:
                assert child.poll() is None, "Runner exited without preparing a valid signature"
                assert time.monotonic() < deadline, "Runner did not prepare a valid signature"
                time.sleep(0.01)
            assert hashlib.sha256(executable.read_bytes()).hexdigest() != unsigned
            print("Unsigned executable prepared with a valid signature", flush=True)
        finally:
            # This check concerns signing before execution. Never wait for or
            # alter macOS approval of this temporary ad-hoc executable.
            if child.poll() is None:
                child.terminate()
            child.wait()


if __name__ == "__main__":
    main()
