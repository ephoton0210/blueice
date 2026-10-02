#!/bin/bash
# This Source Code Form is subject to the terms of the Mozilla Public
# License, v. 2.0. If a copy of the MPL was not distributed with this
# file, You can obtain one at https://mozilla.org/MPL/2.0/.

set -euo pipefail
frontend_dir=$(cd "$(dirname "$0")" && pwd)
repo_root=$(cd "$frontend_dir/../.." && pwd)
configuration=${1:-Debug}
cargo_flags=(build -p blueice-engine --bin blueice-core --locked)
case "$configuration" in
    Debug) ;;
    Release) cargo_flags+=(--release) ;;
    *) echo 'Usage: build.sh [Debug|Release]' >&2; exit 2 ;;
esac
cd "$repo_root"
core_target=${CARGO_TARGET_DIR:-$frontend_dir/.build/core-target}
case "$core_target" in /*) ;; *) core_target="$repo_root/$core_target" ;; esac
rustup_bin=$(command -v rustup || true)
if [ -z "$rustup_bin" ]; then rustup_bin="${CARGO_HOME:-$HOME/.cargo}/bin/rustup"; fi
CARGO_TARGET_DIR="$core_target" "$rustup_bin" run 1.96.0 cargo "${cargo_flags[@]}"
core_profile=$(echo "$configuration" | tr '[:upper:]' '[:lower:]')
xcodebuild -project "$frontend_dir/BlueIce.xcodeproj" -scheme BlueIce \
    -configuration "$configuration" -derivedDataPath "$frontend_dir/.build" \
    -destination "platform=macOS,arch=$(uname -m)" \
    BLUEICE_CORE_EXE="$core_target/$core_profile/blueice-core" build
echo "APP=$frontend_dir/.build/Build/Products/$configuration/BlueIce.app"
