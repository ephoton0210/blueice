# This Source Code Form is subject to the terms of the Mozilla Public
# License, v. 2.0. If a copy of the MPL was not distributed with this
# file, You can obtain one at https://mozilla.org/MPL/2.0/.

import tempfile
from pathlib import Path
import unittest
from unittest import mock

import run
from run import (
    ITERATOR_ZIP_BASIC_MATRIX_FIXTURES,
    ITERATOR_ZIP_BASIC_MATRIX_INSTRUCTION_BUDGET,
    ITERATOR_ZIP_BASIC_MATRIX_TIMEOUT,
    TEMPORAL_CALENDAR_TABLE_FIXTURES,
    TEMPORAL_CALENDAR_TABLE_INSTRUCTION_BUDGET,
    TEMPORAL_TIME_ZONE_LINK_TABLE_FIXTURES,
    TEMPORAL_TIME_ZONE_LINK_TABLE_INSTRUCTION_BUDGET,
    ZONED_DATE_TIME_SAME_EPOCH_MATRIX_FIXTURES,
    ZONED_DATE_TIME_SAME_EPOCH_MATRIX_INSTRUCTION_BUDGET,
    Worker,
    case_timeout,
    classify,
    default_jobs,
    execution_source,
    format_progress,
    instruction_budget,
    metadata,
    modes,
    module_sources,
    selected_files,
)


