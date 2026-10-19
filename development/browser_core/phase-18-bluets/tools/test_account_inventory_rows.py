# This Source Code Form is subject to the terms of the Mozilla Public
# License, v. 2.0. If a copy of the MPL was not distributed with this
# file, You can obtain one at https://mozilla.org/MPL/2.0/.

import copy
import json
import unittest
from pathlib import Path

import account_inventory_rows as rows


class AccountingTests(unittest.TestCase):
    @classmethod
    def setUpClass(cls):
        cls.root = Path(__file__).resolve().parents[4]
        cls.directory = cls.root / "development/browser_core/phase-18-bluets"
        cls.data = json.loads((cls.directory / "COMPATIBILITY_CASES.json").read_text(encoding="utf-8"))
        cls.measurement = json.loads((cls.directory / "COMPATIBILITY_MEASUREMENT.json").read_text(encoding="utf-8"))

    def test_recorded_cases_and_document_have_no_unlabelled_rows(self):
        counts = rows.validate_cases(self.data, self.measurement)
        self.assertEqual(sum(counts.values()), self.data["counts"]["listed"])
        self.assertEqual(counts["open"], self.data["counts"]["open"])
        self.assertEqual(counts["refused"], self.data["counts"]["refused"])
        document = (self.directory / "COMPATIBILITY_INVENTORY.md").read_text(encoding="utf-8")
        self.assertEqual(rows.render(document, self.data, self.root), document)

    def test_missing_forged_or_dangling_case_evidence_is_rejected(self):
        for field, value in [
            ("status", ""), ("status", "open"), ("scope", "whole_language"),
            ("source", "backend/../outside.rs"), ("source", "backend/file.rs:stream"),
            ("evidence", {}),
            ("evidence", {"assertion": "foreign", "measurement": "missing#/suites/0"}),
        ]:
            with self.subTest(field=field, value=value):
                data = copy.deepcopy(self.data)
                data["suites"][0]["cases"][0][field] = value
                with self.assertRaises(ValueError):
                    rows.validate_cases(data, self.measurement)
        data = copy.deepcopy(self.data)
        data["suites"][0]["evidence"] = "COMPATIBILITY_MEASUREMENT.json#/suites/999999"
        with self.assertRaises(ValueError):
            rows.validate_cases(data, self.measurement)

    def test_unknown_classifications_and_unaccounted_suites_are_rejected(self):
        for change in ["unknown", "missing", "duplicate", "foreign"]:
            data = copy.deepcopy(self.data)
            if change == "unknown":
                data["suites"][0]["cases"][0]["comparison"] = "unknown"
            elif change == "missing":
                data["suites"].pop()
            elif change == "duplicate":
                data["suites"][1] = data["suites"][0]
            else:
                data["source_graph_sha256"] = "foreign"
            with self.subTest(change=change), self.assertRaises(ValueError):
                rows.validate_cases(data, self.measurement)

    def test_gap_labels_are_regenerated_but_missing_or_duplicate_gap_rows_are_not_invented(self):
        document = (self.directory / "COMPATIBILITY_INVENTORY.md").read_text(encoding="utf-8")
        actual = rows.render(document, self.data, self.root)
        self.assertEqual(rows.render(actual, self.data, self.root), actual)
        line = next(line for line in actual.splitlines() if line.startswith("| G-T1 |"))
        for bad in [actual.replace(line + "\n", ""), actual + line + "\n"]:
            with self.assertRaises(ValueError):
                rows.render(bad, self.data, self.root)
        changed = actual.replace("| G-T1 | open |", "| G-T1 | closed |")
        self.assertNotEqual(changed, rows.render(changed, self.data, self.root))


if __name__ == "__main__":
    unittest.main()
