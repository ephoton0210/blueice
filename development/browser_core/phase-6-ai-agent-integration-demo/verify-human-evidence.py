#!/usr/bin/env python3
# This Source Code Form is subject to the terms of the Mozilla Public
# License, v. 2.0. If a copy of the MPL was not distributed with this
# file, You can obtain one at https://mozilla.org/MPL/2.0/.

"""Check Phase 6 frame identity and retain a human window-badge attestation.

This does not infer that a PNG actually shows a BlueIce window. A person must
inspect that image and type its visible badge; the independent screenshot and
the resulting report remain available for a separate visual audit.
"""

from __future__ import annotations

import hashlib
import json
from pathlib import Path
import re
import sys


BADGE = re.compile(r"SRC:([0-9a-fA-F]{16}) TAB:([0-9]+) GEN:([0-9]+)")
PNG_SIGNATURE = b"\x89PNG\r\n\x1a\n"


def frame(data: dict) -> tuple[int, int, int]:
    values = tuple(data[key] for key in ("frame_source", "tab_id", "generation"))
    if any(type(value) is not int or value < 0 for value in values):
        raise ValueError("frame identity must contain nonnegative integers")
    if any(value > 0xFFFFFFFFFFFFFFFF for value in values):
        raise ValueError("frame identity exceeds the browser's 64-bit fields")
    return values


def highlighted_evidence(transcript: Path) -> tuple[tuple[int, int, int], Path, list[Path]]:
    highlighted = None
    after_highlight = []
    all_mcp_pngs = []
    with transcript.open(encoding="utf-8") as records:
        for line in records:
            record = json.loads(line)
            kind = record.get("kind")
            if kind == "highlight_frame":
                if highlighted is not None:
                    raise ValueError("the transcript contains multiple highlight frames")
                highlighted = frame(record["data"])
            elif kind == "evidence_saved":
                data = record["data"]
                path = Path(data["path"])
                all_mcp_pngs.append(path)
                if len(all_mcp_pngs) > 16:
                    raise ValueError("the transcript contains too many MCP screenshots")
                if highlighted is not None:
                    after_highlight.append((frame(data), path))
    if highlighted is None or len(after_highlight) != 1:
        raise ValueError("the transcript needs one highlight and one later MCP screenshot")
    retained, path = after_highlight[0]
    if highlighted != retained:
        raise ValueError("the highlighted snapshot and retained MCP PNG have different frames")
    return highlighted, path, all_mcp_pngs


def badge_for(identity: tuple[int, int, int]) -> str:
    source, tab, generation = identity
    return f"SRC:{source:016X} TAB:{tab} GEN:{generation}"


def normalized_badge(observed: str) -> str:
    match = BADGE.fullmatch(observed.strip())
    if match is None:
        raise ValueError("type the entire visible badge as SRC:16HEX TAB:number GEN:number")
    source, tab, generation = match.groups()
    return badge_for((int(source, 16), int(tab), int(generation)))


def png_sha256(path: Path) -> str:
    with path.open("rb") as screenshot:
        if screenshot.read(8) != PNG_SIGNATURE:
            raise ValueError(f"{path} is not a PNG")
        digest = hashlib.sha256(PNG_SIGNATURE)
        for chunk in iter(lambda: screenshot.read(1024 * 1024), b""):
            digest.update(chunk)
    return digest.hexdigest()


def verify(transcript: Path, human_png: Path, observed: str, report: Path) -> dict:
    identity, mcp_png, all_mcp_pngs = highlighted_evidence(transcript)
    expected = badge_for(identity)
    if normalized_badge(observed) != expected:
        raise ValueError(f"window badge does not match the highlighted frame; expected {expected}")
    human_hash = png_sha256(human_png)
    mcp_hash = png_sha256(mcp_png)
    if any(human_hash == png_sha256(path) for path in all_mcp_pngs):
        raise ValueError("the supplied human-window PNG is byte-identical to an MCP page PNG")
    result = {
        "attestation": "operator-transcribed window badge; image content still requires visual audit",
        "badge": expected,
        "frame_source": identity[0],
        "tab_id": identity[1],
        "generation": identity[2],
        "human_png": str(human_png.resolve()),
        "human_png_sha256": human_hash,
        "mcp_png": str(mcp_png.resolve()),
        "mcp_png_sha256": mcp_hash,
        "transcript": str(transcript.resolve()),
    }
    with report.open("x", encoding="utf-8") as output:
        json.dump(result, output, indent=2, sort_keys=True)
        output.write("\n")
    return result


def main(argv: list[str]) -> int:
    if len(argv) != 5:
        print(
            "usage: verify-human-evidence.py <run.jsonl> <human-window.png> '<SRC:... TAB:... GEN:...>' <new-report.json>",
            file=sys.stderr,
        )
        return 2
    try:
        result = verify(Path(argv[1]), Path(argv[2]), argv[3], Path(argv[4]))
    except (OSError, ValueError, KeyError, TypeError, json.JSONDecodeError) as error:
        print(f"Phase 6 evidence check failed: {error}", file=sys.stderr)
        return 1
    print(f"Recorded matching badge {result['badge']}; visually audit {result['human_png']}")
    return 0


if __name__ == "__main__":
    raise SystemExit(main(sys.argv))
