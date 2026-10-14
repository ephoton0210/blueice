#!/usr/bin/env python3
# This Source Code Form is subject to the terms of the Mozilla Public
# License, v. 2.0. If a copy of the MPL was not distributed with this
# file, You can obtain one at https://mozilla.org/MPL/2.0/.

"""Retain OS recordings using documented xcodebuild.xctestrun settings."""
import argparse
import os
from pathlib import Path
import plistlib
import subprocess
import tempfile

parser = argparse.ArgumentParser()
parser.add_argument("products", type=Path)
parser.add_argument("result", type=Path)
args, forwarded = parser.parse_known_args()
environment = {}
build_settings = []
options = []
for option in forwarded:
    if option.startswith("BLUEICE_") and "=" in option:
        key, value = option.split("=", 1)
        environment[key] = value
        build_settings.append(option)
    else:
        options.append(option)
frontend = Path(__file__).resolve().parent.parent
built = subprocess.run([
    "xcodebuild", "-project", str(frontend / "BlueIce.xcodeproj"), "-scheme", "BlueIce",
    "-configuration", "Debug", "-derivedDataPath", str(frontend / ".build"),
    "-destination", "platform=macOS,arch=" + os.uname().machine,
    "build-for-testing", *build_settings,
], check=False)
if built.returncode:
    raise SystemExit(built.returncode)
fixture = subprocess.run([
    "bash", str(frontend / "TestSupport/build-drag-interop-fixture.sh"),
], check=False)
if fixture.returncode:
    raise SystemExit(fixture.returncode)
sources = sorted(args.products.glob("BlueIce_macosx*.xctestrun"), key=lambda p: p.stat().st_mtime)
if not sources:
    raise SystemExit("No built BlueIce test configuration")
configuration = plistlib.loads(sources[-1].read_bytes())
for name in ("ProtocolTests", "BrowserUITests"):
    target = configuration[name]
    target["SystemAttachmentLifetime"] = "keepAlways"
    target["UserAttachmentLifetime"] = "keepAlways"
    target.setdefault("EnvironmentVariables", {}).update(environment)
    target.setdefault("TestHostEnvironmentVariables", {}).update(environment)
with tempfile.NamedTemporaryFile(dir=args.products, prefix=".blueice-kept-", suffix=".xctestrun", delete=False) as output:
    output.write(plistlib.dumps(configuration))
    temporary = Path(output.name)
try:
    result = subprocess.run([
        "xcodebuild", "-xctestrun", str(temporary),
        "-destination", "platform=macOS,arch=" + os.uname().machine,
        "-parallel-testing-enabled", "NO", "-resultBundlePath", str(args.result),
        "test-without-building", *options,
    ], check=False)
finally:
    temporary.unlink()
raise SystemExit(result.returncode)
