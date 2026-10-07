#!/usr/bin/env python3
# This Source Code Form is subject to the terms of the Mozilla Public
# License, v. 2.0. If a copy of the MPL was not distributed with this
# file, You can obtain one at https://mozilla.org/MPL/2.0/.

"""Replay the pinned diagnostic corpus and record every primary position gap."""

import argparse
import json
import re
import subprocess
from pathlib import Path


def legacy_flags(values):
    result = []
    iterator = iter(values)
    spelling = {
        "--experimentalDecorators": "--experimental-decorators",
        "--esModuleInterop": "--es-module-interop",
        "--jsxFactory": "--jsx-factory",
        "--jsxFragmentFactory": "--jsx-fragment-factory",
        "--jsxImportSource": "--jsx-import-source",
    }
    for flag in iterator:
        if flag in ("--strict", "--allowImportingTsExtensions", "--noEmit"):
            continue
        if flag == "--lib":
            next(iterator)
        elif flag in ("--target", "--module"):
            result.extend((flag, next(iterator).lower()))
        else:
            result.append(spelling.get(flag, flag))
    return result


def coordinates(source, start, end):
    prefix = source[:start].decode("utf-8")
    selected = source[start:end].decode("utf-8")
    breaks = list(re.finditer(r"\r\n|\r|\n", prefix))
    column_text = prefix[breaks[-1].end():] if breaks else prefix
    return {
        "line": len(breaks) + 1,
        "column": len(column_text.encode("utf-16-le")) // 2 + 1,
        "length": len(selected.encode("utf-16-le")) // 2,
    }


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--binary", required=True, type=Path)
    parser.add_argument("--output", required=True, type=Path)
    arguments = parser.parse_args()
    root = Path(__file__).resolve().parents[4]
    fixtures = root / "backend/bluets/tests/fixtures"
    reference = json.loads((fixtures / "diagnostics/reference.json").read_text())
    exceptions = {
        row["id"] for row in json.loads(
            (fixtures / "diagnostics/no-typescript-counterpart.json").read_text()
        )
    }
    mismatches = []
    compared = 0
    for case in reference["cases"]:
        if case["id"] in exceptions:
            continue
        command = [str(arguments.binary)]
        if "config" in case:
            config = fixtures / case["config"]
            source_root = config.parent
            command.extend(("--project", str(config), "--noEmit"))
        else:
            entry = fixtures / "typescript_oracle" / case["entry"]
            source_root = entry.parent
            command.extend(("check", str(entry), *legacy_flags(case["flags"])))
        result = subprocess.run(
            [*command, "--diagnostics-json"], capture_output=True, text=True, check=False
        )
        diagnostics = [json.loads(line) for line in result.stderr.splitlines()]
        expected = case["first"]
        if expected is None:
            if diagnostics or result.returncode:
                mismatches.append({"id": case["id"], "expected": None,
                                   "actual": diagnostics})
            continue
        compared += 1
        primary = diagnostics[0]
        counterpart = primary["typescript"]
        desired = {name: expected[name] for name in ("line", "column", "length")}
        actual = counterpart.get("position")
        if counterpart["code"] == expected["code"] and actual == desired:
            continue
        span = counterpart["span"]
        path = source_root / span["module"]
        derived = None
        if path.is_file():
            source = path.read_bytes()
            if 0 <= span["start"] <= span["end"] <= len(source):
                derived = coordinates(source, span["start"], span["end"])
        mismatches.append({
            "id": case["id"], "code": expected["code"], "expected": desired,
            "actual": actual, "originalByteSpan": span,
            "derivedOriginalPosition": derived,
        })
    record = {
        "version": reference["version"],
        "programs": len(reference["cases"]),
        "comparedPrimaries": compared,
        "explicitSubsetRefusals": len(exceptions),
        "mismatches": mismatches,
    }
    arguments.output.write_text(json.dumps(record, indent=2) + "\n")
    print(json.dumps({"programs": record["programs"], "comparedPrimaries": compared,
                      "mismatches": len(mismatches),
                      "differentOriginalSpans": sum(
                          row.get("derivedOriginalPosition") != row["expected"]
                          for row in mismatches
                      )}))


if __name__ == "__main__":
    main()
