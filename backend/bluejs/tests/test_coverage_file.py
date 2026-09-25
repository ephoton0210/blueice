# This Source Code Form is subject to the terms of the Mozilla Public
# License, v. 2.0. If a copy of the MPL was not distributed with this
# file, You can obtain one at https://mozilla.org/MPL/2.0/.

"""Contract tests for fresh, per-file BlueJS coverage reporting."""

import importlib.util
import json
import tempfile
import unittest
from pathlib import Path
from unittest.mock import patch


SCRIPT = Path(__file__).resolve().parents[1] / "coverage_file.py"
SPEC = importlib.util.spec_from_file_location("bluejs_coverage_file", SCRIPT)
coverage_file = importlib.util.module_from_spec(SPEC)
SPEC.loader.exec_module(coverage_file)


class CoverageFileTests(unittest.TestCase):
    def setUp(self):
        self.temp = tempfile.TemporaryDirectory()
        self.addCleanup(self.temp.cleanup)
        self.repo = Path(self.temp.name).resolve()
        self.source = self.repo / "backend/bluejs/src"
        self.source.mkdir(parents=True)
        for name in ("ast.rs", "module.rs", "tests.rs"):
            (self.source / name).write_text("// fixture\n")
        (self.source / "ast.rs").write_text("// fixture\n// second\n")
        nested = self.source / "vm/temporal/dates.rs"
        nested.parent.mkdir(parents=True)
        nested.write_text("// fixture\n")
        self.patch(coverage_file, "REPO_ROOT", self.repo)
        self.patch(coverage_file, "SOURCE_ROOT", self.source)
        self.patch(
            coverage_file,
            "NO_COUNTER_REASONS",
            {
                "module.rs": "Declarations/re-exports only; no executable code",
                "tests.rs": "Test source; not a coverage target",
                "vm/temporal/dates.rs": "Declarations/re-exports only; no executable code",
            },
        )

    def patch(self, module, name, value):
        replacement = patch.object(module, name, value)
        replacement.start()
        self.addCleanup(replacement.stop)

    def export(self, *, covered_lines=2, covered_regions=3):
        summary = {
            "lines": {"count": 2, "covered": covered_lines},
            "functions": {"count": 1, "covered": 1},
            "regions": {"count": 3, "covered": covered_regions},
        }
        return {
            "data": [
                {
                    "files": [
                        {"filename": str(self.source / "ast.rs"), "summary": summary}
                    ],
                    "functions": [
                        {
                            "filenames": [str(self.source / "ast.rs")],
                            "count": 1,
                            "regions": [
                                [1, 1, 1, 2, 1, 0, 0, 0],
                                [2, 1, 2, 2, 1, 0, 0, 0],
                                [2, 3, 2, 4, 1, 0, 0, 0],
                            ],
                        }
                    ],
                    "totals": summary,
                }
            ]
        }

    def test_source_path_accepts_three_forms_and_rejects_foreign_files(self):
        ast = self.source / "ast.rs"
        self.assertEqual(coverage_file.source_path("ast.rs"), ast)
        self.assertEqual(
            coverage_file.source_path("backend/bluejs/src/ast.rs"), ast
        )
        self.assertEqual(coverage_file.source_path(str(ast)), ast)
        self.assertEqual(
            coverage_file.source_path("vm/temporal/dates.rs"),
            self.source / "vm/temporal/dates.rs",
        )
        with self.assertRaises(ValueError):
            coverage_file.source_path(str(self.repo / "elsewhere.rs"))

    def test_export_reconciles_counts_and_rejects_unreviewed_missing_files(self):
        files, totals = coverage_file.checked_export(self.export())
        self.assertEqual(len(files), 1)
        self.assertEqual(totals["lines"]["covered"], 2)
        (self.source / "new_code.rs").write_text("fn new_code() {}\n")
        with self.assertRaisesRegex(ValueError, "unreviewed source files"):
            coverage_file.checked_export(self.export())
        (self.source / "new_code.rs").unlink()
        broken = self.export()
        broken["data"][0]["totals"] = {
            **broken["data"][0]["totals"],
            "lines": {"count": 3, "covered": 2},
        }
        with self.assertRaisesRegex(ValueError, "totals do not reconcile"):
            coverage_file.checked_export(broken)
        no_function_counters = self.export()
        no_function_counters["data"][0]["files"][0]["summary"]["functions"] = {
            "count": 0,
            "covered": 0,
        }
        with self.assertRaisesRegex(ValueError, "invalid functions counters"):
            coverage_file.checked_export(no_function_counters)

    def test_completion_requires_all_three_metrics_and_explains_no_counters(self):
        files, _ = coverage_file.checked_export(self.export())
        ast = self.source / "ast.rs"
        self.assertTrue(coverage_file.is_complete(files[ast]))
        self.assertIn("| ☑ |", coverage_file.markdown_row(ast, files[ast]))
        self.assertIn(
            "| - | - | - | - | Declarations/re-exports only",
            coverage_file.markdown_row(self.source / "module.rs", None),
        )
        self.assertFalse(coverage_file.is_complete(None))
        incomplete, _ = coverage_file.checked_export(self.export(covered_regions=2))
        self.assertFalse(coverage_file.is_complete(incomplete[ast]))
        self.assertIn("| ☐ |", coverage_file.markdown_row(ast, incomplete[ast]))
        zero_coverage, _ = coverage_file.checked_export(self.export(covered_lines=0))
        self.assertIn("0 / 2 (0.00%)", coverage_file.markdown_row(ast, zero_coverage[ast]))
        almost_complete = {
            **files[ast],
            "lines": {"count": 1_000_000, "covered": 999_999},
        }
        self.assertIn("(100.00%)", coverage_file.metric_cell(almost_complete["lines"]))
        self.assertFalse(coverage_file.is_complete(almost_complete))

    def test_fresh_run_cleans_profiles_and_executes_unfiltered_crate_suite(self):
        calls = []

        def fake_run(argv, *, cwd, check):
            self.assertEqual(cwd, self.repo)
            self.assertTrue(check)
            calls.append(argv)
            if "--output-path" in argv:
                output = Path(argv[argv.index("--output-path") + 1])
                if "--json" in argv:
                    output.write_text(json.dumps(self.export()))
                else:
                    output.write_text(
                        f"{self.source / 'ast.rs'}:\n"
                        "    1|      1|fn f() {}\n"
                        "    2|      1|fn g() {}\n"
                    )

        with patch.object(coverage_file.subprocess, "run", side_effect=fake_run):
            files, _ = coverage_file.run_coverage()
        self.assertEqual(len(files), 1)
        self.assertEqual(calls[0], ["cargo", "llvm-cov", "clean", "--workspace"])
        self.assertEqual(calls[1][:4], ["cargo", "llvm-cov", "-p", "blueice-bluejs"])
        self.assertNotIn("--test", calls[1])
        self.assertNotIn("--ignore-filename-regex", calls[1])
        self.assertEqual(calls[2][:5], ["cargo", "llvm-cov", "report", "-p", "blueice-bluejs"])

    def test_source_union_counts_duplicate_compilations_once(self):
        payload = self.export()
        summary = payload["data"][0]["files"][0]["summary"]
        summary["lines"] = {"count": 3, "covered": 2}
        summary["regions"] = {"count": 3, "covered": 2}
        payload["data"][0]["totals"] = summary
        first = payload["data"][0]["functions"][0]
        first["regions"][2][4] = 0
        second = json.loads(json.dumps(first))
        second["regions"][2][4] = 1
        payload["data"][0]["functions"].append(second)
        show = (
            f"{self.source / 'ast.rs'}:\n"
            "    1|      1|fn f() {}\n"
            "    2|      1|fn g() {}\n"
        )
        files, totals = coverage_file.source_union_export(payload, show)
        ast = self.source / "ast.rs"
        for metric, expected in {
            "lines": 2,
            "functions": 1,
            "regions": 3,
        }.items():
            self.assertEqual(files[ast][metric], {"count": expected, "covered": expected})
            self.assertEqual(totals[metric], files[ast][metric])
        row = coverage_file.markdown_row(ast, files[ast])
        self.assertIn("| ☑ |", row)
        self.assertIn("raw LLVM summary: lines 2/3", row)
        with self.assertRaisesRegex(ValueError, "incomplete source-line coverage view"):
            coverage_file.source_union_export(payload, show.splitlines()[0] + "\n")

    def test_report_update_preserves_other_sections_and_adds_completion_column(self):
        report = self.repo / "macos.md"
        report.write_text(
            "before\n## Later BlueJS per-file coverage (old)\nold\n"
            "## Historical differences from the other platforms (old)\nafter\n"
        )
        self.patch(coverage_file, "MACOS_REPORT", report)
        files, totals = coverage_file.checked_export(self.export())
        with patch.object(coverage_file.platform, "system", return_value="Darwin"), patch.object(
            coverage_file, "provenance", return_value="test host"
        ):
            coverage_file.update_macos_report(files, totals)
        text = report.read_text()
        self.assertTrue(text.startswith("before\n"))
        self.assertTrue(text.endswith("after\n"))
        self.assertIn("| Complete | Note |", text)
        self.assertIn("| ☑ |", text)
        self.assertIn("| - | - | - | - | Test source; not a coverage target |", text)

        report.write_text(
            "before\n## Later BlueJS per-file coverage (old)\nold\n"
            "## Differences from the other platforms\nafter\n"
        )
        with patch.object(coverage_file.platform, "system", return_value="Darwin"), patch.object(
            coverage_file, "provenance", return_value="test host"
        ):
            coverage_file.update_macos_report(files, totals)
        self.assertIn("## Differences from the other platforms\nafter\n", report.read_text())

    def test_one_run_can_report_a_file_and_update_the_full_table(self):
        files, totals = coverage_file.checked_export(self.export())
        self.patch(coverage_file, "MACOS_REPORT", self.repo / "macos.md")
        with (
            patch.object(coverage_file, "run_coverage", return_value=(files, totals)),
            patch.object(coverage_file, "update_macos_report") as update,
            patch("builtins.print") as printed,
        ):
            self.assertEqual(
                coverage_file.main(["ast.rs", "--update-macos-report"]), 0
            )
        update.assert_called_once_with(files, totals)
        self.assertTrue(any("Complete: yes" in str(call) for call in printed.call_args_list))

    def test_file_only_measurement_does_not_update_either_report(self):
        files, totals = coverage_file.checked_export(self.export())
        with (
            patch.object(coverage_file, "run_coverage", return_value=(files, totals)) as run,
            patch.object(coverage_file, "update_macos_report") as macos,
            patch.object(coverage_file, "update_linux_report") as linux,
            patch("builtins.print") as printed,
        ):
            self.assertEqual(coverage_file.main(["ast.rs"]), 0)
        run.assert_called_once_with()
        macos.assert_not_called()
        linux.assert_not_called()
        output = [str(call) for call in printed.call_args_list]
        self.assertTrue(any("ast.rs" in line for line in output))
        self.assertTrue(any("Complete: yes" in line for line in output))

    def test_linux_report_adds_the_same_table_before_platform_differences(self):
        report = self.repo / "linux.md"
        report.write_text("before\n## Differences from the other platforms\nafter\n")
        self.patch(coverage_file, "LINUX_REPORT", report)
        files, totals = coverage_file.checked_export(self.export())
        with patch.object(coverage_file.platform, "system", return_value="Linux"), patch.object(
            coverage_file, "provenance", return_value="Linux test host"
        ):
            coverage_file.update_linux_report(files, totals)
        text = report.read_text()
        self.assertTrue(text.startswith("before\n## Later BlueJS per-file coverage"))
        self.assertIn("--update-linux-report", text)
        self.assertIn("| Complete | Note |", text)
        self.assertTrue(text.endswith("## Differences from the other platforms\nafter\n"))


if __name__ == "__main__":
    unittest.main()
