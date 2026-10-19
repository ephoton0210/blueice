# This Source Code Form is subject to the terms of the Mozilla Public
# License, v. 2.0. If a copy of the MPL was not distributed with this
# file, You can obtain one at https://mozilla.org/MPL/2.0/.

import copy
from decimal import Decimal
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
        counts = self.inputs[0]["counts"]
        ratio = Decimal(counts["agreements"]) * 100 / Decimal(counts["comparison_cases"])
        for before, after in [
            (f"{counts['listed']:,}", f"{counts['listed'] + 1:,}"),
            (f"{counts['agreements']:,}/{counts['comparison_cases']:,}", "0/1"),
            (f"{ratio:.4f}%", "100%"), ("5.9.3", "5.9.2"),
            ("parity: OFF", "parity: ON"), ("22 open", "0 open"),
            ("82/82", "81/82"), ("darwin-arm64", "all platforms"),
        ]:
            with self.subTest(change=before), self.assertRaises(ValueError):
                status.check_document(self.block.replace(before, after), self.block, "changed.md")

    def test_immutable_backend_tree_binding_keeps_the_same_count_checks(self):
        inputs = copy.deepcopy(self.inputs)
        revision = "backend-tree:" + "a" * 40
        for index in (0, 1, 2):
            inputs[index]["source_revision"] = revision
        # Recompute the measurement byte binding rather than forging its hash.
        import hashlib
        import json
        inputs = list(inputs)
        inputs[5] = (json.dumps(inputs[2]) + "\n").encode()
        inputs[0]["measurement_sha256"] = hashlib.sha256(inputs[5]).hexdigest()
        block = status.render_status(*inputs)
        self.assertIn(revision, block)
        for bad in ("backend-tree:HEAD", "backend-tree:" + "a" * 39, "tree:" + "a" * 40):
            for index in (0, 1, 2):
                inputs[index]["source_revision"] = bad
            inputs[5] = (json.dumps(inputs[2]) + "\n").encode()
            inputs[0]["measurement_sha256"] = hashlib.sha256(inputs[5]).hexdigest()
            with self.subTest(binding=bad), self.assertRaises(ValueError):
                status.render_status(*inputs)

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
        before = history + self.block.replace("parity: OFF", "parity: ON") + "\n" + history
        self.assertEqual(status.replace_status(before, self.block, "history.md"),
                         history + self.block + "\n" + history)
        for bad in (history, self.block + self.block, status.END + "\n" + status.BEGIN):
            with self.subTest(markers=bad[:50]), self.assertRaises(ValueError):
                status.replace_status(bad, self.block, "bad.md")

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
