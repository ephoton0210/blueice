# This Source Code Form is subject to the terms of the Mozilla Public
# License, v. 2.0. If a copy of the MPL was not distributed with this
# file, You can obtain one at https://mozilla.org/MPL/2.0/.

import copy
import unittest
from pathlib import Path

import check_compatibility_status as status


class CurrentStatusTests(unittest.TestCase):
    @classmethod
    def setUpClass(cls):
        cls.root = Path(__file__).resolve().parents[4]
        cls.inputs = status.load_inputs(cls.root)
        cls.block = status.render_status(*cls.inputs)

    def test_every_current_document_matches_validated_measurement(self):
        for relative in status.DOCUMENTS:
            with self.subTest(document=relative):
                document = (self.root / relative).read_text(encoding="utf-8")
                status.check_document(document, self.block, relative)

    def test_changed_counts_version_parity_and_platform_scope_fail(self):
        for before, after in [
            ("12,511", "12,512"), ("12,010/12,028", "12,493/12,511"),
            ("99.8503%", "100%"), ("5.9.3", "5.9.2"),
            ("parity: OFF", "parity: ON"), ("22 open", "0 open"),
            ("82/82", "81/82"), ("darwin-arm64", "all platforms"),
        ]:
            with self.subTest(change=before), self.assertRaises(ValueError):
                status.check_document(self.block.replace(before, after), self.block, "changed.md")

    def test_missing_duplicate_or_reversed_markers_fail(self):
        for bad in [
            self.block.replace(status.BEGIN, ""),
            self.block.replace(status.END, ""), self.block + self.block,
            status.END + "\n" + status.BEGIN,
        ]:
            with self.subTest(document=bad[:60]), self.assertRaises(ValueError):
                status.check_document(bad, self.block, "bad.md")

    def test_historical_numbers_are_preserved_outside_current_block(self):
        history = "Historical J.6 baseline: 760 programs / 19 suites.\n"
        status.check_document(history + self.block + "\n" + history, self.block, "history.md")

    def test_foreign_catalog_or_forged_case_counts_fail(self):
        for change in ["source_revision", "source_graph_sha256", "typescript", "suite", "count"]:
            inputs = copy.deepcopy(self.inputs)
            if change == "suite":
                inputs[1]["suites"].pop()
            elif change == "count":
                inputs[0]["counts"]["agreements"] += 1
            else:
                inputs[1][change] = "foreign"
            with self.subTest(change=change), self.assertRaises(ValueError):
                status.render_status(*inputs)

    def test_unaccounted_feature_or_refusal_rows_fail(self):
        for before, after in [
            ("| G-T1 | open |", "| G-T1 | closed |"),
            ("| G-T1 | open |", "| G-T1 | unknown |"),
            ("213 refusal sites", "212 refusal sites"),
        ]:
            inputs = list(self.inputs)
            inputs[3] = inputs[3].replace(before, after)
            with self.subTest(change=before), self.assertRaises(ValueError):
                status.render_status(*inputs)


if __name__ == "__main__":
    unittest.main()
