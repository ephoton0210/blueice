#!/usr/bin/env bash
# This Source Code Form is subject to the terms of the Mozilla Public
# License, v. 2.0. If a copy of the MPL was not distributed with this
# file, You can obtain one at https://mozilla.org/MPL/2.0/.

# Guard local Rust tests against filling the host filesystem.
set -euo pipefail

if (( $# == 0 )); then
    echo "usage: scripts/test-with-disk-budget.sh cargo test [arguments...]" >&2
    exit 2
fi

for tool in setsid du df ps realpath awk; do
    if ! command -v "$tool" >/dev/null 2>&1; then
        echo "disk budget: required tool '$tool' is unavailable" >&2
        exit 2
    fi
done

max_target_gib=${BLUEICE_TEST_MAX_TARGET_GIB:-60}
min_free_gib=${BLUEICE_TEST_MIN_FREE_GIB:-50}
for value in "$max_target_gib" "$min_free_gib"; do
    if [[ ! $value =~ ^[0-9]+$ ]] || (( 10#$value < 1 )); then
        echo "disk budget: limits must be positive whole GiB values" >&2
        exit 2
    fi
done

repo_root=$(cd -- "$(dirname -- "${BASH_SOURCE[0]}")/.." && pwd -P)
cd -- "$repo_root"
target_dir=$repo_root/target
mkdir -p -- "$target_dir"
if [[ -L $target_dir ]]; then
    echo "disk budget: shared target must not be a symbolic link" >&2
    exit 2
fi

if [[ -n ${CARGO_TARGET_DIR:-} ]] &&
    [[ $(realpath -m -- "$CARGO_TARGET_DIR") != "$target_dir" ]]; then
    echo "disk budget: CARGO_TARGET_DIR must be the shared $target_dir" >&2
    exit 2
fi
export CARGO_TARGET_DIR=$target_dir
export CARGO_INCREMENTAL=0
# Full workspace test binaries otherwise retain tens of GiB of debug data.
# Keep one compact profile in the shared target unless the caller overrides it.
export CARGO_PROFILE_DEV_DEBUG=${CARGO_PROFILE_DEV_DEBUG:-0}
export CARGO_PROFILE_TEST_DEBUG=${CARGO_PROFILE_TEST_DEBUG:-0}

max_target_kib=$((max_target_gib * 1024 * 1024))
min_free_kib=$((min_free_gib * 1024 * 1024))

check_budget() {
    local target_kib free_kib
    # Rust can remove a temporary object during du's walk. The final total is
    # still usable; discard only that transient diagnostic and require a
    # numeric result below.
    target_kib=$(du -sk -- "$target_dir" 2>/dev/null | awk '{ print $1 }') || :
    free_kib=$(df -Pk -- "$target_dir" | awk 'END { print $4 }')
    if [[ ! $target_kib =~ ^[0-9]+$ || ! $free_kib =~ ^[0-9]+$ ]]; then
        echo "disk budget: could not measure target size or free space" >&2
        return 1
    fi
    if (( target_kib >= max_target_kib || free_kib <= min_free_kib )); then
        echo "disk budget exceeded: target ${target_kib} KiB (limit ${max_target_kib} KiB), free ${free_kib} KiB (reserve ${min_free_kib} KiB)" >&2
        return 1
    fi
}

if ! check_budget; then
    exit 75
fi

echo "disk budget: shared target limit ${max_target_gib} GiB, host free-space reserve ${min_free_gib} GiB; incremental cache disabled; dev/test debug info ${CARGO_PROFILE_DEV_DEBUG}/${CARGO_PROFILE_TEST_DEBUG}" >&2
setsid -- "$@" &
child_pid=$!

stop_child() {
    kill -TERM -- "-$child_pid" 2>/dev/null || true
    sleep 2
    kill -KILL -- "-$child_pid" 2>/dev/null || true
}
trap 'stop_child; exit 130' INT
trap 'stop_child; exit 143' TERM

while true; do
    child_state=$(ps -o stat= -p "$child_pid" 2>/dev/null || true)
    if [[ -z $child_state || $child_state == Z* ]]; then
        break
    fi
    sleep 2
    child_state=$(ps -o stat= -p "$child_pid" 2>/dev/null || true)
    if [[ -z $child_state || $child_state == Z* ]]; then
        break
    fi
    if ! check_budget; then
        stop_child
        wait "$child_pid" 2>/dev/null || true
        exit 75
    fi
done

trap - INT TERM
if wait "$child_pid"; then
    exit 0
else
    exit $?
fi
