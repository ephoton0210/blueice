# This Source Code Form is subject to the terms of the Mozilla Public
# License, v. 2.0. If a copy of the MPL was not distributed with this
# file, You can obtain one at https://mozilla.org/MPL/2.0/.

"""Black-box checks for the operator's Phase 6 evidence verifier."""

import json
from pathlib import Path
import subprocess
import sys
import tempfile
import unittest


SCRIPT = Path(__file__).with_name("verify-human-evidence.py")
PNG = b"\x89PNG\r\n\x1a\n"
SOURCE = 0x123456789ABCDEF0
BADGE = "SRC:123456789ABCDEF0 TAB:1 GEN:3"


class HumanEvidenceVerifierTests(unittest.TestCase):
    def setUp(self):
        self.temp = tempfile.TemporaryDirectory(prefix="blueice-phase6-evidence-")
        self.addCleanup(self.temp.cleanup)
        self.root = Path(self.temp.name)
        self.first = self.root / "first.png"
        self.highlight = self.root / "highlight.png"
        self.human = self.root / "human-window.png"
        self.report = self.root / "report.json"
        self.transcript = self.root / "run.jsonl"
        self.first.write_bytes(PNG + b"first-page")
        self.highlight.write_bytes(PNG + b"highlighted-page")
        self.human.write_bytes(PNG + b"independent-window-with-chrome")
        self.write_transcript()

    def write_transcript(self, highlight_generation=3, png_generation=3):
        entries = [
            {"kind": "evidence_saved", "data": {
                "frame_source": SOURCE, "tab_id": 1, "generation": 1,
                "path": str(self.first),
            }},
            {"kind": "highlight_frame", "data": {
                "frame_source": SOURCE, "tab_id": 1,
                "generation": highlight_generation,
            }},
            {"kind": "evidence_saved", "data": {
                "frame_source": SOURCE, "tab_id": 1,
                "generation": png_generation, "path": str(self.highlight),
            }},
        ]
        self.transcript.write_text(
            "".join(json.dumps(entry) + "\n" for entry in entries), encoding="utf-8"
        )

    def run_check(self, badge=BADGE):
        return subprocess.run(
            [sys.executable, str(SCRIPT), str(self.transcript),
             str(self.human), badge, str(self.report)],
            capture_output=True, text=True, check=False,
        )

    def test_matching_transcription_records_distinct_human_and_mcp_hashes(self):
        result = self.run_check("SRC:123456789abcdef0 TAB:1 GEN:3")
        self.assertEqual(result.returncode, 0, result.stderr)
        report = json.loads(self.report.read_text(encoding="utf-8"))
        self.assertEqual(report["badge"], BADGE)
        self.assertEqual(report["frame_source"], SOURCE)
        self.assertNotEqual(report["human_png_sha256"], report["mcp_png_sha256"])

    def test_wrong_badge_or_stale_mcp_frame_is_rejected_without_report(self):
        self.assertNotEqual(self.run_check("SRC:123456789ABCDEF0 TAB:1 GEN:2").returncode, 0)
        self.assertFalse(self.report.exists())
        self.write_transcript(png_generation=4)
        self.assertNotEqual(self.run_check().returncode, 0)
        self.assertFalse(self.report.exists())

    def test_either_mcp_png_cannot_be_substituted_for_a_window_capture(self):
        for source in (self.first, self.highlight):
            self.human.write_bytes(source.read_bytes())
            self.assertNotEqual(self.run_check().returncode, 0)
            self.assertFalse(self.report.exists())

    def test_non_png_input_is_rejected(self):
        self.human.write_bytes(b"not a PNG")
        self.assertNotEqual(self.run_check().returncode, 0)
        self.assertFalse(self.report.exists())

    def test_old_transcript_without_frame_source_cannot_claim_new_badge_evidence(self):
        records = [json.loads(line) for line in self.transcript.read_text(encoding="utf-8").splitlines()]
        for record in records:
            record["data"].pop("frame_source")
        self.transcript.write_text(
            "".join(json.dumps(record) + "\n" for record in records), encoding="utf-8"
        )
        self.assertNotEqual(self.run_check().returncode, 0)
        self.assertFalse(self.report.exists())


if __name__ == "__main__":
    unittest.main()
