#!/bin/bash
# This Source Code Form is subject to the terms of the Mozilla Public
# License, v. 2.0. If a copy of the MPL was not distributed with this
# file, You can obtain one at https://mozilla.org/MPL/2.0/.
set -euo pipefail
frontend_dir=$(cd "$(dirname "$0")" && pwd)
panel_app="$TARGET_BUILD_DIR/$EXECUTABLE_FOLDER_PATH/BlueIcePanels.app"
mkdir -p "$panel_app/Contents/MacOS" "$DERIVED_FILE_DIR/panels"
cp "$frontend_dir/TrustedPanels/Info.plist" "$panel_app/Contents/Info.plist"
panel_outputs=()
for panel_arch in $ARCHS; do
    panel_binary="$DERIVED_FILE_DIR/panels/BlueIcePanels-$panel_arch"
    xcrun swiftc -swift-version 6 -parse-as-library -target "$panel_arch-apple-macosx14.0" \
        -module-cache-path "$DERIVED_FILE_DIR/panels/module-cache" \
        "$frontend_dir"/TrustedPanels/*.swift -o "$panel_binary"
    panel_outputs+=("$panel_binary")
done
xcrun lipo -create "${panel_outputs[@]}" -output "$panel_app/Contents/MacOS/BlueIcePanels"
/usr/bin/codesign --force --sign - "$panel_app"
