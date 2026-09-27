# This Source Code Form is subject to the terms of the Mozilla Public
# License, v. 2.0. If a copy of the MPL was not distributed with this
# file, You can obtain one at https://mozilla.org/MPL/2.0/.

"""Run tests with 16 GiB growth, 140 GiB total Cargo, and 20 GiB free space.

Usage: python3 backend/bluejs/test_disk_budget.py -- cargo test --workspace
"""

import shutil
import subprocess
import sys

sys.dont_write_bytecode = True

from coverage_file import (
    REPO_ROOT,
    cargo_target,
    prune_superseded_test_executables,
    run_bounded_command,
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
        run_bounded_command(argv, cwd=REPO_ROOT, target=target)
    finally:
        if target.is_relative_to(REPO_ROOT):
            for path in (
                target / "debug/incremental",
                target / "llvm-cov-target/debug/incremental",
            ):
                if path.exists():
                    shutil.rmtree(path)
            prune_superseded_test_executables(target)
    return 0


if __name__ == "__main__":
    try:
        raise SystemExit(main())
    except subprocess.CalledProcessError as error:
        raise SystemExit(error.returncode) from error
    except (OSError, RuntimeError) as error:
        print(f"test_disk_budget.py: {error}", file=sys.stderr)
        raise SystemExit(1) from error