class RunnerTests(unittest.TestCase):
    def test_default_jobs_never_oversubscribes_the_host_or_exceeds_the_ceiling(self):
        self.assertEqual(default_jobs(0), 1)
        self.assertEqual(default_jobs(1), 1)
        self.assertEqual(default_jobs(6), 6)
        self.assertEqual(default_jobs(8), 8)
        self.assertEqual(default_jobs(32), 8)

    def test_regex_worker_binary_preserves_adapter_suffix(self):
        self.assertEqual(
            run.regex_worker_binary(Path("/tmp/bluejs-test262")),
            Path("/tmp/bluejs-regexp-worker"),
        )
        self.assertEqual(
            run.regex_worker_binary(Path("C:/test/bluejs-test262.exe")),
            Path("C:/test/bluejs-regexp-worker.exe"),
        )

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

    def test_stable_array_sort_and_agent_cases_have_scoped_host_allowances(self):
        self.assertEqual(
            instruction_budget({"features": ["stable-array-sort"]}, 100_000),
            10_000_000,
        )
        self.assertEqual(case_timeout({"features": ["stable-array-sort"]}, 2), 30)
        source = "$262.agent.start('');"
        self.assertEqual(instruction_budget({}, 100_000, source=source), 50_000_000)
        self.assertEqual(case_timeout({}, 2, source=source), 120)

    def test_finite_locale_module_and_sparse_array_stress_fixtures_are_bounded(self):
        for relative in (
            "intl402/Intl/getCanonicalLocales/canonicalized-tags.js",
            "intl402/Intl/getCanonicalLocales/complex-region-subtag-replacement.js",
            "intl402/Intl/getCanonicalLocales/transformed-ext-valid.js",
            "intl402/Intl/getCanonicalLocales/unicode-ext-canonicalize-yes-to-true.js",
            "intl402/language-tags-canonicalized.js",
            "language/module-code/top-level-await/fulfillment-order.js",
            "language/module-code/top-level-await/rejection-order.js",
            "language/module-code/top-level-await/unobservable-global-async-evaluation-count-reset.js",
            "staging/sm/Array/sort_holes.js",
            "staging/sm/Reflect/propertyKeys.js",
            "staging/sm/RegExp/unicode-ignoreCase.js",
            "staging/sm/TypedArray/filter-species.js",
            "staging/sm/TypedArray/map-species.js",
            "staging/sm/TypedArray/sort_snans.js",
            "staging/sm/generators/delegating-yield-9.js",
            "staging/sm/object/entries.js",
        ):
            self.assertEqual(instruction_budget({}, 100_000, relative), 10_000_000)
            self.assertEqual(case_timeout({}, 2, relative), 90)

    def test_typed_array_harness_receives_a_bounded_extended_wall_deadline(self):
        self.assertEqual(case_timeout({"includes": []}, 2), 2)
        self.assertEqual(case_timeout({"includes": ["testTypedArray.js"]}, 2), 60)
        self.assertEqual(case_timeout({"includes": ["testTypedArray.js"]}, 40), 60)

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

    def test_number_format_precision_matrix_uses_its_normal_timeout_after_native_adaptation(self):
        relative = "intl402/NumberFormat/test-option-roundingPriority-mixed-options.js"
        source = "function testPrecision() {\n  testNumberFormat(\n      locales, numberingSystems);\n}"
        adapted = execution_source(relative, source)
        self.assertIn(
            "__bluejsTest262NumberFormatPrecisionMatrix(\n      locales,",
            adapted,
        )
        self.assertEqual(instruction_budget({}, 100_000, relative), 100_000)
        # This fixture includes testIntl.js, but its precise adapter retains
        # the standard two-second policy instead of inheriting that helper's
        # generic matrix allowance.
        self.assertEqual(case_timeout({"includes": ["testIntl.js"]}, 2, relative), 2)
        with self.assertRaisesRegex(ValueError, "missing NumberFormat precision-matrix call"):
            execution_source(relative, "testNumberFormat(locales, numberingSystems)")

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
        self.assertEqual(
            case_timeout(
                {},
                2,
                "built-ins/Function/prototype/toString/built-in-function-object.js",
            ),
            180,
        )
        for relative in (
            "built-ins/RegExp/match-indices/indices-array-non-unicode-match.js",
            "built-ins/RegExp/match-indices/indices-array-unicode-match.js",
        ):
            self.assertEqual(case_timeout({}, 2, relative), 30)
        self.assertEqual(instruction_budget({}, 100_000, "built-ins/parseInt/basic.js"), 100_000)
        self.assertEqual(case_timeout({}, 2, "built-ins/parseInt/basic.js"), 2)

    def test_temporal_calendar_matrices_receive_an_exact_bounded_envelope(self):
        for relative in (
            "intl402/DateTimeFormat/prototype/formatToParts/compare-to-temporal.js",
            "intl402/DateTimeFormat/prototype/formatToParts/compare-to-temporal-lunisolar.js",
        ):
            self.assertEqual(instruction_budget({}, 100_000, relative), 10_000_000)
            self.assertEqual(case_timeout({}, 2, relative), 360)
        self.assertEqual(
            instruction_budget(
                {},
                100_000,
                "intl402/DateTimeFormat/prototype/formatToParts/basic.js",
            ),
            100_000,
        )
        self.assertEqual(
            case_timeout(
                {}, 2, "intl402/DateTimeFormat/prototype/formatToParts/basic.js"
            ),
            2,
        )

    def test_finite_temporal_fixtures_get_a_named_bounded_allowance_and_nothing_else(self):
        # Test262 defines no instruction budget: it is this host's own resource
        # policy. Each finite fixture keeps its unmodified source and gets an
        # explicit, bounded allowance; neighbours keep the ordinary default.
        self.assertEqual(
            ZONED_DATE_TIME_SAME_EPOCH_MATRIX_FIXTURES,
            frozenset(
                {
                    "built-ins/Temporal/ZonedDateTime/prototype/since/same-epoch-nanoseconds.js",
                    "built-ins/Temporal/ZonedDateTime/prototype/until/same-epoch-nanoseconds.js",
                }
            ),
        )
        for relative in ZONED_DATE_TIME_SAME_EPOCH_MATRIX_FIXTURES:
            self.assertEqual(
                instruction_budget({}, 100_000, relative),
                ZONED_DATE_TIME_SAME_EPOCH_MATRIX_INSTRUCTION_BUDGET,
            )
        self.assertEqual(
            TEMPORAL_CALENDAR_TABLE_FIXTURES,
            frozenset(
                {
                    "intl402/Temporal/PlainDate/from/hebrew-keviah.js",
                    "intl402/Temporal/PlainDate/from/persian-new-year-dates.js",
                    "intl402/Temporal/PlainDateTime/from/roundtrip-from-property-bag.js",
                    "intl402/Temporal/ZonedDateTime/from/roundtrip-from-property-bag.js",
                    "intl402/Temporal/PlainDate/prototype/dayOfYear/non-iso-calendar-basic.js",
                    "intl402/Temporal/PlainDateTime/prototype/dayOfYear/non-iso-calendar-basic.js",
                    "intl402/Temporal/ZonedDateTime/prototype/dayOfYear/non-iso-calendar-basic.js",
                }
            ),
        )
        for relative in TEMPORAL_CALENDAR_TABLE_FIXTURES:
            self.assertEqual(
                instruction_budget({}, 100_000, relative),
                TEMPORAL_CALENDAR_TABLE_INSTRUCTION_BUDGET,
            )
            # A larger explicit --instruction-budget is never lowered.
            self.assertEqual(
                instruction_budget({}, 50_000_000, relative), 50_000_000
            )
        self.assertEqual(
            TEMPORAL_TIME_ZONE_LINK_TABLE_FIXTURES,
            frozenset({"intl402/Temporal/ZonedDateTime/links.js"}),
        )
        for relative in TEMPORAL_TIME_ZONE_LINK_TABLE_FIXTURES:
            self.assertEqual(
                instruction_budget({}, 100_000, relative),
                TEMPORAL_TIME_ZONE_LINK_TABLE_INSTRUCTION_BUDGET,
            )
            self.assertEqual(
                instruction_budget({}, 50_000_000, relative), 50_000_000
            )
        self.assertEqual(
            instruction_budget(
                {}, 100_000, "intl402/Temporal/PlainDate/from/basic.js"
            ),
            100_000,
        )

    def test_iterator_zip_basic_matrices_receive_an_exact_bounded_envelope(self):
        self.assertEqual(
            ITERATOR_ZIP_BASIC_MATRIX_FIXTURES,
            frozenset(
                {
                    "built-ins/Iterator/zip/basic-shortest.js",
                    "built-ins/Iterator/zip/basic-longest.js",
                    "built-ins/Iterator/zip/basic-strict.js",
                    "built-ins/Iterator/zipKeyed/basic-shortest.js",
                    "built-ins/Iterator/zipKeyed/basic-longest.js",
                    "built-ins/Iterator/zipKeyed/basic-strict.js",
                }
            ),
        )
        for relative in ITERATOR_ZIP_BASIC_MATRIX_FIXTURES:
            self.assertEqual(
                instruction_budget({}, 100_000, relative),
                ITERATOR_ZIP_BASIC_MATRIX_INSTRUCTION_BUDGET,
            )
            self.assertEqual(
                case_timeout({}, 2, relative),
                ITERATOR_ZIP_BASIC_MATRIX_TIMEOUT,
            )
        self.assertEqual(
            instruction_budget({}, 100_000, "built-ins/Iterator/zip/options.js"),
            100_000,
        )
        self.assertEqual(
            case_timeout({}, 2, "built-ins/Iterator/zip/options.js"),
            2,
        )

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
            retained = test / "language" / "retained.js"
            fixture = test / "language" / "match_FIXTURE.js"
            matching.parent.mkdir()
            matching.write_text("/*---\n---*/")
            retained.write_text("/*---\n---*/")
            fixture.write_text("/*---\n---*/")
            all_files = sorted(test.rglob("*.js"))

            self.assertEqual(selected_files(all_files, corpus, "language/match"), [matching])
            self.assertEqual(
                selected_files(all_files, corpus, "language/", "match.js"), [retained]
            )
            with self.assertRaisesRegex(ValueError, "selected no test files"):
                selected_files(all_files, corpus, "language/missing")
            with self.assertRaisesRegex(ValueError, "selected no test files"):
                selected_files(all_files, corpus, "language/", "match.js,retained.js")

    def test_unicode_extension_locale_matrix_has_a_scoped_envelope(self):
        self.assertEqual(
            case_timeout(
                {},
                2,
                "intl402/supportedLocalesOf-unicode-extensions-ignored.js",
            ),
            180,
        )
        self.assertEqual(
            case_timeout({}, 2, "intl402/supportedLocalesOf/basic.js"),
            2,
        )

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

            # `dynamic.js` is reached only through a literal `import(...)`
            # call -- a dynamic edge -- so it lands in `dynamic_sources`, not
            # the eagerly compiled `sources`, even without
            # `include_dynamic_string_roots`. `source-dynamic.js`, reached
            # only through `import.source(...)`, stays in `sources`: that
            # path has no lazy-compile counterpart to
            # `ensure_dynamic_module_compiled` (see `module_sources`'s own
            # docstring), so it must still be eagerly compiled.
            sources, dynamic_sources, json_sources = module_sources(entry, test)
            self.assertEqual(
                set(sources),
                {
                    "modules/entry.js",
                    "modules/nested/dependency.js",
                    "modules/reexport.js",
                    "modules/source-dynamic.js",
                },
            )
            self.assertEqual(set(dynamic_sources), {"modules/dynamic.js"})
            self.assertEqual(json_sources, {})

            entry.write_text("import './bytes_FIXTURE.bin';")
            sources, dynamic_sources, json_sources = module_sources(entry, test)
            self.assertEqual(set(sources), {"modules/entry.js"})
            self.assertEqual(dynamic_sources, {})
            self.assertEqual(json_sources, {})

    def test_module_sources_classifies_a_module_reached_both_ways_as_static(self):
        with tempfile.TemporaryDirectory() as temporary:
            test = Path(temporary) / "test"
            entry = test / "modules" / "entry.js"
            entry.parent.mkdir(parents=True)
            entry.write_text(
                "import './both.js'; import('./both.js');"
            )
            (test / "modules" / "both.js").write_text("export const value = true;")

            sources, dynamic_sources, json_sources = module_sources(entry, test)
            self.assertEqual(set(sources), {"modules/entry.js", "modules/both.js"})
            self.assertEqual(dynamic_sources, {})
            self.assertEqual(json_sources, {})

    def test_module_sources_collects_json_fixture_text_separately(self):
        with tempfile.TemporaryDirectory() as temporary:
            test = Path(temporary) / "test"
            entry = test / "modules" / "entry.js"
            entry.parent.mkdir(parents=True)
            entry.write_text(
                "import value from './data.json' with { type: 'json' }; value;"
            )
            (test / "modules" / "data.json").write_text('{"a": 1}')

            sources, dynamic_sources, json_sources = module_sources(entry, test)
            self.assertEqual(set(sources), {"modules/entry.js"})
            self.assertEqual(dynamic_sources, {})
            self.assertEqual(json_sources, {"modules/data.json": '{"a": 1}'})

    def test_module_sources_collects_dynamic_only_json_fixture_text(self):
        with tempfile.TemporaryDirectory() as temporary:
            test = Path(temporary) / "test"
            entry = test / "modules" / "entry.js"
            entry.parent.mkdir(parents=True)
            entry.write_text("import('./data.json', {with: {type: 'json'}});")
            (test / "modules" / "data.json").write_text('{"a": 1}')

            sources, dynamic_sources, json_sources = module_sources(
                entry, test, include_dynamic_string_roots=True
            )
            self.assertEqual(set(sources), {"modules/entry.js"})
            self.assertEqual(dynamic_sources, {})
            self.assertEqual(json_sources, {"modules/data.json": '{"a": 1}'})

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

    def test_windows_supervisor_uses_pipe_thread_and_process_kill(self):
        with tempfile.TemporaryDirectory() as temporary:
            executable = Path(temporary) / "adapter"
            executable.write_text(
                "#!/usr/bin/env python3\nimport json,sys,time\n"
                "print('{\"ready\":1}',flush=True)\nfor line in sys.stdin:\n"
                " request=json.loads(line)\n if request.get('stall'): time.sleep(10)\n"
                " print('{\"kind\":\"ok\"}',flush=True)\n"
            )
            executable.chmod(0o755)
            worker = Worker(executable, 0.05)
            try:
                # Exercise the Windows-specific pipe exchange on the portable
                # fixture; native Windows has no `os.killpg` or selectable
                # subprocess pipes.
                with mock.patch.object(run.os, "name", "nt"):
                    result = worker.run({"stall": True})
                    self.assertEqual(result["kind"], "timeout", result)
                    self.assertEqual(worker.run({})["kind"], "ok")
            finally:
                worker.close()

    def test_supervisor_falls_back_to_direct_kill_when_group_signal_is_denied(self):
        with tempfile.TemporaryDirectory() as temporary:
            executable = Path(temporary) / "adapter"
            executable.write_text(
                "#!/usr/bin/env python3\nimport json,sys,time\n"
                "print('{\"ready\":1}',flush=True)\nfor line in sys.stdin:\n"
                " request=json.loads(line)\n if request.get('stall'): time.sleep(10)\n"
                " print('{\"kind\":\"ok\"}',flush=True)\n"
            )
            executable.chmod(0o755)
            worker = Worker(executable, 0.05)
            try:
                # macOS may reject killpg after an adapter changes group state.
                # The directly-owned adapter must still be restartable.
                with mock.patch.object(run.os, "killpg", side_effect=PermissionError):
                    self.assertEqual(worker.run({"stall": True})["kind"], "timeout")
                    self.assertEqual(worker.run({})["kind"], "ok")
            finally:
                worker.close()


if __name__ == "__main__":
    unittest.main()
