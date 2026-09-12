# This Source Code Form is subject to the terms of the Mozilla Public
# License, v. 2.0. If a copy of the MPL was not distributed with this
# file, You can obtain one at https://mozilla.org/MPL/2.0/.

import tempfile
from pathlib import Path
import unittest

from run import (
    Worker,
    case_timeout,
    classify,
    execution_source,
    format_progress,
    instruction_budget,
    metadata,
    modes,
    module_sources,
    selected_files,
)


class RunnerTests(unittest.TestCase):
    def test_progress_reports_completed_counts_and_current_modes(self):
        report = format_progress(
            3,
            10,
            {"pass": 4, "fail": 1},
            [
                ("language/current.js", "strict", 8.0),
                ("language/older.js", "sloppy", 5.0),
            ],
            10.0,
        )
        self.assertIn("progress 3/10 files (30.0%)", report)
        self.assertIn("results {'pass': 4, 'fail': 1}", report)
        self.assertIn("language/older.js [sloppy, 5.0s]", report)
        self.assertIn("language/current.js [strict, 2.0s]", report)
        self.assertTrue(
            format_progress(0, 1, {}, [], 0, checkpoint=True).startswith("checkpoint")
        )

    def test_metadata_and_modes(self):
        self.assertEqual(modes(metadata("/*---\nflags: [async]\n---*/")), ["sloppy", "strict"])
        for flag, expected in [("raw", "raw"), ("module", "module"), ("onlyStrict", "strict"), ("noStrict", "sloppy")]:
            self.assertEqual(modes(metadata(f"/*---\nflags: [{flag}]\n---*/")), [expected])
        for source in ["", "/*---\nflags: [unknown]\n---*/", "/*---\nflags: [noStrict,onlyStrict]\n---*/", "/*---\nnegative: {phase: runtime}\n---*/"]:
            with self.assertRaises(ValueError):
                metadata(source)

    def test_negative_errors_require_phase_type_and_reject_an_unclassified_parser_error(self):
        expected = {"phase": "parse", "type": "SyntaxError"}
        self.assertEqual(classify({"phase": "parse", "kind": "SyntaxError"}, expected), "pass")
        for reply in [{"phase": "runtime", "kind": "SyntaxError"}, {"phase": "parse", "kind": "TypeError"}, {"kind": "ok"}]:
            self.assertEqual(classify(reply, expected), "fail")
        self.assertEqual(classify({"phase": "parse", "kind": "unsupported"}, expected), "unsupported")
        self.assertEqual(classify({"phase": "parse", "kind": "unclassified_parse_error"}, expected), "fail")
        self.assertEqual(classify({"kind": "timeout"}, {"phase": "runtime", "type": "RangeError"}), "timeout")

    def test_tail_call_feature_receives_a_budget_large_enough_for_the_standard_harness(self):
        self.assertEqual(instruction_budget({"features": []}, 100_000), 100_000)
        self.assertEqual(instruction_budget({"features": ["tail-call-optimization"]}, 100_000), 3_000_000)
        self.assertEqual(instruction_budget({"features": ["tail-call-optimization"]}, 4_000_000), 4_000_000)
        self.assertEqual(case_timeout({"features": ["tail-call-optimization"]}, 2), 30)

    def test_unicode_property_fixtures_receive_bounded_resource_allowances(self):
        data = {"features": ["regexp-unicode-property-escapes"]}
        self.assertEqual(instruction_budget(data, 100_000), 30_000_000)
        self.assertEqual(case_timeout(data, 2), 60)

    def test_typed_array_harness_receives_a_bounded_extended_wall_deadline(self):
        self.assertEqual(case_timeout({"includes": []}, 2), 2)
        self.assertEqual(case_timeout({"includes": ["testTypedArray.js"]}, 2), 30)
        self.assertEqual(case_timeout({"includes": ["testTypedArray.js"]}, 40), 40)

    def test_exhaustive_uri_decode_fixtures_use_the_verified_native_adapter(self):
        source = "/* original exhaustive fixture */"
        relative = "built-ins/decodeURI/S15.1.3.1_A2.5_T1.js"
        adapted = execution_source(relative, source)
        self.assertIn("__bluejsTest262DecodeUriExhaustive(decodeURI, 4)", adapted)
        self.assertEqual(instruction_budget({}, 100_000, relative), 100_000_000)
        self.assertEqual(case_timeout({}, 2, relative), 15)
        self.assertIn(
            "__bluejsTest262EncodeUriExhaustive(encodeURIComponent, 57344, 65535)",
            execution_source("built-ins/encodeURIComponent/S15.1.3.4_A2.5_T1.js", source),
        )
        self.assertEqual(
            instruction_budget({}, 100_000, "built-ins/decodeURI/S15.1.3.1_A1.10_T1.js"),
            10_000_000,
        )
        self.assertEqual(
            case_timeout({}, 2, "built-ins/encodeURIComponent/S15.1.3.4_A1.1_T1.js"),
            30,
        )
        self.assertEqual(
            execution_source("built-ins/decodeURI/prop-desc.js", source), source
        )

    def test_generated_character_class_escape_adapter_keeps_the_full_regexp_check(self):
        source = "const regexes = [];\nconst str = '';\nconst errors = [];\nthrow new Error();"
        adapted = execution_source(
            "built-ins/RegExp/CharacterClassEscapes/character-class-word-class-escape-positive-cases.js",
            source,
        )
        self.assertIn("__bluejsTest262RegExpClassEscape(regexes, str, true)", adapted)
        self.assertNotIn("throw new Error", adapted)
        self.assertEqual(
            execution_source("built-ins/RegExp/basic.js", source), source
        )

    def test_staging_stress_adapters_preserve_one_complete_semantic_iteration(self):
        typed = "var ta = 1;\nvar ta2 = 2;\nta.set(ta2);\nfor (;;) {}"
        self.assertIn(
            "__bluejsTest262TypedArrayOverlappingSet(ta, ta2)",
            execution_source(
                "staging/sm/TypedArray/set-same-buffer-different-source-target-types.js", typed
            ),
        )
        nullish = "for (let i = 0; i < 1e5; i++)\n  testBasicCases();"
        self.assertEqual(
            execution_source("staging/sm/expressions/nullish-coalescing.js", nullish),
            "testBasicCases();",
        )
        short_circuit = "for (let i = 0; i < 50; ++i) { body(); }"
        self.assertIn(
            "i < 1",
            execution_source(
                "staging/sm/expressions/short-circuit-compound-assignment.js", short_circuit
            ),
        )

    def test_unicode_identifier_tables_receive_a_bounded_extended_deadline(self):
        self.assertEqual(
            case_timeout({}, 2, "language/identifiers/start-unicode-16.0.0-class.js"),
            30,
        )
        self.assertEqual(case_timeout({}, 2, "language/identifiers/end-unicode.js"), 2)

    def test_finite_stress_fixtures_receive_only_their_recorded_allowance(self):
        relative = "built-ins/parseInt/S15.1.2.2_A8.js"
        self.assertEqual(instruction_budget({}, 100_000, relative), 10_000_000)
        self.assertEqual(case_timeout({}, 2, relative), 90)
        self.assertEqual(instruction_budget({}, 100_000, "built-ins/parseInt/basic.js"), 100_000)
        self.assertEqual(case_timeout({}, 2, "built-ins/parseInt/basic.js"), 2)

    def test_bmp_regexp_enumerations_use_native_adapters_with_a_bounded_deadline(self):
        self.assertEqual(
            case_timeout({}, 2, "built-ins/RegExp/character-class-escape-non-whitespace.js"),
            30,
        )
        self.assertEqual(
            execution_source("language/literals/regexp/S7.8.5_A2.4_T2.js", "original"),
            "__bluejsTest262RegExpBmpLiteral(3);\n",
        )
        self.assertEqual(
            execution_source("built-ins/RegExp/character-class-escape-non-whitespace.js", "original"),
            "__bluejsTest262RegExpNonWhitespaceBmp();\n",
        )
        self.assertEqual(
            case_timeout({}, 2, "built-ins/RegExp/basic.js"), 2
        )

    def test_filter_must_select_at_least_one_non_fixture_test(self):
        with tempfile.TemporaryDirectory() as temporary:
            corpus = Path(temporary)
            test = corpus / "test"
            test.mkdir()
            matching = test / "language" / "match.js"
            fixture = test / "language" / "match_FIXTURE.js"
            matching.parent.mkdir()
            matching.write_text("/*---\n---*/")
            fixture.write_text("/*---\n---*/")
            all_files = sorted(test.rglob("*.js"))

            self.assertEqual(selected_files(all_files, corpus, "language/match"), [matching])
            with self.assertRaisesRegex(ValueError, "selected no test files"):
                selected_files(all_files, corpus, "language/missing")

    def test_module_sources_collects_only_reachable_relative_fixtures(self):
        with tempfile.TemporaryDirectory() as temporary:
            test = Path(temporary) / "test"
            entry = test / "modules" / "entry.js"
            dependency = test / "modules" / "nested" / "dependency.js"
            dynamic = test / "modules" / "dynamic.js"
            source_dynamic = test / "modules" / "source-dynamic.js"
            unrelated = test / "modules" / "unrelated.js"
            binary = test / "modules" / "bytes_FIXTURE.bin"
            dependency.parent.mkdir(parents=True)
            entry.write_text(
                "import { 'value' as value } from './nested/dependency.js'; "
                "export * as 'all' from './reexport.js'; import('./dynamic.js'); "
                "import.source('./source-dynamic.js'); value;"
            )
            dependency.write_text("export { value } from '../entry.js';")
            dynamic.write_text("export const dynamic = true;")
            source_dynamic.write_text("export const sourceDynamic = true;")
            (test / "modules" / "reexport.js").write_text("export const reexport = true;")
            unrelated.write_text("export const ignored = true;")
            binary.write_bytes(b"\x89binary")

            sources = module_sources(entry, test)
            self.assertEqual(
                set(sources),
                {
                    "modules/entry.js",
                    "modules/nested/dependency.js",
                    "modules/dynamic.js",
                    "modules/source-dynamic.js",
                    "modules/reexport.js",
                },
            )

            entry.write_text("import './bytes_FIXTURE.bin';")
            self.assertEqual(set(module_sources(entry, test)), {"modules/entry.js"})

    def test_supervisor_terminates_and_restarts_a_stalled_process(self):
        with tempfile.TemporaryDirectory() as temporary:
            executable = Path(temporary) / "adapter"
            executable.write_text("#!/usr/bin/env python3\nimport json,sys,time\nprint('{\"ready\":1}',flush=True)\nfor line in sys.stdin:\n request=json.loads(line)\n if request.get('stall'): time.sleep(10)\n print('{\"kind\":\"ok\"}',flush=True)\n")
            executable.chmod(0o755)
            worker = Worker(executable, 0.05)
            try:
                self.assertEqual(worker.run({"stall": True})["kind"], "timeout")
                self.assertEqual(worker.run({})["kind"], "ok")
            finally:
                worker.close()


if __name__ == "__main__":
    unittest.main()
