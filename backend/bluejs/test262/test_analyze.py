# This Source Code Form is subject to the terms of the Mozilla Public
# License, v. 2.0. If a copy of the MPL was not distributed with this
# file, You can obtain one at https://mozilla.org/MPL/2.0/.

import hashlib
import json
from pathlib import Path
import tempfile
import unittest

from analyze import analyze, classify_item


class AnalysisTests(unittest.TestCase):
    def record(self, path="language/statements/try/basic.js", **fields):
        return {"path": path, "mode": "strict", "status": "fail",
                "actual": {"kind": "TypeError", "phase": "runtime"},
                "expected": None, "features": [], "flags": ["onlyStrict"], **fields}

    def test_target_and_observed_blocker_are_independent(self):
        item = classify_item(self.record("built-ins/Array/from/example.js",
                             actual={"kind": "unsupported", "reason": "module host"}),
                             {"features": ["Symbol.iterator"], "flags": ["module"]})
        self.assertEqual(item["target"], "library")
        self.assertEqual(item["blocker"], "module-host")
        self.assertIn("completion", item["dependencies"])
        self.assertIn("modules", item["dependencies"])
        self.assertEqual(classify_item(self.record(), {})["target"], "completion")
        item = classify_item(self.record("built-ins/Array/from/subclass.js"), {"features": ["class"]})
        self.assertEqual(item["target"], "library")
        self.assertIn("classes", item["dependencies"])

    def test_negative_pass_is_not_classified_as_an_error(self):
        item = classify_item(self.record(status="pass", expected={"phase": "runtime", "type": "TypeError"}), {})
        self.assertEqual(item["blocker"], "none")
        item = classify_item(self.record(actual={"kind": "unclassified_parse_error", "phase": "parse"}), {})
        self.assertEqual(item["blocker"], "unclassified-parse")
        self.assertEqual(item["confidence"], "observed-symptom")

    def test_annex_staging_and_intl_remain_in_the_inventory(self):
        for path, scope in [("annexB/language/statements/try/x.js", "annexB"),
                            ("staging/sm/x.js", "staging"), ("intl402/Locale/x.js", "intl402")]:
            item = classify_item(self.record(path), {})
            self.assertEqual(item["scope"], scope)
        self.assertEqual(classify_item(self.record("annexB/language/statements/try/x.js"), {})["target"], "completion")

    def fixture(self, root):
        corpus = root / "corpus"
        source = "/*---\nflags: [onlyStrict]\nesid: sec-try-statement\ndescription: exception propagation\n---*/\ntry {} finally {}"
        path = corpus / "test/language/statements/try/basic.js"
        path.parent.mkdir(parents=True)
        path.write_text(source)
        record = self.record(sha256=hashlib.sha256(source.encode()).hexdigest())
        results = root / "results.jsonl"
        results.write_text(json.dumps(record) + "\n")
        summary = {"complete_inventory": True, "filter": "", "test_files": 1,
                   "scheduled_modes": 1, "results": {"fail": 1},
                   "groups": {"language": {"fail": 1}}, "features": {},
                   "snapshot": {"revision": "test-fixture"}}
        summary_path = root / "summary.json"
        summary_path.write_text(json.dumps(summary))
        (corpus / ".bluejs-snapshot.json").write_text(json.dumps(summary["snapshot"]))
        return corpus, results, summary_path

    def test_reconciles_inventory_and_retains_every_mode(self):
        with tempfile.TemporaryDirectory() as temp:
            root = Path(temp)
            corpus, results, summary = self.fixture(root)
            report = analyze(results, summary, corpus, root / "analysis")
            self.assertEqual(report["scheduled_modes"], 1)
            self.assertEqual(report["targets"]["completion"]["fail"], 1)
            item = json.loads((root / "analysis/items.jsonl").read_text())
            self.assertEqual(item["esid"], "sec-try-statement")
            self.assertEqual(item["actual"], self.record()["actual"])
            self.assertTrue((root / "analysis/REPORT.md").is_file())

    def test_rejects_duplicate_missing_changed_or_mislabelled_results(self):
        for mutation in ("duplicate", "missing", "changed-source", "wrong-status", "wrong-mode", "partial-as-full", "snapshot"):
            with self.subTest(mutation=mutation), tempfile.TemporaryDirectory() as temp:
                root = Path(temp)
                corpus, results, summary = self.fixture(root)
                record = json.loads(results.read_text())
                if mutation == "duplicate":
                    results.write_text(results.read_text() * 2)
                elif mutation == "missing":
                    results.write_text("")
                elif mutation == "changed-source":
                    (corpus / "test" / record["path"]).write_text("changed")
                elif mutation == "partial-as-full":
                    extra = corpus / "test/extra.js"
                    extra.write_text("/*---\nflags: [onlyStrict]\n---*/\n0;")
                elif mutation == "snapshot":
                    (corpus / ".bluejs-snapshot.json").write_text('{"revision":"wrong"}')
                else:
                    record["status" if mutation == "wrong-status" else "mode"] = "pass" if mutation == "wrong-status" else "sloppy"
                    results.write_text(json.dumps(record) + "\n")
                with self.assertRaises(ValueError):
                    analyze(results, summary, corpus, root / "analysis")
                self.assertFalse((root / "analysis/REPORT.md").exists())

    def test_partial_inventory_excludes_only_unselected_tests_and_fixtures(self):
        with tempfile.TemporaryDirectory() as temp:
            root = Path(temp)
            corpus, results, summary = self.fixture(root)
            (corpus / "test/unselected.js").write_text("/*---\n---*/\n0;")
            (corpus / "test/ignored_FIXTURE.js").write_text("fixture has no metadata")
            info = json.loads(summary.read_text())
            info.update(complete_inventory=False, filter="language/statements/try/")
            summary.write_text(json.dumps(info))
            report = analyze(results, summary, corpus, root / "analysis")
            self.assertFalse(report["complete_inventory"])
            self.assertEqual(report["scheduled_modes"], 1)


if __name__ == "__main__":
    unittest.main()
