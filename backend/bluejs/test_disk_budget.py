# This Source Code Form is subject to the terms of the Mozilla Public
# License, v. 2.0. If a copy of the MPL was not distributed with this
# file, You can obtain one at https://mozilla.org/MPL/2.0/.

"""Bound each run to 16 GiB target and host growth, 64 GiB target, and 20 GiB free.

Usage: python3 backend/bluejs/test_disk_budget.py -- cargo test --workspace
"""

import os
import shutil
import subprocess
import sys
import tempfile
from pathlib import Path

sys.dont_write_bytecode = True

from coverage_file import (
    REPO_ROOT,
    cargo_target,
    prune_superseded_test_executables,
    run_bounded_command,
    strip_test_binary_debug,
)


def main() -> int:
    argv = sys.argv[1:]
    if argv and argv[0] == "--":
        argv = argv[1:]
    if not argv:
        print("provide a test command after --", file=sys.stderr)
        return 2
    target = cargo_target()
    try:
        with tempfile.TemporaryDirectory(prefix="bluejs-test-profiles-") as profiles:
            original_profile = os.environ.get("LLVM_PROFILE_FILE")
            os.environ["LLVM_PROFILE_FILE"] = str(Path(profiles) / "%m-%p.profraw")
            try:
                run_bounded_command(argv, cwd=REPO_ROOT, target=target)
            finally:
                if original_profile is None:
                    os.environ.pop("LLVM_PROFILE_FILE", None)
                else:
                    os.environ["LLVM_PROFILE_FILE"] = original_profile
    finally:
        if target.is_relative_to(REPO_ROOT):
            for path in (
                target / "debug/incremental",
                target / "llvm-cov-target/debug/incremental",
            ):
                if path.exists():
                    shutil.rmtree(path)
            prune_superseded_test_executables(target)
            strip_test_binary_debug(target)
    return 0


if __name__ == "__main__":
    try:
        raise SystemExit(main())
    except subprocess.CalledProcessError as error:
        raise SystemExit(error.returncode) from error
    except (OSError, RuntimeError) as error:
        print(f"test_disk_budget.py: {error}", file=sys.stderr)
        raise SystemExit(1) from error
