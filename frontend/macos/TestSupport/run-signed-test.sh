#!/bin/sh
# This Source Code Form is subject to the terms of the Mozilla Public
# License, v. 2.0. If a copy of the MPL was not distributed with this
# file, You can obtain one at https://mozilla.org/MPL/2.0/.

# Cargo runner for locally signed macOS backend test executables, matching
# the native frontend's build/sign workflow without changing host settings.
set -eu
# Keep a valid signature intact: re-signing would rewrite an executable that
# another test may already be using and discard its existing signing identity.
if ! /usr/bin/codesign --verify --strict "$1" >/dev/null 2>&1; then
    /usr/bin/codesign --force --sign - "$1"
fi
exec "$@"
