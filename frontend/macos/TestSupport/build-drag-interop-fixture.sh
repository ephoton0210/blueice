#!/bin/bash
# This Source Code Form is subject to the terms of the Mozilla Public
# License, v. 2.0. If a copy of the MPL was not distributed with this
# file, You can obtain one at https://mozilla.org/MPL/2.0/.

set -euo pipefail
support_dir=$(cd "$(dirname "$0")" && pwd)
frontend_dir=$(cd "$support_dir/.." && pwd)
product="$frontend_dir/.build/Build/Products/Debug/DragInteropFixture.app"
mkdir -p "$product/Contents/MacOS" "$frontend_dir/.build/drag-fixture-module-cache"
cat > "$product/Contents/Info.plist" <<'PLIST'
<?xml version="1.0" encoding="UTF-8"?>
<!DOCTYPE plist PUBLIC "-//Apple//DTD PLIST 1.0//EN" "http://www.apple.com/DTDs/PropertyList-1.0.dtd">
<plist version="1.0"><dict>
<key>CFBundleIdentifier</key><string>cc.blueice.DragInteropFixture</string>
<key>CFBundleName</key><string>DragInteropFixture</string>
<key>CFBundleExecutable</key><string>DragInteropFixture</string>
<key>CFBundlePackageType</key><string>APPL</string>
<key>NSHighResolutionCapable</key><true/>
</dict></plist>
PLIST
xcrun swiftc -parse-as-library -module-cache-path "$frontend_dir/.build/drag-fixture-module-cache" \
    "$support_dir/DragInteropFixture.swift" -o "$product/Contents/MacOS/DragInteropFixture"
codesign --force --sign - "$product"
