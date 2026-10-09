#!/bin/bash
# This Source Code Form is subject to the terms of the Mozilla Public
# License, v. 2.0. If a copy of the MPL was not distributed with this
# file, You can obtain one at https://mozilla.org/MPL/2.0/.

set -euo pipefail
frontend_dir=$(cd "$(dirname "$0")" && pwd)
repo_root=$(cd "$frontend_dir/../.." && pwd)
python3 "$frontend_dir/TestSupport/test-signed-runner.py"
"$frontend_dir/build.sh" Debug
core_target=${CARGO_TARGET_DIR:-$frontend_dir/.build/core-target}
case "$core_target" in /*) ;; *) core_target="$repo_root/$core_target" ;; esac
result="$frontend_dir/.build/results-$(date +%Y%m%d-%H%M%S).xcresult"
python3 "$frontend_dir/TestSupport/run-sftp-ui-tests.py" \
    python3 "$frontend_dir/TestSupport/run-update-ui-tests.py" "$frontend_dir/.build/Build/Products/Debug/BlueIce.app" \
    xcodebuild -project "$frontend_dir/BlueIce.xcodeproj" -scheme BlueIce \
    -configuration Debug -derivedDataPath "$frontend_dir/.build" \
    -destination "platform=macOS,arch=$(uname -m)" -parallel-testing-enabled NO \
    -resultBundlePath "$result" BLUEICE_BACKEND_DIR="$core_target/debug" test "$@"
attachments="${result%.xcresult}-attachments"
xcrun xcresulttool export attachments --path "$result" --output-path "$attachments"
echo "RESULTS=$result"
echo "SCREENSHOTS=$attachments"
