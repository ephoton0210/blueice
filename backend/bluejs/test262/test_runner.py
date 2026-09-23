# This Source Code Form is subject to the terms of the Mozilla Public
# License, v. 2.0. If a copy of the MPL was not distributed with this
# file, You can obtain one at https://mozilla.org/MPL/2.0/.

import tempfile
from pathlib import Path
import unittest
from unittest import mock

import run
from run import (
    HOST_CAN_BLOCK,
    ITERATOR_ZIP_BASIC_MATRIX_FIXTURES,
    ITERATOR_ZIP_BASIC_MATRIX_INSTRUCTION_BUDGET,
    FINITE_STRESS_FIXTURES,
    FINITE_STRESS_INSTRUCTION_BUDGET,
    FINITE_STRESS_TIMEOUT,
    LARGE_FIXTURE_RESOURCES,
    ITERATOR_ZIP_BASIC_MATRIX_TIMEOUT,
    TEMPORAL_CALENDAR_TABLE_FIXTURES,
    TEMPORAL_CALENDAR_TABLE_INSTRUCTION_BUDGET,
    TEMPORAL_TIME_ZONE_ID_TABLE_FIXTURES,
    TEMPORAL_TIME_ZONE_ID_TABLE_INSTRUCTION_BUDGET,
    TEMPORAL_TIME_ZONE_LINK_TABLE_FIXTURES,
    TEMPORAL_TIME_ZONE_LINK_TABLE_INSTRUCTION_BUDGET,
    FINITE_FIXTURE_INSTRUCTION_BUDGETS,
    FIXTURE_STRING_LIMITS,
    STRING_SUBSTR_NUMBER_MATRIX_FIXTURES,
    STRING_SUBSTR_NUMBER_MATRIX_INSTRUCTION_BUDGET,
    WALL_CLOCK_BUSY_WAIT_FIXTURES,
    WALL_CLOCK_BUSY_WAIT_INSTRUCTION_BUDGET,
    ZONED_DATE_TIME_SAME_EPOCH_MATRIX_FIXTURES,
    ZONED_DATE_TIME_SAME_EPOCH_MATRIX_INSTRUCTION_BUDGET,
    Worker,
    canblock_exclusion,
    case_timeout,
    classify,
    default_jobs,
    large_fixture_limits,
    execution_source,
    fixture_string_limit,
    format_progress,
    instruction_budget,
    metadata,
    modes,
    module_source_requests,
    module_sources,
    selected_files,
    stale_corpus_reason,
    STALE_CORPUS_FIXTURES,
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

    def test_only_sta_and_assert_are_native_includes(self):
        # `propertyHelper.js` and `isConstructor.js` define behaviour the
        # native counterparts only approximated (the destructive probes, the
        # `restore` option, exact messages), so they run their upstream
        # source like every other include; the adapter rejects them as
        # unsupported unless the runner supplies that source.
        self.assertEqual(run.NATIVE_INCLUDES, frozenset({"sta.js", "assert.js"}))
        for include in (
            "propertyHelper.js",
            "isConstructor.js",
            "compareArray.js",
            "deepEqual.js",
            "testTypedArray.js",
        ):
            self.assertNotIn(include, run.NATIVE_INCLUDES)

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
            "built-ins/ArrayBuffer/prototype/sliceToImmutable/argument-coercion.js",
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

    def test_typed_array_detach_during_coercion_fixtures_have_a_measured_envelope(self):
        # These three fixtures run testTypedArray.js's byte-copy loop over a
        # 10,000-element source for every constructor/factory pair (about
        # 27M dispatches), so the generic 10M/60s harness class is too small.
        data = {"includes": ["testTypedArray.js", "detachArrayBuffer.js"]}
        for relative in (
            "built-ins/TypedArray/prototype/copyWithin/coerced-values-end-detached-prototype.js",
            "built-ins/TypedArray/prototype/copyWithin/coerced-values-end-detached.js",
            "built-ins/TypedArray/prototype/copyWithin/coerced-values-start-detached.js",
        ):
            self.assertEqual(instruction_budget(data, 100_000, relative), 50_000_000)
            self.assertEqual(case_timeout(data, 2, relative), 120)
        # The allowance is exact-path only: a sibling fixture keeps the
        # generic TypedArray harness envelope.
        sibling = "built-ins/TypedArray/prototype/copyWithin/coerced-values-target-detached.js"
        self.assertEqual(instruction_budget(data, 100_000, sibling), 10_000_000)
        self.assertEqual(case_timeout(data, 2, sibling), 60)

    def test_substr_number_matrix_gets_an_exact_path_dispatch_allowance(self):
        # annexB/.../substr/start-and-length-as-numbers.js checks
        # String.prototype.substr against a reference implementation for
        # 4 strings x 35 starts x 36 lengths = 5,040 finite calls, each with a
        # per-character comparison loop. Measured minimum: 1,496,386
        # dispatches (identically in both modes), about 15x the default. The
        # allowance is exact-path and leaves the 2 s wall deadline untouched.
        relative = "annexB/built-ins/String/prototype/substr/start-and-length-as-numbers.js"
        self.assertEqual(STRING_SUBSTR_NUMBER_MATRIX_FIXTURES, frozenset({relative}))
        self.assertEqual(STRING_SUBSTR_NUMBER_MATRIX_INSTRUCTION_BUDGET, 6_000_000)
        self.assertEqual(instruction_budget({}, 100_000, relative), 6_000_000)
        self.assertEqual(case_timeout({}, 2, relative), 2)
        # A sibling fixture keeps the default, and so does an unrelated path
        # whose name merely resembles this one.
        for sibling in (
            "annexB/built-ins/String/prototype/substr/length-negative.js",
            "annexB/built-ins/String/prototype/substr/start-and-length-as-numbers-2.js",
            "built-ins/String/prototype/substring/start-and-length-as-numbers.js",
        ):
            self.assertNotIn(sibling, STRING_SUBSTR_NUMBER_MATRIX_FIXTURES)
            self.assertEqual(instruction_budget({}, 100_000, sibling), 100_000)
            self.assertEqual(case_timeout({}, 2, sibling), 2)
        # The allowance raises the floor only: a larger default is kept, so a
        # caller asking for more dispatches is never reduced.
        self.assertEqual(
            instruction_budget({}, 20_000_000, relative), 20_000_000
        )

    def test_large_finite_fixtures_get_exact_path_resource_allowances(self):
        # Each of these fixtures is finite but bigger than the default 1 MiB
        # string, 16 MiB heap (which also caps an ArrayBuffer), dispatch or
        # wall-clock ceiling. The allowances are about 3-4x the measured
        # minimum, exact-path only and never unlimited.
        mib = 1024 * 1024
        self.assertEqual(
            LARGE_FIXTURE_RESOURCES,
            {
                "staging/sm/extensions/dataview.js": {"heap_limit": 64 * mib},
                "staging/sm/RegExp/unicode-braced.js": {
                    "string_limit": 128 * mib,
                    "heap_limit": 128 * mib,
                    "timeout": 20,
                },
                "staging/sm/RegExp/unicode-class-braced.js": {
                    "string_limit": 128 * mib,
                    "heap_limit": 128 * mib,
                    "timeout": 20,
                },
                "staging/sm/regress/regress-610026.js": {
                    "string_limit": 64 * mib,
                    "heap_limit": 128 * mib,
                    "instruction_budget": 100_000_000,
                    "timeout": 90,
                },
                "staging/sm/JSON/parse-mega-huge-array.js": {
                    "string_limit": 16 * mib,
                    "heap_limit": 512 * mib,
                    "instruction_budget": 50_000_000,
                    "timeout": 20,
                },
                "staging/sm/String/replace-math.js": {"string_limit": 4 * mib},
                **{
                    f"staging/sm/Date/dst-offset-caching-{part}-of-8.js": {
                        "instruction_budget": 200_000_000,
                        "timeout": 70,
                    }
                    for part in range(1, 9)
                },
                "staging/sm/Date/two-digit-years.js": {
                    "instruction_budget": 30_000_000,
                    "timeout": 10,
                },
                "staging/sm/Array/toSpliced-dense.js": {
                    "instruction_budget": 80_000_000,
                    "timeout": 20,
                },
                "staging/sm/regress/regress-1507322-deep-weakmap.js": {
                    "instruction_budget": 30_000_000,
                    "heap_limit": 64 * mib,
                    "timeout": 15,
                },
            },
        )
        # The heavy-fuel/wall-clock staging fixtures each get their own exact
        # dispatch budget and wall deadline; only the deep-WeakMap fixture
        # also needs a bigger managed heap.
        for part in range(1, 9):
            dst = f"staging/sm/Date/dst-offset-caching-{part}-of-8.js"
            self.assertEqual(instruction_budget({}, 100_000, dst), 200_000_000, dst)
            self.assertEqual(case_timeout({}, 2, dst), 70, dst)
            self.assertEqual(large_fixture_limits(dst), {}, dst)
        two_digit_years = "staging/sm/Date/two-digit-years.js"
        self.assertEqual(instruction_budget({}, 100_000, two_digit_years), 30_000_000)
        self.assertEqual(case_timeout({}, 2, two_digit_years), 10)
        to_spliced = "staging/sm/Array/toSpliced-dense.js"
        self.assertEqual(instruction_budget({}, 100_000, to_spliced), 80_000_000)
        self.assertEqual(case_timeout({}, 2, to_spliced), 20)
        deep_weakmap = "staging/sm/regress/regress-1507322-deep-weakmap.js"
        self.assertEqual(instruction_budget({}, 100_000, deep_weakmap), 30_000_000)
        self.assertEqual(case_timeout({}, 2, deep_weakmap), 15)
        self.assertEqual(large_fixture_limits(deep_weakmap), {"heap_limit": 64 * mib})
        # Dispatch budget and wall deadline follow the table, and only for
        # the entries that name them.
        long_running = "staging/sm/regress/regress-610026.js"
        self.assertEqual(instruction_budget({}, 100_000, long_running), 100_000_000)
        self.assertEqual(case_timeout({}, 2, long_running), 90)
        braced = "staging/sm/RegExp/unicode-braced.js"
        self.assertEqual(instruction_budget({}, 100_000, braced), 100_000)
        self.assertEqual(case_timeout({}, 2, braced), 20)
        dataview = "staging/sm/extensions/dataview.js"
        self.assertEqual(instruction_budget({}, 100_000, dataview), 100_000)
        self.assertEqual(case_timeout({}, 2, dataview), 2)
        # The byte limits go to the adapter with the request.
        self.assertEqual(large_fixture_limits(dataview), {"heap_limit": 64 * mib})
        self.assertEqual(
            large_fixture_limits(long_running),
            {"string_limit": 64 * mib, "heap_limit": 128 * mib},
        )
        self.assertEqual(large_fixture_limits("staging/sm/regress/regress-610025.js"), {})
        huge_array = "staging/sm/JSON/parse-mega-huge-array.js"
        self.assertEqual(
            large_fixture_limits(huge_array),
            {"string_limit": 16 * mib, "heap_limit": 512 * mib},
        )
        self.assertEqual(instruction_budget({}, 100_000, huge_array), 50_000_000)
        self.assertEqual(case_timeout({}, 2, huge_array), 20)
        replace_math = "staging/sm/String/replace-math.js"
        self.assertEqual(large_fixture_limits(replace_math), {"string_limit": 4 * mib})
        self.assertEqual(instruction_budget({}, 100_000, replace_math), 100_000)
        self.assertEqual(case_timeout({}, 2, replace_math), 2)
        # A larger default is never reduced.
        self.assertEqual(instruction_budget({}, 200_000_000, long_running), 200_000_000)
        self.assertEqual(case_timeout({}, 120, long_running), 120)
        # Neighbouring and same-named paths keep every default.
        for sibling in (
            "staging/sm/regress/regress-610025.js",
            "staging/sm/RegExp/unicode-lead-trail.js",
            "staging/sm/extensions/dataview2.js",
            "built-ins/DataView/extensions/dataview.js",
            "test/staging/sm/regress/regress-610026.js",
        ):
            self.assertNotIn(sibling, LARGE_FIXTURE_RESOURCES)
            self.assertEqual(instruction_budget({}, 100_000, sibling), 100_000)
            self.assertEqual(case_timeout({}, 2, sibling), 2)

    def test_finite_staging_fixtures_get_exact_path_dispatch_allowances(self):
        # Each of these fixtures is a fixed, finite loop or a run of eagerly
        # message-building assertions whose size only just exceeds the
        # 100,000-dispatch default. The allowance is 4x the measured minimum
        # (identical in sloppy and strict mode), applies to the exact path
        # only, and leaves the ordinary 2 s wall deadline untouched. Fixtures
        # that are too slow for that deadline even with fuel (the
        # dst-offset-caching parts, toSpliced-dense) must NOT be listed here:
        # they need a wall-deadline allowance too, which only
        # `LARGE_FIXTURE_RESOURCES` can express.
        self.assertEqual(
            FINITE_FIXTURE_INSTRUCTION_BUDGETS,
            {
                "staging/sm/Array/with-dense.js": 750_000,
                "staging/sm/JSON/parse-reviver-array-delete.js": 750_000,
                "staging/sm/Math/log2-approx.js": 1_300_000,
                "staging/sm/extensions/es5ish-defineGetter-defineSetter.js": 450_000,
            },
        )
        for relative, budget in FINITE_FIXTURE_INSTRUCTION_BUDGETS.items():
            self.assertEqual(instruction_budget({}, 100_000, relative), budget, relative)
            self.assertEqual(case_timeout({}, 2, relative), 2, relative)
            # Raises the floor only: a larger default is never reduced.
            self.assertEqual(instruction_budget({}, budget * 10, relative), budget * 10)
        for other in (
            "staging/sm/Array/with-dense-2.js",
            "built-ins/Array/prototype/with/index-bigger-or-eq-than-length.js",
        ):
            self.assertNotIn(other, FINITE_FIXTURE_INSTRUCTION_BUDGETS)
            self.assertEqual(instruction_budget({}, 100_000, other), 100_000, other)
            self.assertEqual(case_timeout({}, 2, other), 2, other)
        for heavy in (
            "staging/sm/Array/toSpliced-dense.js",
            "staging/sm/Date/dst-offset-caching-1-of-8.js",
        ):
            self.assertNotIn(heavy, FINITE_FIXTURE_INSTRUCTION_BUDGETS)
            self.assertIn(heavy, LARGE_FIXTURE_RESOURCES)

    def test_a_fixture_that_needs_a_huge_string_gets_an_exact_path_string_limit(self):
        # staging/sm/String/unicode-braced.js evaluates a source string built
        # from 2**24 zeros, which is 32 MiB of UTF-16 by itself and needs a
        # string limit of at least 33,558,528 bytes (33,554,432 fails). The
        # rest of the fixture is ordinary and takes about a second, well inside
        # the unchanged 2 s wall deadline and the default dispatch budget. The
        # limit is 64 MiB, twice the requirement: it is a data size, so extra
        # headroom would buy nothing.
        relative = "staging/sm/String/unicode-braced.js"
        self.assertEqual(FIXTURE_STRING_LIMITS, {relative: 64 * 1024 * 1024})
        self.assertEqual(fixture_string_limit(relative), 64 * 1024 * 1024)
        self.assertEqual(instruction_budget({}, 100_000, relative), 100_000)
        self.assertEqual(case_timeout({}, 2, relative), 2)
        # Neighbours, and fixtures whose failure is a different resource (a
        # string of 2**36 units, a 2**21-element JSON array), keep the default.
        for other in (
            "staging/sm/String/unicode-braced-2.js",
            "staging/sm/String/replace-math.js",
            "staging/sm/JSON/parse-mega-huge-array.js",
            "staging/sm/RegExp/unicode-class-braced.js",
            "built-ins/String/prototype/repeat/repeat-string-n-times.js",
        ):
            self.assertIsNone(fixture_string_limit(other), other)

    def test_wall_clock_busy_wait_fixture_gets_an_exact_path_dispatch_allowance(self):
        # await-import-evaluation_FIXTURE.js spins `while (true)` until
        # Date.now() has advanced 100 ms, so its dispatch count is a property
        # of the machine (about 0.3M-1M dispatches here), not of the test. The
        # allowance is exact-path, leaves the 2 s wall deadline untouched, and
        # a sibling keeps the default budget.
        relative = "language/expressions/dynamic-import/await-import-evaluation.js"
        self.assertEqual(WALL_CLOCK_BUSY_WAIT_FIXTURES, frozenset({relative}))
        self.assertEqual(WALL_CLOCK_BUSY_WAIT_INSTRUCTION_BUDGET, 10_000_000)
        self.assertEqual(
            instruction_budget({}, 100_000, relative),
            WALL_CLOCK_BUSY_WAIT_INSTRUCTION_BUDGET,
        )
        self.assertEqual(instruction_budget({}, 50_000_000, relative), 50_000_000)
        self.assertEqual(case_timeout({}, 2, relative), 2)
        sibling = "language/expressions/dynamic-import/await-import-evaluation-2.js"
        self.assertEqual(instruction_budget({}, 100_000, sibling), 100_000)

    def test_canblock_flag_mismatch_is_excluded_not_failed_or_unsupported(self):
        # This host's Atomics.wait genuinely suspends the agent (every
        # CanBlockIsTrue fixture already passes against that behavior), so
        # its declared [[CanBlock]] is true. A CanBlockIsFalse fixture
        # assumes the opposite and can only be satisfied by making
        # Atomics.wait always throw, which would break every already-passing
        # CanBlockIsTrue fixture -- so it is excluded rather than dispatched,
        # and "excluded" is its own status, never conflated with
        # "unsupported" (a capability this host actually lacks).
        self.assertTrue(HOST_CAN_BLOCK)
        reason = canblock_exclusion(["CanBlockIsFalse"])
        self.assertIsNotNone(reason)
        self.assertIn("CanBlockIsFalse", reason)
        # A flag combined with an unrelated one is still excluded.
        self.assertIsNotNone(canblock_exclusion(["onlyStrict", "CanBlockIsFalse"]))
        # The matching flag, and no flag at all, are not excluded.
        self.assertIsNone(canblock_exclusion(["CanBlockIsTrue"]))
        self.assertIsNone(canblock_exclusion([]))
        self.assertIsNone(canblock_exclusion(["onlyStrict"]))

    def test_excluded_kind_classifies_as_its_own_status(self):
        self.assertEqual(classify({"kind": "excluded", "reason": "x"}, None), "excluded")
        # Never satisfies a negative-error expectation either.
        expected = {"phase": "runtime", "type": "TypeError"}
        self.assertEqual(classify({"kind": "excluded", "reason": "x"}, expected), "excluded")

    def test_stale_corpus_kind_classifies_as_its_own_status(self):
        # `analyze.py` re-derives every record's status with `classify` and
        # rejects the run if it disagrees with the runner's own, so a
        # `stale_corpus` record (which carries a `stale_corpus` reply kind and
        # never reaches the adapter) must round-trip rather than becoming "fail".
        self.assertEqual(
            classify({"kind": "stale_corpus", "reason": "x"}, None), "stale_corpus"
        )
        expected = {"phase": "runtime", "type": "TypeError"}
        self.assertEqual(
            classify({"kind": "stale_corpus", "reason": "x"}, expected), "stale_corpus"
        )

    def test_stale_corpus_fixtures_are_a_distinct_status_from_fail_unsupported_and_excluded(self):
        # A fixture whose own assertions contradict the *current* ECMA-262
        # draft (verified directly against the live spec text, not merely
        # inferred from disagreement), with an upstream Test262 issue/fix
        # already open, is not a BlueJS engine gap ("fail"), a capability
        # this host lacks ("unsupported"), or a host capability declaration
        # ("excluded") -- it is the corpus itself that hasn't caught up.
        stale_path = "annexB/language/function-code/block-decl-func-skip-arguments.js"
        self.assertIn(stale_path, STALE_CORPUS_FIXTURES)
        reason = stale_corpus_reason(stale_path)
        self.assertIsNotNone(reason)
        self.assertIn("tc39/test262#5113", reason)
        self.assertIn("tc39/test262#5112", reason)
        # Every entry documents both the spec section verified and the
        # upstream issue/PR -- never a bare "we disagree" -- and every
        # unrelated path is unaffected.
        for path, entry_reason in STALE_CORPUS_FIXTURES.items():
            self.assertRegex(entry_reason, r"tc39/test262#\d+")
        self.assertIsNone(stale_corpus_reason("staging/sm/lexical-environment/block-scoped-functions-annex-b-arguments.js"))
        self.assertIsNone(stale_corpus_reason("annexB/language/function-code/block-decl-func-skip-arguments2.js"))
        self.assertIsNone(stale_corpus_reason(""))

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

    def test_the_all_pairs_time_zone_comparison_is_a_finite_stress_fixture(self):
        # canonical-not-equal.js compares every pair of the ~446 primary time
        # zone identifiers (about 99,000 pairs): finite, but well past the
        # ordinary budget and wall deadline. Its neighbours keep the defaults.
        relative = "intl402/Temporal/ZonedDateTime/prototype/equals/canonical-not-equal.js"
        self.assertIn(relative, FINITE_STRESS_FIXTURES)
        self.assertEqual(
            instruction_budget({}, 100_000, relative),
            FINITE_STRESS_INSTRUCTION_BUDGET,
        )
        self.assertEqual(case_timeout({}, 2, relative), FINITE_STRESS_TIMEOUT)
        neighbour = "intl402/Temporal/ZonedDateTime/prototype/equals/argument-valid.js"
        self.assertNotIn(neighbour, FINITE_STRESS_FIXTURES)
        self.assertEqual(instruction_budget({}, 100_000, neighbour), 100_000)

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
            TEMPORAL_TIME_ZONE_ID_TABLE_FIXTURES,
            frozenset(
                {"intl402/Temporal/ZonedDateTime/from/timezone-case-insensitive.js"}
            ),
        )
        for relative in TEMPORAL_TIME_ZONE_ID_TABLE_FIXTURES:
            self.assertEqual(
                instruction_budget({}, 100_000, relative),
                TEMPORAL_TIME_ZONE_ID_TABLE_INSTRUCTION_BUDGET,
            )
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
            sources, dynamic_sources, json_sources, text_sources, bytes_sources = module_sources(entry, test)
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
            sources, dynamic_sources, json_sources, text_sources, bytes_sources = module_sources(entry, test)
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

            sources, dynamic_sources, json_sources, text_sources, bytes_sources = module_sources(entry, test)
            self.assertEqual(set(sources), {"modules/entry.js", "modules/both.js"})
            self.assertEqual(dynamic_sources, {})
            self.assertEqual(json_sources, {})

    def test_module_sources_keeps_a_literal_dynamic_specifier_dynamic_with_string_roots(self):
        with tempfile.TemporaryDirectory() as temporary:
            test = Path(temporary) / "test"
            entry = test / "modules" / "entry.js"
            entry.parent.mkdir(parents=True)
            entry.write_text(
                "import('./only-dynamic.js'); "
                "const candidate = './variable.js'; import(candidate); "
                "import('./both.js'); const both = './both.js';"
            )
            (test / "modules" / "only-dynamic.js").write_text("var a; function a() {}")
            (test / "modules" / "variable.js").write_text("export const value = true;")
            (test / "modules" / "both.js").write_text("export const value = true;")

            # The literal `import('./only-dynamic.js')` stays a dynamic edge
            # even though its string also looks like a relative-string root;
            # a bare relative string (the variable candidate) and a string
            # that also appears outside an import call stay static.
            sources, dynamic_sources, json_sources, text_sources, bytes_sources = module_sources(
                entry, test, include_dynamic_string_roots=True
            )
            self.assertEqual(
                set(sources),
                {"modules/entry.js", "modules/variable.js", "modules/both.js"},
            )
            self.assertEqual(set(dynamic_sources), {"modules/only-dynamic.js"})
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

            sources, dynamic_sources, json_sources, text_sources, bytes_sources = module_sources(entry, test)
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

            sources, dynamic_sources, json_sources, text_sources, bytes_sources = module_sources(
                entry, test, include_dynamic_string_roots=True
            )
            self.assertEqual(set(sources), {"modules/entry.js"})
            self.assertEqual(dynamic_sources, {})
            self.assertEqual(json_sources, {"modules/data.json": '{"a": 1}'})

    def test_module_sources_classifies_text_and_bytes_fixtures_by_their_type_attribute(self):
        with tempfile.TemporaryDirectory() as temporary:
            test = Path(temporary) / "test"
            entry = test / "modules" / "entry.js"
            entry.parent.mkdir(parents=True)
            entry.write_text(
                "import text from './plain_FIXTURE' with { type: 'text' };\n"
                "import bytes from './image_FIXTURE.png' with { type: \"bytes\" };\n"
                "import json from './data_FIXTURE.json' with { type: 'json' };\n"
                "export { default as again } from './plain_FIXTURE' with { type: 'text' };\n"
                "export * as ns from './note_FIXTURE.txt' with { type: 'text' };\n"
            )
            (test / "modules" / "plain_FIXTURE").write_text("plain\n")
            (test / "modules" / "image_FIXTURE.png").write_bytes(b"\x89PNG\x00\xff")
            (test / "modules" / "data_FIXTURE.json").write_text('{"a": 1}')
            (test / "modules" / "note_FIXTURE.txt").write_text("note")

            found = module_sources(entry, test)
            self.assertEqual(set(found.sources), {"modules/entry.js"})
            self.assertEqual(
                found.text_sources,
                {"modules/plain_FIXTURE": "plain\n", "modules/note_FIXTURE.txt": "note"},
            )
            self.assertEqual(
                found.bytes_sources, {"modules/image_FIXTURE.png": [0x89, 0x50, 0x4E, 0x47, 0, 0xFF]}
            )
            self.assertEqual(found.json_sources, {"modules/data_FIXTURE.json": '{"a": 1}'})

    def test_module_sources_type_attribute_wins_over_the_file_extension(self):
        # A `.json` fixture imported as bytes is bytes, not JSON, and a `.js`
        # fixture imported as text is never parsed as a module.
        with tempfile.TemporaryDirectory() as temporary:
            test = Path(temporary) / "test"
            entry = test / "modules" / "entry.js"
            entry.parent.mkdir(parents=True)
            entry.write_text(
                "import a from './data.json' with { type: 'bytes' };\n"
                "import b from './invalid.js' with { type: 'text' };\n"
            )
            (test / "modules" / "data.json").write_text('{"a": 1}')
            (test / "modules" / "invalid.js").write_text("invalid { javascript")

            found = module_sources(entry, test)
            self.assertEqual(set(found.sources), {"modules/entry.js"})
            self.assertEqual(found.dynamic_sources, {})
            self.assertEqual(found.json_sources, {})
            self.assertEqual(found.text_sources, {"modules/invalid.js": "invalid { javascript"})
            self.assertEqual(found.bytes_sources, {"modules/data.json": list(b'{"a": 1}')})

    def test_module_sources_supplies_a_module_that_imports_itself_as_text(self):
        # One path, two request identities: the entry module and its text.
        with tempfile.TemporaryDirectory() as temporary:
            test = Path(temporary) / "test"
            entry = test / "modules" / "self.js"
            entry.parent.mkdir(parents=True)
            source = "import value from './self.js' with { type: 'text' };"
            entry.write_text(source)

            found = module_sources(entry, test)
            self.assertEqual(found.sources, {"modules/self.js": source})
            self.assertEqual(found.text_sources, {"modules/self.js": source})

    def test_module_sources_decodes_text_as_utf8_and_drops_a_leading_bom(self):
        with tempfile.TemporaryDirectory() as temporary:
            test = Path(temporary) / "test"
            entry = test / "modules" / "entry.js"
            entry.parent.mkdir(parents=True)
            entry.write_text("import a from './a.txt' with { type: 'text' };")
            (test / "modules" / "a.txt").write_bytes(b"\xef\xbb\xbfcaf\xc3\xa9 \xff")

            found = module_sources(entry, test)
            # UTF-8 decode: BOM removed, malformed byte becomes U+FFFD.
            self.assertEqual(found.text_sources, {"modules/a.txt": "caf\u00e9 \ufffd"})

    def test_module_sources_reads_a_type_attribute_on_a_dynamic_import(self):
        with tempfile.TemporaryDirectory() as temporary:
            test = Path(temporary) / "test"
            entry = test / "modules" / "entry.js"
            entry.parent.mkdir(parents=True)
            entry.write_text(
                "import('./fixture.json', { with: { type: 'text' } });\n"
                "import('./other.bin', {\n  with: { type: 'bytes' },\n});\n"
                "import('./plain.js');\n"
            )
            (test / "modules" / "fixture.json").write_text("{}")
            (test / "modules" / "other.bin").write_bytes(b"\x00\x01")
            (test / "modules" / "plain.js").write_text("export {};")

            found = module_sources(entry, test)
            self.assertEqual(found.text_sources, {"modules/fixture.json": "{}"})
            self.assertEqual(found.bytes_sources, {"modules/other.bin": [0, 1]})
            self.assertEqual(found.json_sources, {})
            self.assertEqual(set(found.dynamic_sources), {"modules/plain.js"})

    def test_module_sources_untyped_relative_string_to_a_json_fixture_stays_json(self):
        with tempfile.TemporaryDirectory() as temporary:
            test = Path(temporary) / "test"
            entry = test / "modules" / "entry.js"
            entry.parent.mkdir(parents=True)
            entry.write_text("const specifier = './data.json'; import(specifier);")
            (test / "modules" / "data.json").write_text("[]")

            found = module_sources(entry, test, include_dynamic_string_roots=True)
            self.assertEqual(found.json_sources, {"modules/data.json": "[]"})
            self.assertEqual(found.text_sources, {})
            self.assertEqual(found.bytes_sources, {})

    def test_module_sources_finds_requests_written_without_whitespace_around_punctuation(self):
        # `export*from"./a.js"` and `import{b}from"./b.js"` are ordinary
        # module declarations (staging/sm/module/bug1488117.js writes the
        # first); a fixture reachable only through one must still be supplied.
        with tempfile.TemporaryDirectory() as temporary:
            test = Path(temporary) / "test"
            entry = test / "modules" / "entry.js"
            entry.parent.mkdir(parents=True)
            entry.write_text(
                'export* from "./star.js";\n'
                'export*as ns from"./namespace.js";\n'
                'export{x}from"./named.js";\n'
                'import{y}from"./imported.js";\n'
                'import*as z from"./whole.js";\n'
                'import"./bare.js";\n'
            )
            names = ["star", "namespace", "named", "imported", "whole", "bare"]
            for name in names:
                (test / "modules" / f"{name}.js").write_text("export const x = 1;")

            found = module_sources(entry, test)
            self.assertEqual(
                set(found.sources),
                {"modules/entry.js"} | {f"modules/{name}.js" for name in names},
            )

    def test_module_source_requests_find_static_and_dynamic_host_source_specifiers(self):
        # `<module source>` is Test262's host-provided Module Source; a test
        # names it in a static `import source` or a dynamic `import.source()`.
        self.assertEqual(module_source_requests({}), [])
        self.assertEqual(
            module_source_requests(
                {
                    "a.js": "import source x from '<module source>';",
                    "b.js": "import source y from './other.js';",
                }
            ),
            ["<module source>"],
        )
        self.assertEqual(
            module_source_requests(
                {"c.js": "const s = await import.source(  \"<module source>\"  );"}
            ),
            ["<module source>"],
        )
        # A plain `import()` or an unrelated specifier never registers one.
        self.assertEqual(
            module_source_requests(
                {
                    "d.js": "import('<module source>'); import.source('./x.js');",
                    "e.js": "import.defer('<module source>');",
                }
            ),
            [],
        )
        # Both forms together still yield one entry.
        self.assertEqual(
            module_source_requests(
                {
                    "f.js": "import source x from '<module source>';"
                    "import.source('<module source>');"
                }
            ),
            ["<module source>"],
        )

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
