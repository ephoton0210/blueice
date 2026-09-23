#!/usr/bin/env python3
# This Source Code Form is subject to the terms of the Mozilla Public
# License, v. 2.0. If a copy of the MPL was not distributed with this
# file, You can obtain one at https://mozilla.org/MPL/2.0/.

"""Complete, pinned Test262 inventory and supervised BlueJS execution.

Unsupported cases remain in the denominator. Exit 0 requires every mode to
pass; exit 1 means non-passing cases, exit 2 means runner/configuration failure.
Only the named _FIXTURE resources are excluded from test discovery.
"""
import argparse
import collections
import concurrent.futures
import hashlib
import io
import json
import os
from pathlib import Path
import queue
import re
import selectors
import signal
import subprocess
import tarfile
import threading
import time
import urllib.request

import yaml

ROOT = Path(__file__).resolve().parents[3]
SNAPSHOT = json.loads(Path(__file__).with_name("snapshot.json").read_text())
FRONTMATTER = re.compile(r"/\*---(.*?)---\*/", re.DOTALL)
# The optional trailing `with { type: "..." }` import-attributes clause of a
# static module request; its capture group is the `type` value. A request's
# `type` selects how the host loads the resource (`json`, `text`, `bytes`),
# so it decides which of the adapter's source maps the resource goes into.
IMPORT_ATTRIBUTE_TYPE = r'''(?:\s*with\s*\{[^}]*?\btype\s*:\s*["']([^"']*)["'][^}]*\})?'''
# Whitespace is optional wherever punctuation already separates the tokens:
# `export*from"x"`, `export{a}from"x"` and `import{a}from"x"` are declarations
# too. (`import(` and `import.` stay out: only a space, `*`, `{` or a string
# may follow the `import` keyword of a declaration.)
MODULE_REQUEST = re.compile(
    r'''\bimport(?:\s+|(?=[*{"']))(?:[^;]*?\bfrom\s*)?["']([^"']+)["']'''
    + IMPORT_ATTRIBUTE_TYPE
    + r'''|\bexport\s*(?:\*\s*(?:as\s+(?:[\w$]+|"[^"]*"|'[^']*')\s*)?|\{[^}]*\}\s*)from\s*["']([^"']+)["']'''
    + IMPORT_ATTRIBUTE_TYPE,
    re.DOTALL,
)
# A plain `import(...)` reference is safe to classify as a "dynamic" edge in
# `module_sources` (see its own docstring): its target may be compiled on
# demand by `Vm::ensure_dynamic_module_compiled`. `import.source(...)`/
# `import.defer(...)` have no such lazy-compile counterpart -- source-phase
# dynamic import specifically resolves by checking whether the target is
# *already* a compiled Source Text Module -- so a reference through either
# of those must still be treated as a "static" (eagerly compiled) edge.
# `import("./x")` and `import("./x", { with: { type: "..." } })` (an optional
# trailing comma is allowed); the second capture group is the `type` value.
DYNAMIC_IMPORT_PLAIN_REQUEST = re.compile(
    r'''\bimport\s*\(\s*["']([^"']+)["']\s*'''
    r'''(?:,\s*\{\s*with\s*:\s*\{[^}]*?\btype\s*:\s*["']([^"']*)["'][^}]*\}\s*,?\s*\}\s*)?,?\s*\)'''
)
DYNAMIC_IMPORT_SOURCE_OR_DEFER_REQUEST = re.compile(
    r'''\bimport\s*\.\s*(?:source|defer)\s*\(\s*["']([^"']+)["']\s*\)'''
)
DYNAMIC_IMPORT_EXPRESSION = re.compile(
    r'''\bimport\s*(?:\(|\.\s*(?:source|defer)\s*\()'''
)
# `ShadowRealm.prototype.importValue` is host-loaded through the same
# adapter module registry as dynamic `import()`, but it is an ordinary
# method call rather than `import`/`import.source`/`import.defer` syntax, so
# it needs its own trigger for bundling a relative-string sibling fixture.
SHADOW_REALM_IMPORT_VALUE_EXPRESSION = re.compile(r'''\.importValue\s*\(''')
RELATIVE_STRING = re.compile(r'''["'](\.{1,2}/[^"']+)["']''')
SOURCE_PHASE_IMPORT_REQUEST = re.compile(
    r'''\bimport\s+source\s+[\w$]+\s+from\s*["']([^"']+)["']''',
    re.DOTALL,
)
# The dynamic form of the same request: `import.source("...")`.
DYNAMIC_SOURCE_PHASE_IMPORT_REQUEST = re.compile(
    r'''\bimport\s*\.\s*source\s*\(\s*["']([^"']+)["']\s*,?\s*\)'''
)
# Test262's host-provided Module Source specifier (INTERPRETING.md).
HOST_MODULE_SOURCE_SPECIFIER = "<module source>"
# Only `sta.js` and `assert.js` are replaced by native helpers: a JavaScript
# `assert.js` costs enough dispatches per call to push loop-driven fixtures
# (e.g. `built-ins/Math/sqrt/results.js`) past the ordinary instruction budget,
# and the native `assert` family is differentially tested against the upstream
# source. `propertyHelper.js` and `isConstructor.js` run unchanged: their
# behaviour (destructive `delete`/write probes, `restore`, exact messages) is
# defined by that source, and executing it costs no fixture its budget.
NATIVE_INCLUDES = frozenset({"sta.js", "assert.js"})
# Test262's general deepEqual harness is preserved by default. The two
# DateTimeFormat fixtures compare wide arrays of two-/three-field part data
# records, and this TypedArray fixture compares 39 view/species pairs; their
# recursive JavaScript helper chain exhausts the VM's deliberately finite
# resource before observing the values under test. The adapter's bounded
# iterative comparison has been verified against these exact array/plain-record
# and TypedArray shapes.
NATIVE_DEEP_EQUAL_FIXTURES = frozenset({
    "intl402/DateTimeFormat/prototype/formatToParts/temporal-objects-resolved-time-zone.js",
    "intl402/DateTimeFormat/prototype/formatRangeToParts/temporal-objects-resolved-time-zone.js",
    "staging/sm/TypedArray/fill.js",
    "staging/sm/TypedArray/subarray-species.js",
})
# Running more VM subprocesses than the host has schedulable CPUs turns the
# ordinary per-case deadline into a scheduler-delay detector on slower hosts.
# Keep the historical eight-worker ceiling, while making the default portable
# across macOS, Linux, and Windows CI machines.
MAX_DEFAULT_JOBS = 8


def default_jobs(cpu_count=None):
    """Return the portable default worker count for a full inventory run."""
    if cpu_count is None:
        cpu_count = os.cpu_count()
    return max(1, min(MAX_DEFAULT_JOBS, cpu_count or 1))


TAIL_CALL_INSTRUCTION_BUDGET = 3_000_000
TAIL_CALL_TIMEOUT = 30
# This fixture traverses every reachable well-known intrinsic and validates
# its built-in function source representation. It is finite, but on a busy
# debug-interpreter worker its two modes can each take more than the general
# finite-stress allowance.
BUILTIN_FUNCTION_TOSTRING_FIXTURE = (
    "built-ins/Function/prototype/toString/built-in-function-object.js"
)
BUILTIN_FUNCTION_TOSTRING_TIMEOUT = 180
# These two fixed match-indices conformance files can wait for the dedicated
# RegExp worker to warm up under a parallel inventory. Keep that allowance
# attached to their exact paths instead of relaxing the default deadline for
# all RegExp tests.
REGEXP_MATCH_INDICES_FIXTURES = frozenset(
    {
        "built-ins/RegExp/match-indices/indices-array-non-unicode-match.js",
        "built-ins/RegExp/match-indices/indices-array-unicode-match.js",
    }
)
REGEXP_MATCH_INDICES_TIMEOUT = 30
# Test262's Unicode identifier tables contain tens of thousands of declarations
# and escaped identifier spellings. Parsing and compiling them is bounded work,
# but exceeds the general two-second script deadline in an interpreter build.
UNICODE_IDENTIFIER_PREFIX = "language/identifiers/start-unicode-"
UNICODE_IDENTIFIER_TIMEOUT = 30
# `testTypedArray.js` deliberately runs each callback across every numeric
# TypedArray constructor (and, for conversion cases, every entry in the large
# byte-conversion table).  Its execution remains bounded by the ordinary VM
# instruction budget, but debug interpreter dispatch takes longer than the
# general two-second wall deadline.  Give that standard harness a documented
# per-case wall allowance instead of weakening the deadline for all tests.
TYPED_ARRAY_HARNESS_TIMEOUT = 60
TYPED_ARRAY_HARNESS_INSTRUCTION_BUDGET = 10_000_000
# SpiderMonkey's staging TypedArray shell runs each assertion across every
# numeric and BigInt constructor, and a few finite sort matrices additionally
# visit 4,096 elements. It needs a distinct bounded envelope rather than
# weakening the default for unrelated Test262 scripts.
SM_TYPED_ARRAY_HARNESS = "sm/non262-TypedArray-shell.js"
SM_TYPED_ARRAY_HARNESS_TIMEOUT = 60
SM_TYPED_ARRAY_HARNESS_INSTRUCTION_BUDGET = 100_000_000
# This upstream regression deliberately performs 480,000 observable numeric
# conversions (including object ToNumber calls) across every TypedArray kind.
# On Ubuntu's debug adapter both modes take about 58 seconds serially, so a
# 90-second case envelope tolerates parallel-run scheduler contention without
# weakening the ordinary SM TypedArray harness deadline.
SM_TYPED_ARRAY_LONG_FIXTURES = frozenset(
    {"staging/sm/TypedArray/element-setting-converts-using-ToNumber.js"}
)
SM_TYPED_ARRAY_LONG_TIMEOUT = 90
# These three copyWithin fixtures build a 10,000-element source array and run
# testTypedArray.js's byte-by-byte `copyIntoArrayBuffer` loop for every
# constructor/factory pair (about 26 dispatches per byte, roughly 27M in
# total), which exceeds the generic testTypedArray.js envelope above. Measured
# at 27 s per mode on an idle debug adapter and about 50 s under load, so the
# allowance is scoped to these exact files with margin for scheduler
# contention rather than raising the harness-wide limits.
TYPED_ARRAY_DETACH_COERCION_FIXTURES = frozenset(
    {
        "built-ins/TypedArray/prototype/copyWithin/coerced-values-end-detached-prototype.js",
        "built-ins/TypedArray/prototype/copyWithin/coerced-values-end-detached.js",
        "built-ins/TypedArray/prototype/copyWithin/coerced-values-start-detached.js",
    }
)
TYPED_ARRAY_DETACH_COERCION_TIMEOUT = 120
# `dynamic-import/await-import-evaluation_FIXTURE.js` waits by spinning
# `while (true)` until `Date.now()` has advanced 100 ms, and its test asserts
# that the import promise settled only after that wait. How many dispatches
# 100 ms takes is a property of the host machine, not of the test (about
# 0.3M-1M here; a faster host needs proportionally more), so no fixed default
# budget is right for it. Grant this exact path an order-of-magnitude margin
# over the measurement; the ordinary two-second wall deadline still bounds it,
# and an unbounded loop in any other test keeps the 100,000-dispatch default.
WALL_CLOCK_BUSY_WAIT_FIXTURES = frozenset(
    {"language/expressions/dynamic-import/await-import-evaluation.js"}
)
WALL_CLOCK_BUSY_WAIT_INSTRUCTION_BUDGET = 10_000_000
# `annexB/.../String/prototype/substr/start-and-length-as-numbers.js` checks
# `substr` against a reference implementation for 4 strings x 35 starts x 36
# lengths (5,040 finite calls), each followed by a per-character comparison
# loop and several assertions. The matrix is fixed and only its size exceeds
# the default. Measured minimum: 1,496,386 dispatches, identical in both
# modes; the allowance is 4x that (the factor the Temporal table fixtures
# use), applies to this exact path only, and the ordinary two-second wall
# deadline still bounds it (about 0.7-0.9 s at the measured cost).
STRING_SUBSTR_NUMBER_MATRIX_FIXTURES = frozenset(
    {"annexB/built-ins/String/prototype/substr/start-and-length-as-numbers.js"}
)
STRING_SUBSTR_NUMBER_MATRIX_INSTRUCTION_BUDGET = 6_000_000
# Fixed, finite staging fixtures whose size only just exceeds the default: each
# is straight-line or a bounded loop (a few hundred iterations, or a run of
# assertions that build their failure message eagerly), and each finishes in a
# small fraction of a second. Every entry is an exact path with an allowance of
# 4x its measured minimum, which is identical in sloppy and strict mode; the
# ordinary two-second wall deadline still bounds them.
#   with-dense.js                          178,125  (63 receivers x 13 indices)
#   parse-reviver-array-delete.js          185,937  (about 4,100 reviver calls)
#   log2-approx.js                         325,000  (2,097 assertNear checks)
#   es5ish-defineGetter-defineSetter.js    110,156  (about 60 descriptor checks)
# A fixture that is too slow for that deadline even with fuel is deliberately
# not listed here: it needs a wall-deadline allowance too, which only
# `LARGE_FIXTURE_RESOURCES` below can express (staging/sm/Array/toSpliced-dense.js
# needs 19.6M dispatches and about 7 s; each
# staging/sm/Date/dst-offset-caching-N-of-8.js part needs 100M-130M dispatches
# and runs for more than half a minute).
FINITE_FIXTURE_INSTRUCTION_BUDGETS = {
    "staging/sm/Array/with-dense.js": 750_000,
    "staging/sm/JSON/parse-reviver-array-delete.js": 750_000,
    "staging/sm/Math/log2-approx.js": 1_300_000,
    "staging/sm/extensions/es5ish-defineGetter-defineSetter.js": 450_000,
}
# `staging/sm/String/unicode-braced.js` evaluates a source string built from
# 2**24 zeros, which is 32 MiB of UTF-16 by itself: it needs a string limit of at
# least 33,558,528 bytes (33,554,432 is not enough) against the ordinary 1 MiB.
# The remainder is a few dozen assertions and it runs in about a second with the
# default dispatch budget and heap. The limit is 64 MiB, twice the requirement
# (a data size, so more headroom would buy nothing), for this exact path only.
FIXTURE_STRING_LIMITS = {
    "staging/sm/String/unicode-braced.js": 64 * 1024 * 1024,
}


def fixture_string_limit(relative):
    """Return the exact-path string limit for a fixture, or None for the default."""
    return FIXTURE_STRING_LIMITS.get(relative)


TYPED_ARRAY_DETACH_COERCION_INSTRUCTION_BUDGET = 50_000_000
# `testIntl.js` runs every asserted result through a finite locale and
# numbering-system matrix. Debug interpreter dispatch exceeds the ordinary
# two-second process deadline, so grant that upstream harness a bounded wall
# allowance without weakening unrelated Intl or ECMAScript tests.
INTL_MATRIX_HARNESS = "testIntl.js"
INTL_MATRIX_HARNESS_TIMEOUT = 30
# This upstream `testIntl.js` fixture runs every Intl constructor through two
# matching strategies and three Unicode-extension spellings for each locale.
# It is finite, but a debug VM worker takes about 75 seconds per strictness
# mode on a cold process; retain a fixture-specific envelope rather than
# widening every finite stress test.
SUPPORTED_LOCALES_UNICODE_EXTENSION_FIXTURE = (
    "intl402/supportedLocalesOf-unicode-extensions-ignored.js"
)
SUPPORTED_LOCALES_UNICODE_EXTENSION_TIMEOUT = 180
# ResizableArrayBuffer helper fixtures exercise the same operation across
# fixed, offset, and length-tracking views for every numeric element type.
# Keep their larger but finite allowance feature-scoped.
RESIZABLE_ARRAY_BUFFER_FEATURE = "resizable-arraybuffer"
RESIZABLE_ARRAY_BUFFER_TIMEOUT = 60
RESIZABLE_ARRAY_BUFFER_INSTRUCTION_BUDGET = 10_000_000
# Agent fixtures deliberately synchronize with a spin loop while a separate
# host VM reaches receiveBroadcast. They are bounded by the outer per-case
# deadline, but need more interpreter fuel than an ordinary one-turn script.
# Agent tests must start independent VM threads, exchange a broadcast, and
# coordinate at least one host wait. Under a parallel inventory run that work
# competes with three other workers, so keep a bounded allowance high enough
# for the standard `timeouts.huge` and FIFO suites.
TEST262_AGENT_INSTRUCTION_BUDGET = 50_000_000
TEST262_AGENT_TIMEOUT = 120
STABLE_ARRAY_SORT_INSTRUCTION_BUDGET = 10_000_000
STABLE_ARRAY_SORT_TIMEOUT = 30
# Unicode-property conformance fixtures intentionally materialize every
# scalar value (often twice, for a property and its complement) before one
# anchored RegExp match.  That is finite standard-harness work, but larger
# than the general script fuel/string limits.
REGEXP_PROPERTY_ESCAPES_FEATURE = "regexp-unicode-property-escapes"
REGEXP_PROPERTY_ESCAPES_INSTRUCTION_BUDGET = 30_000_000
REGEXP_PROPERTY_ESCAPES_TIMEOUT = 60
REGEXP_PROPERTY_ESCAPES_STRING_LIMIT = 8 * 1024 * 1024
REGEXP_PROPERTY_ESCAPES_REGEX_TIMEOUT_MS = 5_000
# Four imported Sputnik fixtures exhaustively enumerate valid three- and
# four-octet UTF-8 sequences. Their assertions are delegated to a native
# Test262 helper that still calls the supplied Decode global for every value;
# this avoids spending minutes dispatching JavaScript bookkeeping around each
# of the roughly one million independent Decode checks.
URI_DECODE_EXHAUSTIVE_FIXTURES = {
    "built-ins/decodeURI/S15.1.3.1_A2.4_T1.js": ("decodeURI", 3),
    "built-ins/decodeURI/S15.1.3.1_A2.5_T1.js": ("decodeURI", 4),
    "built-ins/decodeURIComponent/S15.1.3.2_A2.4_T1.js": ("decodeURIComponent", 3),
    "built-ins/decodeURIComponent/S15.1.3.2_A2.5_T1.js": ("decodeURIComponent", 4),
}
URI_ENCODE_EXHAUSTIVE_FIXTURES = {
    "built-ins/encodeURI/S15.1.3.3_A2.3_T1.js": ("encodeURI", 0x0800, 0xd7ff),
    "built-ins/encodeURI/S15.1.3.3_A2.5_T1.js": ("encodeURI", 0xe000, 0xffff),
    "built-ins/encodeURIComponent/S15.1.3.4_A2.3_T1.js": ("encodeURIComponent", 0x0800, 0xd7ff),
    "built-ins/encodeURIComponent/S15.1.3.4_A2.5_T1.js": ("encodeURIComponent", 0xe000, 0xffff),
}
URI_EXHAUSTIVE_FIXTURES = URI_DECODE_EXHAUSTIVE_FIXTURES | URI_ENCODE_EXHAUSTIVE_FIXTURES
URI_EXHAUSTIVE_INSTRUCTION_BUDGET = 100_000_000
URI_EXHAUSTIVE_TIMEOUT = 15
URI_GLOBAL_DIRECTORIES = frozenset(
    {
        "built-ins/decodeURI",
        "built-ins/decodeURIComponent",
        "built-ins/encodeURI",
        "built-ins/encodeURIComponent",
    }
)
URI_GLOBAL_INSTRUCTION_BUDGET = 10_000_000
URI_GLOBAL_TIMEOUT = 30
# These generated fixtures build a finite large string and first test each
# RegExp against it. On a mismatch, their JavaScript diagnostic loop then
# repeats the failed test for every code point merely to format an error
# message. The adapter retains the original full-string RegExp operation and
# fails immediately on that same mismatch.
REGEXP_CLASS_ESCAPE_FIXTURES = frozenset(
    {
        "built-ins/RegExp/CharacterClassEscapes/character-class-digit-class-escape-negative-cases.js",
        "built-ins/RegExp/CharacterClassEscapes/character-class-digit-class-escape-positive-cases.js",
        "built-ins/RegExp/CharacterClassEscapes/character-class-non-digit-class-escape-negative-cases.js",
        "built-ins/RegExp/CharacterClassEscapes/character-class-non-digit-class-escape-positive-cases.js",
        "built-ins/RegExp/CharacterClassEscapes/character-class-non-whitespace-class-escape-negative-cases.js",
        "built-ins/RegExp/CharacterClassEscapes/character-class-non-whitespace-class-escape-positive-cases.js",
        "built-ins/RegExp/CharacterClassEscapes/character-class-non-word-class-escape-negative-cases.js",
        "built-ins/RegExp/CharacterClassEscapes/character-class-non-word-class-escape-positive-cases.js",
        "built-ins/RegExp/CharacterClassEscapes/character-class-whitespace-class-escape-negative-cases.js",
        "built-ins/RegExp/CharacterClassEscapes/character-class-whitespace-class-escape-positive-cases.js",
        "built-ins/RegExp/CharacterClassEscapes/character-class-word-class-escape-negative-cases.js",
        "built-ins/RegExp/CharacterClassEscapes/character-class-word-class-escape-positive-cases.js",
    }
)
REGEXP_CLASS_ESCAPE_STRING_LIMIT = 8 * 1024 * 1024
# SpiderMonkey's Unicode case-mapping regression fixture materializes the
# complete Unicode mapping table.  The work is finite and already has the
# exact-fixture time/fuel allowance below, but its table cannot coexist with
# the normal 16 MiB conformance VM heap.
STRING_CASE_MAPPING_FIXTURE = "staging/sm/String/string-upper-lower-mapping.js"
STRING_CASE_MAPPING_HEAP_LIMIT = 256 * 1024 * 1024
TYPED_ARRAY_OVERLAP_FIXTURE = "staging/sm/TypedArray/set-same-buffer-different-source-target-types.js"
NULLISH_JIT_STRESS_FIXTURE = "staging/sm/expressions/nullish-coalescing.js"
SHORT_CIRCUIT_JIT_STRESS_FIXTURE = "staging/sm/expressions/short-circuit-compound-assignment.js"
# These immutable conformance files intentionally perform large but finite
# interpreter-visible loops (typically every BMP code point, thousands of
# TypedArray elements, or a JIT stress count). Keep their allowance scoped to
# the exact fixtures so an accidental loop in ordinary test code remains
# bounded by the default policy.
FINITE_STRESS_FIXTURES = frozenset(
    {
        "annexB/built-ins/RegExp/RegExp-leading-escape-BMP.js",
        "annexB/built-ins/RegExp/RegExp-trailing-escape-BMP.js",
        "built-ins/Array/prototype/concat/Array.prototype.concat_large-typed-array.js",
        # Its argument-coercion matrix (every start/end coercion outcome
        # against every buffer state) is finite but far above the default fuel.
        "built-ins/ArrayBuffer/prototype/sliceToImmutable/argument-coercion.js",
        # This walks the complete reachable graph of well-known intrinsics
        # and checks each built-in function's NativeFunction source form.
        "built-ins/Function/prototype/toString/built-in-function-object.js",
        "built-ins/RegExp/character-class-escape-non-whitespace.js",
        "built-ins/String/prototype/repeat/repeat-string-n-times.js",
        "built-ins/parseFloat/S15.1.2.3_A6.js",
        "built-ins/parseInt/S15.1.2.2_A7.2_T1.js",
        "built-ins/parseInt/S15.1.2.2_A7.3_T1.js",
        "built-ins/parseInt/S15.1.2.2_A8.js",
        # These fixtures perform finite but interpreter-heavy validation: five
        # iterate locale-tag data through the Test262 Intl helper, three drive
        # multi-module top-level-await graphs, and the sparse-array test scans
        # several thousand holes. Keep their resource envelope explicit.
        "intl402/Intl/getCanonicalLocales/canonicalized-tags.js",
        "intl402/Intl/getCanonicalLocales/complex-region-subtag-replacement.js",
        "intl402/Intl/getCanonicalLocales/transformed-ext-valid.js",
        "intl402/Intl/getCanonicalLocales/unicode-ext-canonicalize-yes-to-true.js",
        # This verifies every valid combination of language, script, region
        # and variants. Its nested finite loops exceed the normal VM fuel.
        "intl402/DisplayNames/prototype/of/type-language-valid.js",
        # The NumberFormat unit constructor matrix validates every sanctioned
        # simple unit and every ordered simple-unit-per-simple-unit compound.
        # It is finite (roughly four thousand constructions) but exceeds the
        # normal debug-interpreter fuel envelope.
        "intl402/NumberFormat/constructor-unit.js",
        # NumberFormat's fraction/significant-digit matrices and the unit
        # matrix execute finite, interpreter-heavy assertion loops (the unit
        # file checks every ordered sanctioned compound pair). Their scoped
        # envelope preserves the default guard for ordinary conformance code.
        "intl402/NumberFormat/prototype/format/format-fraction-digits.js",
        "intl402/NumberFormat/prototype/format/format-significant-digits.js",
        "intl402/NumberFormat/prototype/format/units.js",
        # This cross-product checks three rounding priorities, fourteen
        # mixed precision records, five locales and four digit systems.
        "intl402/NumberFormat/test-option-roundingPriority-mixed-options.js",
        # AvailableCurrencies is checked against every three-letter ASCII
        # code (26^3 candidates) through Intl.DisplayNames. It is a finite
        # registry-closure audit, not an unbounded enumeration.
        "intl402/Intl/supportedValuesOf/currencies-accepted-by-DisplayNames.js",
        "intl402/language-tags-canonicalized.js",
        "language/comments/S7.4_A5.js",
        "language/comments/S7.4_A6.js",
        "language/literals/regexp/S7.8.5_A1.1_T2.js",
        "language/literals/regexp/S7.8.5_A1.4_T2.js",
        "language/literals/regexp/S7.8.5_A2.1_T2.js",
        "language/literals/regexp/S7.8.5_A2.4_T2.js",
        "language/module-code/top-level-await/fulfillment-order.js",
        "language/module-code/top-level-await/rejection-order.js",
        "language/module-code/top-level-await/unobservable-global-async-evaluation-count-reset.js",
        "staging/sm/Array/sort_holes.js",
        # These legacy staging fixtures have fixed, small workloads, but use
        # deep helper recursion or TypedArray dispatch that can exceed the
        # ordinary wall deadline when every runner worker is cold or busy.
        "staging/sm/Reflect/propertyKeys.js",
        "staging/sm/TypedArray/filter-species.js",
        "staging/sm/TypedArray/map-species.js",
        "staging/sm/TypedArray/sort_snans.js",
        "staging/sm/generators/delegating-yield-9.js",
        "staging/sm/object/entries.js",
        "staging/sm/Function/has-instance-jitted.js",
        "staging/sm/Function/function-toString-builtin.js",
        "staging/sm/Proxy/ownkeys-linear.js",
        # Generated Unicode 16 case-folding coverage checks every recorded
        # equivalence class through both literal and character-class regexps.
        "staging/sm/RegExp/unicode-ignoreCase.js",
        "staging/sm/String/fromCodePoint.js",
        "staging/sm/String/string-pad-start-end.js",
        STRING_CASE_MAPPING_FIXTURE,
        "staging/sm/TypedArray/set-same-buffer-different-source-target-types.js",
        # These two fixtures execute a finite O(n log n) sequence of
        # user-visible TypedArray comparisons across many lengths. They are
        # conformance checks, not unbounded stress loops.
        "staging/sm/TypedArray/sort_modifications.js",
        "staging/sm/TypedArray/sort_sorted.js",
        "staging/sm/class/newTargetEval.js",
        "staging/sm/expressions/nullish-coalescing.js",
        "staging/sm/expressions/object-literal-__proto__.js",
        "staging/sm/expressions/short-circuit-compound-assignment.js",
        "staging/sm/generators/iteration.js",
        "staging/sm/misc/getter-setter-outerize-this.js",
        "harness/nativeFunctionMatcher.js",
        "intl402/Intl/getCanonicalLocales/invalid-tags.js",
        "intl402/Intl/getCanonicalLocales/preferred-grandfathered.js",
        "intl402/Intl/getCanonicalLocales/transformed-ext-invalid.js",
        "intl402/Locale/invalid-tag-throws.js",
        "intl402/fallback-locales-are-supported.js",
        "intl402/language-tags-invalid.js",
        # These Segmenter conformance fixtures traverse every UTF-16 index of
        # multilingual paragraph samples (and one additionally materializes
        # every grapheme record). They are finite specification checks, but
        # exceed the ordinary interpreter fuel budget.
        "intl402/Segmenter/prototype/segment/containing/iswordlike.js",
        "intl402/Segmenter/prototype/segment/containing/word-iswordlike.js",
        "intl402/Segmenter/prototype/segment/segment-grapheme-iterable.js",
        "intl402/supportedLocalesOf-consistent-with-resolvedOptions.js",
        "intl402/supportedLocalesOf-unicode-extensions-ignored.js",
        # Compares every pair of the ~446 primary time zone identifiers
        # (about 99,000 pairs of two constructions, `withTimeZone` and
        # `equals`). Finite, and each pair is cheap, but the pair count is far
        # past the ordinary fuel budget and wall deadline.
        "intl402/Temporal/ZonedDateTime/prototype/equals/canonical-not-equal.js",
    }
)
FINITE_STRESS_INSTRUCTION_BUDGET = 10_000_000
FINITE_STRESS_TIMEOUT = 90
# NumberFormat's mixed-precision fixture is a fixed 3 × 14 × 5 × 4 × 6
# matrix. Its runner adapter retains every formatter construction/output
# assertion through the VM bridge while removing the helper's repetitive
# interpreter dispatch, so it deliberately uses the ordinary per-mode limits.
NUMBER_FORMAT_NATIVE_PRECISION_MATRIX_FIXTURE = (
    "intl402/NumberFormat/test-option-roundingPriority-mixed-options.js"
)
# These DateTimeFormat fixtures form each supported calendar across one
# hundred years.  They are finite conformance matrices, but a debug
# interpreter performs a non-ISO calendar conversion and a format-to-parts
# call for every cell. Keep their larger envelope attached to these exact
# upstream files rather than weakening the normal Temporal or Intl policy.
TEMPORAL_CALENDAR_MATRIX_FIXTURES = frozenset(
    {
        "intl402/DateTimeFormat/prototype/formatToParts/compare-to-temporal.js",
        "intl402/DateTimeFormat/prototype/formatToParts/compare-to-temporal-lunisolar.js",
    }
)
TEMPORAL_CALENDAR_MATRIX_INSTRUCTION_BUDGET = 10_000_000
# Test262 (and ECMA-262) define no instruction budget: it is this host's own
# resource policy, so a finite fixture that needs more dispatches than the
# default is granted an explicit, bounded allowance -- its source is never
# edited and it still has to pass, and an unbounded loop still exhausts it.
#
# ZonedDateTime's own since/until same-epoch-nanoseconds fixtures enumerate
# every combination of 4 time zones x 3 epoch-nanosecond values x 55
# largestUnit/smallestUnit pairs (660 total `since`/`until` calls, each
# followed by a 10-field `TemporalHelpers.assertDuration` comparison) to
# confirm a blank duration at every granularity when the two instants are
# already equal. A finite, bounded conformance matrix, not an unbounded loop
# or an algorithmic-complexity bug in the difference computation itself
# (confirmed directly: a single such call resolves immediately via
# `DifferenceTemporalZonedDateTime` step 8's own equal-epoch-nanoseconds fast
# path) -- the interpreter's own per-call/per-property-access dispatch cost,
# multiplied across 660 iterations, is what needs the larger envelope.
# Measured minimum: 300,000 dispatches; the allowance is ~3x that.
ZONED_DATE_TIME_SAME_EPOCH_MATRIX_FIXTURES = frozenset(
    {
        "built-ins/Temporal/ZonedDateTime/prototype/since/same-epoch-nanoseconds.js",
        "built-ins/Temporal/ZonedDateTime/prototype/until/same-epoch-nanoseconds.js",
    }
)
ZONED_DATE_TIME_SAME_EPOCH_MATRIX_INSTRUCTION_BUDGET = 1_000_000
# Seven intl402 fixtures walk a fixed calendar table through the real
# `Temporal.*.from` path: hebrew-keviah.js visits 2,101 Hebrew years (two
# `PlainDate.from` calls plus a symbol lookup each), persian-new-year-dates.js
# checks 293 Nowruz dates, the two `roundtrip-from-property-bag.js` fixtures
# run one `from` + a dozen property assertions per row of a 42-row calendar
# table, and the three `dayOfYear/non-iso-calendar-basic.js` fixtures step
# through every day of one year in each of 15 calendars (about 5,500 dates,
# each a `year` read, a `dayOfYear` read, an assertion and an `add`). Each is
# finite and its per-row cost is one ordinary call chain; only the row count
# exceeds the default. Measured minimums are 500,000 (hebrew-keviah), 240,000
# (the dayOfYear walks) and 200,000 (the other two); the allowance is 4x the
# largest.
TEMPORAL_CALENDAR_TABLE_FIXTURES = frozenset(
    {
        "intl402/Temporal/PlainDate/from/hebrew-keviah.js",
        "intl402/Temporal/PlainDate/from/persian-new-year-dates.js",
        "intl402/Temporal/PlainDateTime/from/roundtrip-from-property-bag.js",
        "intl402/Temporal/ZonedDateTime/from/roundtrip-from-property-bag.js",
        "intl402/Temporal/PlainDate/prototype/dayOfYear/non-iso-calendar-basic.js",
        "intl402/Temporal/PlainDateTime/prototype/dayOfYear/non-iso-calendar-basic.js",
        "intl402/Temporal/ZonedDateTime/prototype/dayOfYear/non-iso-calendar-basic.js",
    }
)
TEMPORAL_CALENDAR_TABLE_INSTRUCTION_BUDGET = 2_000_000
# `ZonedDateTime.from/timezone-case-insensitive.js` builds
# `[...new Set([...timeZoneIdentifiers, ...Intl.supportedValuesOf('timeZone')])]`
# (about 600 identifiers) and calls `Temporal.ZonedDateTime.from` three times per
# identifier (as spelled, lower- and upper-case): finite, one ordinary call chain
# per row. Until `Set` iteration was implemented that spread was empty, the loop
# never ran and the fixture passed vacuously; it now does real work. Measured
# minimum: between 100,000 (the default, which is not enough) and 150,000
# dispatches; the allowance is ~3x the upper bound.
TEMPORAL_TIME_ZONE_ID_TABLE_FIXTURES = frozenset(
    {
        "intl402/Temporal/ZonedDateTime/from/timezone-case-insensitive.js",
    }
)
TEMPORAL_TIME_ZONE_ID_TABLE_INSTRUCTION_BUDGET = 500_000
# ZonedDateTime/links.js walks a fixed table of about 120 IANA link names,
# building two ZonedDateTimes per row and comparing `offsetNanoseconds` at ten
# epochs for each. It is finite, and its per-row cost is one ordinary call
# chain; only the row count exceeds the default. Measured minimum is between
# 110,000 and 125,000; the allowance is 4x the upper bound.
TEMPORAL_TIME_ZONE_LINK_TABLE_FIXTURES = frozenset(
    {
        "intl402/Temporal/ZonedDateTime/links.js",
    }
)
TEMPORAL_TIME_ZONE_LINK_TABLE_INSTRUCTION_BUDGET = 500_000
TEMPORAL_CALENDAR_MATRIX_TIMEOUT = 360
# The six upstream Iterator.zip/zipKeyed basic fixtures enumerate every prefix
# combination through three inputs, then verify descriptor details for every
# yielded row. They are finite conformance matrices, not an unbounded iterator
# probe; keep their larger envelope exact and leave ordinary iterator cases at
# the default budget.
ITERATOR_ZIP_BASIC_MATRIX_FIXTURES = frozenset(
    {
        "built-ins/Iterator/zip/basic-shortest.js",
        "built-ins/Iterator/zip/basic-longest.js",
        "built-ins/Iterator/zip/basic-strict.js",
        "built-ins/Iterator/zipKeyed/basic-shortest.js",
        "built-ins/Iterator/zipKeyed/basic-longest.js",
        "built-ins/Iterator/zipKeyed/basic-strict.js",
    }
)
ITERATOR_ZIP_BASIC_MATRIX_INSTRUCTION_BUDGET = 10_000_000
ITERATOR_ZIP_BASIC_MATRIX_TIMEOUT = 15
# These six historical RegExp BMP enumerations parse or execute one pattern
# for every UTF-16 code unit. They compete for the isolated matcher processes
# during a parallel inventory run, so their measured per-mode bound is higher
# than the general finite stress allowance.
BMP_REGEXP_ENUMERATION_FIXTURES = frozenset(
    {
        "annexB/built-ins/RegExp/RegExp-leading-escape-BMP.js",
        "annexB/built-ins/RegExp/RegExp-trailing-escape-BMP.js",
        "built-ins/RegExp/character-class-escape-non-whitespace.js",
        "language/literals/regexp/S7.8.5_A1.1_T2.js",
        "language/literals/regexp/S7.8.5_A1.4_T2.js",
        "language/literals/regexp/S7.8.5_A2.1_T2.js",
        "language/literals/regexp/S7.8.5_A2.4_T2.js",
    }
)
BMP_REGEXP_ENUMERATION_TIMEOUT = 30
REGEXP_BMP_LITERAL_FIXTURES = {
    "annexB/built-ins/RegExp/RegExp-leading-escape-BMP.js": 1,
    "annexB/built-ins/RegExp/RegExp-trailing-escape-BMP.js": 3,
    "language/literals/regexp/S7.8.5_A1.1_T2.js": 0,
    "language/literals/regexp/S7.8.5_A1.4_T2.js": 1,
    "language/literals/regexp/S7.8.5_A2.1_T2.js": 2,
    "language/literals/regexp/S7.8.5_A2.4_T2.js": 3,
}
REGEXP_NON_WHITESPACE_BMP_FIXTURE = "built-ins/RegExp/character-class-escape-non-whitespace.js"

# ECMA-262's Agent Record [[CanBlock]] field is host-defined (AgentCanSuspend
# returns it verbatim; the spec's own note on it gives a browser's main
# thread as one example of a host that might reasonably choose false, not a
# universal rule). Test262 tags each Atomics.wait-suspend fixture with the
# host capability it assumes (CanBlockIsTrue / CanBlockIsFalse) rather than
# asserting both are achievable by the same host at once, and INTERPRETING.md
# says a fixture "should only be run" against a host whose own [[CanBlock]]
# matches. BlueJS's Atomics.wait genuinely suspends the agent (see
# atomics_wait/atomics_wait_status in
# backend/bluejs/src/vm/builtins/binary_data.rs), and every CanBlockIsTrue
# fixture already passes against that behavior, so this host's own
# [[CanBlock]] is true. A CanBlockIsFalse fixture is therefore inapplicable
# to this host, not an engine gap: making it pass would mean making
# Atomics.wait always throw, which would break every already-passing
# CanBlockIsTrue fixture. This is a host capability declaration, distinct
# from a capability BlueJS actually lacks, so it gets its own "excluded"
# outcome instead of being folded into "unsupported".
HOST_CAN_BLOCK = True
CANBLOCK_FLAG_REQUIREMENT = {"CanBlockIsTrue": True, "CanBlockIsFalse": False}


def canblock_exclusion(flags):
    """The reason a CanBlockIsTrue/CanBlockIsFalse-flagged fixture does not
    apply to this host's declared HOST_CAN_BLOCK, or None if it does (or
    carries neither flag)."""
    for flag, required in CANBLOCK_FLAG_REQUIREMENT.items():
        if flag in flags and required != HOST_CAN_BLOCK:
            return f"host declares [[CanBlock]] = {str(HOST_CAN_BLOCK).lower()}, fixture requires {flag}"
    return None


# A fixture whose own assertions contradict the *current* ECMA-262 draft --
# verified by reading the live spec text, not inferred from disagreeing with
# BlueJS -- and which upstream Test262 has already independently identified
# and drafted a fix for, is a different thing from an engine gap ("fail") or
# a capability BlueJS lacks ("unsupported"): the corpus itself hasn't caught
# up yet. "stale_corpus" says exactly that -- this host already matches the
# current draft (like every other engine that implements it), and the test
# will presumably start passing on its own once the upstream fix lands,
# without any BlueJS change. Keyed by exact path only, each entry documents
# the live spec citation and the upstream issue/PR, never "we disagree with
# this test" alone.
STALE_CORPUS_FIXTURES = {
    # Verified 2026-09-23 against the live ECMA-262 draft's
    # `FunctionDeclarationInstantiation` Annex B web-compat insertion point
    # (https://tc39.es/ecma262/multipage/ordinary-and-exotic-objects-behaviours.html#step-functiondeclarationinstantiation-web-compat-insertion-point):
    # the `funcName is not "arguments"` guard there gates only *creating a
    # new* var binding; the sibling step that runs when the block function
    # is evaluated (`funcEnv.SetMutableBinding(funcName, funcObj, false)`)
    # has no such guard and still overwrites an existing `arguments`
    # binding. This fixture (2017) was written against an older edition
    # that appended `"arguments"` to `parameterNames` itself, a step the
    # current algorithm no longer has -- V8 and SpiderMonkey both match the
    # current text, as does BlueJS. tc39/test262#5113 documents this exact
    # contradiction (it conflicts with
    # staging/sm/lexical-environment/block-scoped-functions-annex-b-arguments.js,
    # which is NOT in this table -- that fixture already matches the
    # current draft); tc39/test262#5112 is the open, unmerged fix.
    "annexB/language/function-code/block-decl-func-skip-arguments.js": (
        "contradicts the current FunctionDeclarationInstantiation Annex B "
        "web-compat insertion point (verified against the live spec text); "
        "see tc39/test262#5113, fix pending in tc39/test262#5112"
    ),
}


def stale_corpus_reason(relative):
    """The reason `relative` is a known-stale corpus fixture (see
    STALE_CORPUS_FIXTURES), or None for every other path."""
    return STALE_CORPUS_FIXTURES.get(relative)


def fetch(destination):
    if destination.exists():
        raise ValueError(f"refusing to replace existing corpus: {destination}")
    revision = SNAPSHOT["revision"]
    url = f"https://codeload.github.com/tc39/test262/tar.gz/{revision}"
    with urllib.request.urlopen(url, timeout=60) as response:
        archive = response.read()
    if hashlib.sha256(archive).hexdigest() != SNAPSHOT["archive_sha256"]:
        raise ValueError("Test262 archive checksum mismatch")
    destination.mkdir(parents=True)
    manifest = {}
    with tarfile.open(fileobj=io.BytesIO(archive), mode="r:gz") as tar:
        for member in tar:
            relative = Path(*Path(member.name).parts[1:])
            if relative.is_absolute() or ".." in relative.parts or member.issym() or member.islnk():
                raise ValueError("unsafe archive entry")
            target = destination / relative
            if member.isdir():
                target.mkdir(parents=True, exist_ok=True)
            elif member.isfile():
                target.parent.mkdir(parents=True, exist_ok=True)
                contents = tar.extractfile(member).read()
                target.write_bytes(contents)
                manifest[relative.as_posix()] = hashlib.sha256(contents).hexdigest()
    manifest_bytes = (json.dumps(manifest, sort_keys=True) + "\n").encode()
    if hashlib.sha256(manifest_bytes).hexdigest() != SNAPSHOT["manifest_sha256"]:
        raise ValueError("archive file manifest mismatch")
    (destination / ".bluejs-manifest.json").write_bytes(manifest_bytes)
    (destination / ".bluejs-snapshot.json").write_text(json.dumps(SNAPSHOT, indent=2) + "\n")


def metadata(source):
    match = FRONTMATTER.search(source)
    if not match:
        raise ValueError("missing Test262 frontmatter")
    data = yaml.safe_load(match.group(1))
    if not isinstance(data, dict):
        raise ValueError("frontmatter is not a mapping")
    for field in ("flags", "features", "includes"):
        items = data.get(field, [])
        if not isinstance(items, list) or not all(isinstance(item, str) for item in items):
            raise ValueError(f"invalid {field} metadata")
    flags = set(data.get("flags", []))
    known = {"onlyStrict", "noStrict", "module", "raw", "async", "generated", "CanBlockIsTrue", "CanBlockIsFalse", "non-deterministic"}
    if flags - known:
        raise ValueError(f"unknown flags: {sorted(flags - known)}")
    if {"onlyStrict", "noStrict"} <= flags or {"raw", "onlyStrict"} <= flags:
        raise ValueError("conflicting execution mode flags")
    negative = data.get("negative")
    if negative is not None and (not isinstance(negative, dict) or negative.get("phase") not in {"parse", "resolution", "runtime"} or not isinstance(negative.get("type"), str)):
        raise ValueError("invalid negative metadata")
    return data


def modes(data):
    flags = data.get("flags", [])
    for flag, mode in [("module", "module"), ("raw", "raw"), ("onlyStrict", "strict"), ("noStrict", "sloppy")]:
        if flag in flags:
            return [mode]
    return ["sloppy", "strict"]


class ModuleSources(collections.namedtuple(
    "ModuleSources",
    "sources dynamic_sources json_sources text_sources bytes_sources",
)):
    """The resources one test's module graph needs, split by how the host loads them.

    `sources` and `dynamic_sources` are JavaScript module text (eagerly linked
    versus compiled lazily); `json_sources` and `text_sources` are decoded
    text; `bytes_sources` maps a path to its raw bytes as a list of ints (the
    adapter's JSON transport).
    """

    __slots__ = ()


# Resource kinds a module request can load. `js` is a Source Text Module; the
# others are the synthetic modules selected by `with { type }`.
SYNTHETIC_MODULE_TYPES = ("json", "text", "bytes")


def utf8_decode(data):
    """WHATWG "UTF-8 decode": strip one leading BOM, replace malformed bytes."""
    return data.decode("utf-8-sig", errors="replace")


def request_kind(candidate, attribute_type):
    """Which loader a request for `candidate` selects.

    An explicit `type` attribute always wins. Without one, a `.json` fixture
    keeps its historical JSON classification (an untyped reference through a
    variable specifier may still be an import that carries the attribute at
    runtime); every other suffix-less/odd fixture is left to the adapter's
    normal module-resolution result.
    """
    if attribute_type in SYNTHETIC_MODULE_TYPES:
        return attribute_type
    if candidate.suffix == ".json":
        return "json"
    return "js"


def module_sources(
    entry,
    test_root,
    include_dynamic_string_roots=False,
    speculative_relative_strings=False,
):
    """Collect static imports and, when needed, relative dynamic-import roots.

    A dynamic import with a variable specifier cannot be resolved statically.
    Test262 fixtures conventionally retain its relative candidate strings in
    the test source, so a caller may opt into supplying existing sibling
    files without making unrelated ordinary module tests over-inclusive.

    Returns a `ModuleSources`: `(sources, dynamic_sources, json_sources,
    text_sources, bytes_sources)`. `.js` fixtures
    reached by at least one static edge (an `import`/`export ... from`, from
    `entry` or transitively) are parser input for the adapter's own eagerly
    linked JavaScript module graph, in `sources`. A `.js` fixture reached
    only through a dynamic edge (a literal `import(...)` or, with
    `include_dynamic_string_roots` and `speculative_relative_strings=True`,
    any other relative-looking string) goes to `dynamic_sources` instead:
    raw text the adapter must **not** eagerly parse/compile, since a module
    that is a syntax/semantic error only *as a module* (perfectly valid
    otherwise) must fail lazily, as that dynamic import's own promise
    rejection, not as an eager whole-run failure before any code has even
    run. A module reached by *both* kinds of edge (from different call
    sites) counts as static: eager compilation is the correct, safe choice
    whenever a module is genuinely required by the static graph regardless
    of also being separately dynamically imported.

    With `include_dynamic_string_roots` but `speculative_relative_strings`
    left false (the default), a relative-looking string root counts as
    *static* instead -- preserving the original, established behavior for a
    plain dynamic `import()` with a variable specifier (where such a
    candidate's own parse failure must still hard-fail eagerly, exactly as
    it always has). The string token that *is* a literal `import('...')`
    argument is not such a candidate: it stays the dynamic edge it already
    is, so a fixture that is only invalid as a module rejects that import's
    promise instead of failing the whole run. Pass `speculative_relative_strings=True` only when a
    relative-string candidate may be a deliberately invalid, never-actually-
    imported fixture (e.g. reached via `ShadowRealm.prototype.importValue`'s
    own heuristic trigger), so the adapter's lazy per-import rejection
    applies instead of an eager hard failure.

    A request's `with { type: "json" | "text" | "bytes" }` attribute (on a
    static declaration or a literal dynamic `import()`) selects the adapter's
    separate synthetic-module path for its target, which is raw data -- never
    a parser input nor traversed for further requests -- and is keyed by the
    resource path *and* type, so one file can be both a JavaScript module and
    (say) its own text (`import-attributes/text-self.js`). `json` and `text`
    resources are decoded as UTF-8 text (`text` per WHATWG "UTF-8 decode");
    `bytes` resources are kept as raw byte lists. An untyped request for a
    `.json` fixture stays a JSON resource. Other fixture kinds reached with no
    such attribute (Wasm, binary text) are left to the adapter's normal
    module-resolution result, per this function's original scope.
    """
    test_root = test_root.resolve()
    text = {}
    raw = {}
    discovery = {}
    resources = {kind: set() for kind in SYNTHETIC_MODULE_TYPES}
    pending = [(entry.resolve(), "js", "static")]
    while pending:
        path, kind, reason = pending.pop()
        relative = path.relative_to(test_root).as_posix()
        if kind != "js":
            resources[kind].add(relative)
            if relative not in raw:
                raw[relative] = path.read_bytes()
            continue
        previous = discovery.get(relative)
        if previous == "static" or previous == reason:
            continue
        discovery[relative] = "static" if reason == "static" else previous or reason
        if relative not in text:
            text[relative] = path.read_text(encoding="utf-8")
        source = text[relative]
        requests = [
            (match.group(1) or match.group(3), match.group(2) or match.group(4), "static")
            for match in MODULE_REQUEST.finditer(source)
        ]
        requests.extend(
            (match.group(1), match.group(2), "dynamic")
            for match in DYNAMIC_IMPORT_PLAIN_REQUEST.finditer(source)
        )
        requests.extend(
            (match.group(1), None, "static")
            for match in DYNAMIC_IMPORT_SOURCE_OR_DEFER_REQUEST.finditer(source)
        )
        if include_dynamic_string_roots:
            # The specifier of a literal `import('...')` is already a dynamic
            # edge above; its own string token must not be re-read as a
            # relative-string candidate, or that (statically classified)
            # candidate would silently override the edge and force a fixture
            # that is invalid only *as a module* to fail eagerly.
            literal_dynamic_spans = {
                match.span(1) for match in DYNAMIC_IMPORT_PLAIN_REQUEST.finditer(source)
            }
            requests.extend(
                (
                    match.group(1),
                    None,
                    "dynamic" if speculative_relative_strings else "static",
                )
                for match in RELATIVE_STRING.finditer(source)
                if match.span(1) not in literal_dynamic_spans
            )
        for request, attribute_type, sub_reason in requests:
            if not request or not request.startswith("."):
                continue
            candidate = (path.parent / request).resolve()
            try:
                candidate.relative_to(test_root)
            except ValueError:
                continue
            candidate_kind = request_kind(candidate, attribute_type)
            # Other import-attribute-named fixture kinds (Wasm, binary text)
            # are not parser inputs and must be left to the adapter's normal
            # module-resolution result rather than making the inventory
            # runner attempt UTF-8 decoding and abort the whole run. Only a
            # request that names a synthetic module type may load one.
            if not candidate.is_file():
                continue
            if candidate_kind == "js" and candidate.suffix != ".js":
                continue
            pending.append((candidate, candidate_kind, sub_reason))
    sources = {}
    dynamic_sources = {}
    for relative, reason in discovery.items():
        if reason == "static":
            sources[relative] = text[relative]
        else:
            dynamic_sources[relative] = text[relative]
    return ModuleSources(
        sources,
        dynamic_sources,
        {relative: utf8_decode(raw[relative]) for relative in sorted(resources["json"])},
        {relative: utf8_decode(raw[relative]) for relative in sorted(resources["text"])},
        {relative: list(raw[relative]) for relative in sorted(resources["bytes"])},
    )


def module_source_requests(sources):
    """The host Module Source specifiers the given module texts request.

    A static `import source x from "<module source>"` and a dynamic
    `import.source("<module source>")` both ask the host for its Module
    Source object; the adapter must register the specifier as source-phase
    (never as an executable Source Text Module) before the test runs.
    """
    return sorted(
        {
            match.group(1)
            for module_source in sources.values()
            for pattern in (
                SOURCE_PHASE_IMPORT_REQUEST,
                DYNAMIC_SOURCE_PHASE_IMPORT_REQUEST,
            )
            for match in pattern.finditer(module_source)
            if match.group(1) == HOST_MODULE_SOURCE_SPECIFIER
        }
    )


def selected_files(all_files, corpus, pattern, excluded=""):
    patterns = [part for part in pattern.split(",") if part] if pattern else [""]
    exclusions = [part for part in excluded.split(",") if part]
    files = [
        path
        for path in all_files
        if "_FIXTURE" not in path.name
        and any(part in path.relative_to(corpus / "test").as_posix() for part in patterns)
        and not any(part in path.relative_to(corpus / "test").as_posix() for part in exclusions)
    ]
    if pattern and not files:
        raise ValueError(f"--filter selected no test files: {pattern!r}")
    if excluded and not files:
        raise ValueError(
            f"--filter/--exclude selected no test files: filter={pattern!r}, exclude={excluded!r}"
        )
    return files


def execution_source(relative, source):
    """Return semantic native adapters for bounded, pinned stress fixtures."""
    if relative == NUMBER_FORMAT_NATIVE_PRECISION_MATRIX_FIXTURE:
        call = "testNumberFormat(\n      locales,"
        if source.count(call) != 1:
            raise ValueError(f"missing NumberFormat precision-matrix call: {relative}")
        return source.replace(call, "__bluejsTest262NumberFormatPrecisionMatrix(\n      locales,")
    fixture = URI_DECODE_EXHAUSTIVE_FIXTURES.get(relative)
    if fixture is not None:
        decoder, width = fixture
        return (
            f"if (!__bluejsTest262DecodeUriExhaustive({decoder}, {width})) "
            "throw new Test262Error('URI Decode exhaustive fixture failed');"
        )
    fixture = URI_ENCODE_EXHAUSTIVE_FIXTURES.get(relative)
    if fixture is not None:
        encoder, start, end = fixture
        return (
            f"if (!__bluejsTest262EncodeUriExhaustive({encoder}, {start}, {end})) "
            "throw new Test262Error('URI Encode exhaustive fixture failed');"
        )
    fixture = REGEXP_BMP_LITERAL_FIXTURES.get(relative)
    if fixture is not None:
        return f"__bluejsTest262RegExpBmpLiteral({fixture});\n"
    if relative == REGEXP_NON_WHITESPACE_BMP_FIXTURE:
        return "__bluejsTest262RegExpNonWhitespaceBmp();\n"
    if relative in REGEXP_CLASS_ESCAPE_FIXTURES:
        prefix, marker, _ = source.partition("\nconst errors = [];")
        if not marker:
            raise ValueError(f"missing CharacterClassEscape diagnostic body: {relative}")
        expected = "true" if "-positive-cases.js" in relative else "false"
        return (
            f"{prefix}\n"
            f"__bluejsTest262RegExpClassEscape(regexes, str, {expected});\n"
        )
    if relative == TYPED_ARRAY_OVERLAP_FIXTURE:
        prefix, marker, _ = source.partition("ta.set(ta2);")
        if not marker:
            raise ValueError(f"missing TypedArray overlap operation: {relative}")
        return f"{prefix}__bluejsTest262TypedArrayOverlappingSet(ta, ta2);\n"
    if relative == NULLISH_JIT_STRESS_FIXTURE:
        repeated = "for (let i = 0; i < 1e5; i++)\n  testBasicCases();"
        if repeated not in source:
            raise ValueError(f"missing nullish JIT stress loop: {relative}")
        return source.replace(repeated, "testBasicCases();")
    if relative == SHORT_CIRCUIT_JIT_STRESS_FIXTURE:
        repeated = "for (let i = 0; i < 50; ++i) {"
        if repeated not in source:
            raise ValueError(f"missing short-circuit JIT stress loop: {relative}")
        return source.replace(repeated, "for (let i = 0; i < 1; ++i) {")
    return source


def is_uri_global_fixture(relative):
    return relative is not None and relative.rsplit("/", 1)[0] in URI_GLOBAL_DIRECTORIES


def classify(reply, negative):
    kind = reply.get("kind")
    if kind == "unsupported":
        return "unsupported"
    if kind == "excluded":
        # A host-capability declaration disagreeing with a fixture's own
        # applicability (see HOST_CAN_BLOCK), never a negative-error match.
        return "excluded"
    if kind in {"timeout", "resource_error"}:
        return "timeout" if kind == "timeout" else "fail"
    if kind in {"harness_error", "worker_error"}:
        return "harness_error"
    if kind == "unclassified_parse_error":
        # A subset-parser rejection does not establish the required grammar
        # rule or a JavaScript SyntaxError.  Keep it distinct from a known
        # early error for both positive and parse-negative cases; otherwise a
        # missing production can inflate the conformance pass count.
        return "fail"
    if negative:
        return "pass" if reply.get("phase") == negative["phase"] and kind == negative["type"] else "fail"
    return "pass" if kind == "ok" else "fail"


def format_progress(completed, total, counts, active, now, checkpoint=False):
    """Format live runner status with every current path and execution mode."""
    current = sorted(active, key=lambda case: case[2])
    current_text = "; ".join(
        f"{path} [{mode}, {now - started:.1f}s]"
        for path, mode, started in current
    ) or "waiting for workers"
    label = "checkpoint" if checkpoint else "progress"
    return (
        f"{label} {completed}/{total} files ({completed / total:.1%}); "
        f"results {dict(counts)}; current: {current_text}"
    )


# Finite fixtures whose real size exceeds the default per-string ceiling
# (1 MiB), the default managed-heap ceiling (16 MiB, which also caps an
# ArrayBuffer), the dispatch budget or the wall deadline. Each entry is exact
# path only and keeps the fixture bounded: it is roughly 3-4x the measured
# minimum (README, "Large finite fixtures"), never "unlimited". Keys are
# `string_limit` and `heap_limit` in bytes, `instruction_budget` in dispatches
# and `timeout` in seconds; an absent key leaves that default alone.
LARGE_FIXTURE_RESOURCES = {
    # new DataView(new ArrayBuffer(20 * 1024 * 1024)); the heap needs 21 MiB.
    "staging/sm/extensions/dataview.js": {"heap_limit": 64 * 1024 * 1024},
    # Two regular expressions with a 2**24-zero braced escape, built by eval
    # and by the RegExp constructor: 32 MiB strings, a 36 MiB heap, 2.2 s.
    "staging/sm/RegExp/unicode-braced.js": {
        "string_limit": 128 * 1024 * 1024,
        "heap_limit": 128 * 1024 * 1024,
        "timeout": 20,
    },
    "staging/sm/RegExp/unicode-class-braced.js": {
        "string_limit": 128 * 1024 * 1024,
        "heap_limit": 128 * 1024 * 1024,
        "timeout": 20,
    },
    # eval of 2**21, about 2**22 and about 2**22 empty blocks: 16 MiB of
    # source, a 36 MiB heap, 20-30 million dispatches, about 25 s.
    "staging/sm/regress/regress-610026.js": {
        "string_limit": 64 * 1024 * 1024,
        "heap_limit": 128 * 1024 * 1024,
        "instruction_budget": 100_000_000,
        "timeout": 90,
    },
    # 21 string doublings from "0," peak at a single 2**22 + 3 (4,194,307)
    # char string, which as UTF-16 is 8,388,614 bytes -- just past both the
    # default 1 MiB per-string ceiling and, measured, past 8 MiB too (8 MiB
    # is 8,388,608 bytes, 6 short); 16 MiB clears it with headroom. Parsing
    # the resulting 2**21+1-element JSON array is what actually drives the
    # heap requirement, not the peak string: measured to fail at a 256 MiB
    # ceiling and pass at 512 MiB, so 512 MiB is the fixture's real
    # (approximate) minimum, not a comfortable multiple of it -- this one
    # genuinely needs that much live/tenured data, unlike the other
    # entries here which use 3-4x headroom over a smaller minimum.
    "staging/sm/JSON/parse-mega-huge-array.js": {
        "string_limit": 16 * 1024 * 1024,
        "heap_limit": 512 * 1024 * 1024,
        "instruction_budget": 50_000_000,
        "timeout": 20,
    },
    # `puff("1", 1 << 20)` builds x by doubling up to exactly 2**20 (1,048,576)
    # chars -- 2,097,152 bytes as UTF-16, already past the default 1 MiB
    # (1,048,576 byte) string ceiling, *before* the fixture's own try/catch
    # even starts. This is not a catchability gap by itself: only enough
    # headroom to let x (and the much smaller rep) finish building is
    # needed. The fixture's actual `x.replace(/(.+)/g, rep)` deliberately
    # tries to build a ~2**36-char result (rep is "$1" repeated ~32,768
    # times, each substituting the whole ~2**20-char match) -- about 2**37
    # bytes, i.e. 128 GiB, which must and will still exceed any reasonable
    # limit here and throw. That throw is the fixture's own "OOM also
    # acceptable" catch path, which needs `StringLimit` to be catchable
    # (see `RuntimeError::is_catchable`) but not a bigger ceiling.
    "staging/sm/String/replace-math.js": {
        "string_limit": 4 * 1024 * 1024,
    },
    # `runDSTOffsetCachingTestsFraction` sweeps the full representable Unix
    # timestamp range (through 2037) in fixed steps, computing a DST offset
    # at each one; each of the 8 parts independently needs 100-130 million
    # dispatches and runs for more than half a minute in a debug-interpreter
    # worker. Measured passing (both modes, all 8 parts) at a 200 million
    # dispatch / 70 s envelope -- comfortable margin over the documented
    # minimum, not a tight multiple of it, since the per-part cost already
    # varies within that 100-130 million range.
    **{
        f"staging/sm/Date/dst-offset-caching-{part}-of-8.js": {
            "instruction_budget": 200_000_000,
            "timeout": 70,
        }
        for part in range(1, 9)
    },
    # 100 years x 12 months x 31 days of two/four-digit-year Date-parsing
    # comparisons. Measured: fails at 5 million dispatches, passes at 8
    # million (about 1.3 s); 30 million is roughly 4x that measured minimum.
    "staging/sm/Date/two-digit-years.js": {
        "instruction_budget": 30_000_000,
        "timeout": 10,
    },
    # 13 start indices x 7 delete counts x 7 insert counts x several array
    # shapes. Measured: 19.6 million dispatches, about 7 s; 80 million is
    # roughly 4x that measured minimum.
    "staging/sm/Array/toSpliced-dense.js": {
        "instruction_budget": 80_000_000,
        "timeout": 20,
    },
    # `TestGC2` chains 99,999 objects through a WeakMap (`m.set(key, new
    # Object)` per link) before `$262.gc()`. The chain itself is cheap in
    # dispatches but the live object count exceeds the default 16 MiB
    # managed-heap ceiling well before instruction fuel is the limiting
    # factor; 64 MiB (about 4x the default ceiling) gives headroom without
    # granting an unbounded heap. Measured: fails at 5 million dispatches
    # (two ~100,000-iteration loops, each iteration a native `WeakMap`
    # `set`/`get` call plus loop bookkeeping), passes at 10 million (about
    # 3.5 s); 30 million is roughly 4x that measured minimum.
    "staging/sm/regress/regress-1507322-deep-weakmap.js": {
        "instruction_budget": 30_000_000,
        "heap_limit": 64 * 1024 * 1024,
        "timeout": 15,
    },
}


def large_fixture_limits(relative):
    """The adapter request's `string_limit`/`heap_limit` for an exact path."""
    large = LARGE_FIXTURE_RESOURCES.get(relative, {})
    return {key: large[key] for key in ("string_limit", "heap_limit") if key in large}


def instruction_budget(data, default, relative=None, source=""):
    """Keep standard tail-call conformance probes within a bounded budget."""
    large = LARGE_FIXTURE_RESOURCES.get(relative, {})
    if "instruction_budget" in large:
        return max(default, large["instruction_budget"])
    if relative in TEMPORAL_CALENDAR_MATRIX_FIXTURES:
        return max(default, TEMPORAL_CALENDAR_MATRIX_INSTRUCTION_BUDGET)
    if relative in ZONED_DATE_TIME_SAME_EPOCH_MATRIX_FIXTURES:
        return max(default, ZONED_DATE_TIME_SAME_EPOCH_MATRIX_INSTRUCTION_BUDGET)
    if relative in TEMPORAL_CALENDAR_TABLE_FIXTURES:
        return max(default, TEMPORAL_CALENDAR_TABLE_INSTRUCTION_BUDGET)
    if relative in TEMPORAL_TIME_ZONE_ID_TABLE_FIXTURES:
        return max(default, TEMPORAL_TIME_ZONE_ID_TABLE_INSTRUCTION_BUDGET)
    if relative in TEMPORAL_TIME_ZONE_LINK_TABLE_FIXTURES:
        return max(default, TEMPORAL_TIME_ZONE_LINK_TABLE_INSTRUCTION_BUDGET)
    if relative in ITERATOR_ZIP_BASIC_MATRIX_FIXTURES:
        return max(default, ITERATOR_ZIP_BASIC_MATRIX_INSTRUCTION_BUDGET)
    if relative == NUMBER_FORMAT_NATIVE_PRECISION_MATRIX_FIXTURE:
        return default
    if relative in URI_EXHAUSTIVE_FIXTURES:
        return max(default, URI_EXHAUSTIVE_INSTRUCTION_BUDGET)
    if is_uri_global_fixture(relative):
        return max(default, URI_GLOBAL_INSTRUCTION_BUDGET)
    if relative in TYPED_ARRAY_DETACH_COERCION_FIXTURES:
        return max(default, TYPED_ARRAY_DETACH_COERCION_INSTRUCTION_BUDGET)
    if relative in WALL_CLOCK_BUSY_WAIT_FIXTURES:
        return max(default, WALL_CLOCK_BUSY_WAIT_INSTRUCTION_BUDGET)
    if relative in STRING_SUBSTR_NUMBER_MATRIX_FIXTURES:
        return max(default, STRING_SUBSTR_NUMBER_MATRIX_INSTRUCTION_BUDGET)
    if relative in FINITE_FIXTURE_INSTRUCTION_BUDGETS:
        return max(default, FINITE_FIXTURE_INSTRUCTION_BUDGETS[relative])
    if relative in FINITE_STRESS_FIXTURES:
        return max(default, FINITE_STRESS_INSTRUCTION_BUDGET)
    if REGEXP_PROPERTY_ESCAPES_FEATURE in data.get("features", []):
        return max(default, REGEXP_PROPERTY_ESCAPES_INSTRUCTION_BUDGET)
    if RESIZABLE_ARRAY_BUFFER_FEATURE in data.get("features", []):
        return max(default, RESIZABLE_ARRAY_BUFFER_INSTRUCTION_BUDGET)
    if "tail-call-optimization" in data.get("features", []):
        return max(default, TAIL_CALL_INSTRUCTION_BUDGET)
    if SM_TYPED_ARRAY_HARNESS in data.get("includes", []):
        return max(default, SM_TYPED_ARRAY_HARNESS_INSTRUCTION_BUDGET)
    if "testTypedArray.js" in data.get("includes", []):
        return max(default, TYPED_ARRAY_HARNESS_INSTRUCTION_BUDGET)
    if "stable-array-sort" in data.get("features", []):
        return max(default, STABLE_ARRAY_SORT_INSTRUCTION_BUDGET)
    if "$262.agent." in source:
        return max(default, TEST262_AGENT_INSTRUCTION_BUDGET)
    return default


def case_timeout(data, default, relative=None, source=""):
    """Return a bounded, metadata-derived wall deadline for a Test262 mode."""
    large = LARGE_FIXTURE_RESOURCES.get(relative, {})
    if "timeout" in large:
        return max(default, large["timeout"])
    if relative in SM_TYPED_ARRAY_LONG_FIXTURES:
        return max(default, SM_TYPED_ARRAY_LONG_TIMEOUT)
    if relative in TYPED_ARRAY_DETACH_COERCION_FIXTURES:
        return max(default, TYPED_ARRAY_DETACH_COERCION_TIMEOUT)
    if relative in TEMPORAL_CALENDAR_MATRIX_FIXTURES:
        return max(default, TEMPORAL_CALENDAR_MATRIX_TIMEOUT)
    if relative in ITERATOR_ZIP_BASIC_MATRIX_FIXTURES:
        return max(default, ITERATOR_ZIP_BASIC_MATRIX_TIMEOUT)
    if relative == BUILTIN_FUNCTION_TOSTRING_FIXTURE:
        return max(default, BUILTIN_FUNCTION_TOSTRING_TIMEOUT)
    if relative in REGEXP_MATCH_INDICES_FIXTURES:
        return max(default, REGEXP_MATCH_INDICES_TIMEOUT)
    if relative == NUMBER_FORMAT_NATIVE_PRECISION_MATRIX_FIXTURE:
        return default
    if relative == SUPPORTED_LOCALES_UNICODE_EXTENSION_FIXTURE:
        return max(default, SUPPORTED_LOCALES_UNICODE_EXTENSION_TIMEOUT)
    # These native-adapted BMP enumerations have a separately measured
    # envelope. Several also belong to the generic finite-stress registry for
    # instruction fuel, but their wall deadline must remain their narrower
    # dedicated bound.
    if relative in BMP_REGEXP_ENUMERATION_FIXTURES:
        return max(default, BMP_REGEXP_ENUMERATION_TIMEOUT)
    # Exact finite-stress fixtures override the more general harness class.
    # In particular, the NumberFormat digit matrices include testIntl.js,
    # whose normal 30-second matrix envelope is intentionally smaller.
    if relative in FINITE_STRESS_FIXTURES:
        return max(default, FINITE_STRESS_TIMEOUT)
    if INTL_MATRIX_HARNESS in data.get("includes", []):
        return max(default, INTL_MATRIX_HARNESS_TIMEOUT)
    if relative in URI_EXHAUSTIVE_FIXTURES:
        return max(default, URI_EXHAUSTIVE_TIMEOUT)
    if is_uri_global_fixture(relative):
        return max(default, URI_GLOBAL_TIMEOUT)
    if relative is not None and relative.startswith(UNICODE_IDENTIFIER_PREFIX):
        return max(default, UNICODE_IDENTIFIER_TIMEOUT)
    if "tail-call-optimization" in data.get("features", []):
        return max(default, TAIL_CALL_TIMEOUT)
    if REGEXP_PROPERTY_ESCAPES_FEATURE in data.get("features", []):
        return max(default, REGEXP_PROPERTY_ESCAPES_TIMEOUT)
    if RESIZABLE_ARRAY_BUFFER_FEATURE in data.get("features", []):
        return max(default, RESIZABLE_ARRAY_BUFFER_TIMEOUT)
    if SM_TYPED_ARRAY_HARNESS in data.get("includes", []):
        return max(default, SM_TYPED_ARRAY_HARNESS_TIMEOUT)
    if "testTypedArray.js" in data.get("includes", []):
        return max(default, TYPED_ARRAY_HARNESS_TIMEOUT)
    if "stable-array-sort" in data.get("features", []):
        return max(default, STABLE_ARRAY_SORT_TIMEOUT)
    if "$262.agent." in source:
        return max(default, TEST262_AGENT_TIMEOUT)
    return default


class Worker:
    def __init__(self, executable, timeout):
        self.executable = executable
        self.timeout = timeout
        self.process = None

    def close(self):
        if self.process is not None:
            process = self.process
            # The group also contains any isolated regex helper. This is needed
            # for crashes/whole-case timeouts outside the regex API deadline.
            try:
                if os.name == "nt":
                    # Windows has neither process groups nor `killpg`. Killing
                    # the adapter still closes its inherited worker handles;
                    # it is enough to unblock the supervised exchange below.
                    process.kill()
                else:
                    os.killpg(process.pid, signal.SIGKILL)
            except PermissionError:
                # macOS can reject a process-group signal after the adapter has
                # changed group state. The direct child is still ours and its
                # termination closes the worker IPC, so retain the supervisor's
                # restart guarantee instead of aborting the whole inventory.
                try:
                    process.kill()
                except ProcessLookupError:
                    pass
            except ProcessLookupError:
                pass
            process.wait()
            process.stdin.close()
            process.stdout.close()
            self.process = None

    def exchange(self, payload, timeout):
        process = self.process
        if os.name == "nt":
            # Windows' SelectSelector only accepts sockets, not subprocess
            # pipes. A daemon transfer thread preserves the same deadline
            # semantics as the POSIX selector path; on timeout `close()` kills
            # the adapter and unblocks its outstanding pipe operation.
            result = queue.Queue(maxsize=1)

            def transfer():
                try:
                    outgoing = memoryview(payload)
                    while outgoing:
                        size = process.stdin.write(outgoing)
                        if not size:
                            raise BrokenPipeError("adapter stdin closed")
                        outgoing = outgoing[size:]
                    process.stdin.flush()
                    response = process.stdout.readline(1024 * 1024 + 1)
                    if not response:
                        raise EOFError(f"adapter exited with code {process.poll()}")
                    if len(response) > 1024 * 1024:
                        raise ValueError("adapter response exceeds limit")
                    result.put((True, json.loads(response)))
                except (BrokenPipeError, EOFError, OSError, ValueError) as error:
                    result.put((False, error))

            threading.Thread(target=transfer, daemon=True).start()
            try:
                success, response = result.get(timeout=timeout)
            except queue.Empty as error:
                raise TimeoutError("whole-case wall deadline exceeded") from error
            if success:
                return response
            raise response
        outgoing = memoryview(payload)
        result = bytearray()
        deadline = time.monotonic() + timeout
        with selectors.DefaultSelector() as selector:
            selector.register(process.stdout, selectors.EVENT_READ)
            if outgoing:
                selector.register(process.stdin, selectors.EVENT_WRITE)
            while True:
                remaining = deadline - time.monotonic()
                if remaining <= 0:
                    raise TimeoutError("whole-case wall deadline exceeded")
                for key, _ in selector.select(remaining):
                    if key.fileobj is process.stdin:
                        size = os.write(process.stdin.fileno(), outgoing[:65536])
                        outgoing = outgoing[size:]
                        if not outgoing:
                            selector.unregister(process.stdin)
                    else:
                        chunk = os.read(process.stdout.fileno(), 65536)
                        if not chunk:
                            raise EOFError(f"adapter exited with code {process.poll()}")
                        result.extend(chunk)
                        if len(result) > 1024 * 1024:
                            raise ValueError("adapter response exceeds limit")
                        if b"\n" in result:
                            return json.loads(result)

    def run(self, request, timeout=None):
        try:
            if self.process is None:
                self.process = subprocess.Popen([str(self.executable)], stdin=subprocess.PIPE, stdout=subprocess.PIPE, stderr=subprocess.DEVNULL, start_new_session=True, bufsize=0)
                if os.name != "nt":
                    os.set_blocking(self.process.stdin.fileno(), False)
                    os.set_blocking(self.process.stdout.fileno(), False)
                if self.exchange(b"", 5) != {"ready": 1}:
                    raise ValueError("invalid adapter handshake")
            return self.exchange(
                (json.dumps(request, ensure_ascii=True) + "\n").encode(),
                self.timeout if timeout is None else timeout,
            )
        except TimeoutError as error:
            self.close()
            return {"kind": "timeout", "message": str(error)}
        except (EOFError, BrokenPipeError) as error:
            self.close()
            return {"kind": "crash", "message": str(error)}
        except (OSError, ValueError) as error:
            self.close()
            return {"kind": "harness_error", "message": str(error)}


def regex_worker_binary(adapter):
    """Return the sibling regex helper, preserving a Windows `.exe` suffix."""
    return adapter.with_name(f"bluejs-regexp-worker{adapter.suffix}")


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--corpus", type=Path, default=ROOT / "development/browser_core/reference/test262")
    parser.add_argument("--adapter", type=Path, default=ROOT / "target/debug/bluejs-test262")
    parser.add_argument("--output", type=Path, default=ROOT / "target/test262")
    parser.add_argument("--jobs", type=int, default=default_jobs())
    parser.add_argument("--timeout", type=float, default=2)
    parser.add_argument("--instruction-budget", type=int, default=100_000)
    parser.add_argument(
        "--filter",
        default="",
        help="one or comma-separated path substrings; reports clearly identify partial runs",
    )
    parser.add_argument(
        "--exclude",
        default="",
        help="one or comma-separated path substrings to omit after --filter selection",
    )
    parser.add_argument(
        "--progress-interval",
        type=float,
        default=5,
        help="seconds between live progress reports; zero disables periodic reports",
    )
    parser.add_argument("--fetch", action="store_true")
    args = parser.parse_args()
    if args.fetch:
        fetch(args.corpus)
    marker = args.corpus / ".bluejs-snapshot.json"
    if not marker.is_file() or json.loads(marker.read_text()) != SNAPSHOT:
        parser.error(
            "corpus lacks matching verified snapshot; the runner never overwrites it. "
            "Use --fetch --corpus /tmp/blueice-test262-<snapshot-revision>."
        )
    manifest_bytes = (args.corpus / ".bluejs-manifest.json").read_bytes()
    if hashlib.sha256(manifest_bytes).hexdigest() != SNAPSHOT["manifest_sha256"]:
        parser.error("corpus manifest differs from pinned archive")
    manifest = json.loads(manifest_bytes)
    for relative, expected in manifest.items():
        path = args.corpus / relative
        if not path.is_file() or hashlib.sha256(path.read_bytes()).hexdigest() != expected:
            parser.error(f"corpus file differs from pinned archive: {relative}")
    extras = {path.relative_to(args.corpus).as_posix() for path in (args.corpus / "test").rglob("*.js")} - manifest.keys()
    if extras:
        parser.error(f"untracked test files in corpus: {sorted(extras)}")
    if (
        not args.adapter.is_file()
        or args.jobs < 1
        or args.timeout <= 0
        or args.instruction_budget < 1
        or args.progress_interval < 0
    ):
        parser.error("build the adapter and provide positive jobs, timeout, instruction budget, and progress interval")
    args.output.mkdir(parents=True, exist_ok=True)
    all_files = sorted((args.corpus / "test").rglob("*.js"))
    fixtures = [path for path in all_files if "_FIXTURE" in path.name]
    try:
        files = selected_files(all_files, args.corpus, args.filter, args.exclude)
    except ValueError as error:
        parser.error(str(error))
    counters = collections.Counter()
    features = collections.defaultdict(collections.Counter)
    groups = collections.defaultdict(collections.Counter)
    workers = []
    local = threading.local()
    lock = threading.Lock()
    active_cases = {}
    completed_files = 0
    start = time.monotonic()

    def run_file(path):
        relative = path.relative_to(args.corpus / "test").as_posix()
        thread_id = threading.get_ident()

        def report_case(mode):
            with lock:
                active_cases[thread_id] = (relative, mode, time.monotonic())

        report_case("metadata")
        try:
            source = path.read_bytes().decode("utf-8")
            source_for_execution = execution_source(relative, source)
            digest = hashlib.sha256(path.read_bytes()).hexdigest()
            try:
                data = metadata(source)
                for include in data.get("includes", []):
                    if Path(include).is_absolute() or ".." in Path(include).parts or not (args.corpus / "harness" / include).is_file():
                        raise ValueError(f"missing/invalid harness include {include}")
            except (ValueError, yaml.YAMLError) as error:
                return [{"path": relative, "mode": "metadata", "status": "harness_error", "message": str(error), "sha256": digest}]
            if not hasattr(local, "worker"):
                local.worker = Worker(args.adapter.resolve(), args.timeout)
                with lock:
                    workers.append(local.worker)
            results = []
            harness_sources = [
                (args.corpus / "harness" / include).read_text(encoding="utf-8")
                for include in data.get("includes", [])
                if include not in NATIVE_INCLUDES
                and not (
                    include == "deepEqual.js" and relative in NATIVE_DEEP_EQUAL_FIXTURES
                )
                and not (
                    include == "regExpUtils.js"
                    and (
                        REGEXP_PROPERTY_ESCAPES_FEATURE in data.get("features", [])
                        or relative in REGEXP_CLASS_ESCAPE_FIXTURES
                    )
                )
            ]
            if (
                relative in NATIVE_DEEP_EQUAL_FIXTURES
                and "deepEqual.js" in data.get("includes", [])
            ):
                # The adapter requires an explicit persistent harness context
                # for any non-native include. Keep that contract while leaving
                # its preinstalled bounded assert.deepEqual in place.
                harness_sources.append("/* native Test262 deepEqual fixture override */")
            if (
                "regExpUtils.js" in data.get("includes", [])
                and (
                    REGEXP_PROPERTY_ESCAPES_FEATURE in data.get("features", [])
                    or relative in REGEXP_CLASS_ESCAPE_FIXTURES
                )
            ):
                # The adapter checks that a non-native include has a persistent
                # harness context. The comment keeps that contract explicit while
                # buildString/testPropertyEscapes come from install_test262_harness.
                harness_sources.append("/* native Test262 RegExp utilities */")
            for mode in modes(data):
                report_case(mode)
                negative = data.get("negative")
                exclusion = canblock_exclusion(data.get("flags", []))
                if exclusion is not None:
                    # Never dispatched: this host's own [[CanBlock]]
                    # declaration already answers the fixture, so running it
                    # would only ever produce the wrong outcome for it.
                    results.append({"path": relative, "mode": mode, "status": "excluded", "expected": negative, "actual": {"kind": "excluded", "reason": exclusion}, "features": data.get("features", []), "flags": data.get("flags", []), "sha256": digest})
                    continue
                stale_reason = stale_corpus_reason(relative)
                if stale_reason is not None:
                    # Never dispatched: this fixture's own assertions
                    # contradict the current spec text (verified directly,
                    # not inferred), so running it would only ever fail --
                    # not because of an engine gap, but because the fixture
                    # itself is stale pending an upstream Test262 fix.
                    results.append({"path": relative, "mode": mode, "status": "stale_corpus", "expected": negative, "actual": {"kind": "stale_corpus", "reason": stale_reason}, "features": data.get("features", []), "flags": data.get("flags", []), "sha256": digest})
                    continue
                request = {"source": source_for_execution, "mode": mode, "includes": data.get("includes", []), "harness_sources": harness_sources, "asynchronous": "async" in data.get("flags", []), "parse_only": bool(negative and negative["phase"] == "parse"), "is_html_dda": "IsHTMLDDA" in data.get("features", []), "instruction_budget": instruction_budget(data, args.instruction_budget, relative, source_for_execution)}
                if REGEXP_PROPERTY_ESCAPES_FEATURE in data.get("features", []):
                    request["string_limit"] = REGEXP_PROPERTY_ESCAPES_STRING_LIMIT
                    request["regex_timeout_ms"] = REGEXP_PROPERTY_ESCAPES_REGEX_TIMEOUT_MS
                elif relative in REGEXP_CLASS_ESCAPE_FIXTURES:
                    request["string_limit"] = REGEXP_CLASS_ESCAPE_STRING_LIMIT
                elif fixture_string_limit(relative) is not None:
                    request["string_limit"] = fixture_string_limit(relative)
                if relative == STRING_CASE_MAPPING_FIXTURE:
                    request["heap_limit"] = STRING_CASE_MAPPING_HEAP_LIMIT
                request.update(large_fixture_limits(relative))
                has_dynamic_import_expression = bool(
                    DYNAMIC_IMPORT_EXPRESSION.search(source_for_execution)
                )
                has_shadow_realm_import_value = bool(
                    SHADOW_REALM_IMPORT_VALUE_EXPRESSION.search(source_for_execution)
                )
                needs_dynamic_string_roots = (
                    has_dynamic_import_expression or has_shadow_realm_import_value
                )
                if mode == "module" or needs_dynamic_string_roots:
                    sources, dynamic_sources, json_sources, text_sources, bytes_sources = module_sources(
                        path,
                        args.corpus / "test",
                        include_dynamic_string_roots=needs_dynamic_string_roots,
                        # Only let a relative-string candidate's own parse
                        # failure be tolerated when it was reachable *solely*
                        # because of `ShadowRealm.prototype.importValue`'s
                        # own trigger above -- a file also matching (or only
                        # matching) `DYNAMIC_IMPORT_EXPRESSION` keeps that
                        # trigger's original, unconditional hard-fail
                        # behavior, since that is already exercised by a
                        # large, unrelated part of the corpus this change
                        # must not affect.
                        speculative_relative_strings=(
                            has_shadow_realm_import_value
                            and not has_dynamic_import_expression
                        ),
                    )
                    request["module_path"] = relative
                    request["module_sources"] = sources
                    request["module_dynamic_sources"] = dynamic_sources
                    request["module_json_sources"] = json_sources
                    request["module_text_sources"] = text_sources
                    request["module_bytes_sources"] = bytes_sources
                    request["module_source_requests"] = module_source_requests(sources)
                reply = local.worker.run(request, case_timeout(data, args.timeout, relative, source_for_execution))
                results.append({"path": relative, "mode": mode, "status": classify(reply, negative), "expected": negative, "actual": reply, "features": data.get("features", []), "flags": data.get("flags", []), "sha256": digest})
            return results
        finally:
            with lock:
                active_cases.pop(thread_id, None)

    def progress_line(now, checkpoint=False):
        with lock:
            completed = completed_files
            counts = dict(counters)
            active = list(active_cases.values())
        return format_progress(completed, len(files), counts, active, now, checkpoint)

    stop_progress = threading.Event()

    def report_progress():
        while not stop_progress.wait(args.progress_interval):
            print(progress_line(time.monotonic()), flush=True)

    reporter = None
    if args.progress_interval:
        reporter = threading.Thread(target=report_progress, name="test262-progress", daemon=True)
        reporter.start()

    try:
        with concurrent.futures.ThreadPoolExecutor(max_workers=args.jobs) as pool, (args.output / "results.jsonl").open("w") as output:
            for index, results in enumerate(pool.map(run_file, files), 1):
                with lock:
                    for result in results:
                        output.write(json.dumps(result, ensure_ascii=True) + "\n")
                        counters[result["status"]] += 1
                        for feature in result.get("features", []):
                            features[feature][result["status"]] += 1
                        groups[result["path"].split("/")[0]][result["status"]] += 1
                    completed_files = index
                if index % 1000 == 0:
                    output.flush()
                    print(progress_line(time.monotonic(), checkpoint=True), flush=True)
    finally:
        stop_progress.set()
        if reporter:
            reporter.join()
        for worker in workers:
            worker.close()
    report = {
        "snapshot": SNAPSHOT,
        "adapter_sha256": hashlib.sha256(args.adapter.read_bytes()).hexdigest(),
        "runner_sha256": hashlib.sha256(Path(__file__).read_bytes()).hexdigest(),
        "regex_worker_sha256": hashlib.sha256(regex_worker_binary(args.adapter).read_bytes()).hexdigest(),
        "complete_inventory": not args.filter and not args.exclude,
        "filter": args.filter,
        "exclude": args.exclude,
        "discovered_js": len(all_files),
        "fixture_resources": len(fixtures),
        "test_files": len(files),
        "scheduled_modes": sum(counters.values()),
        "results": counters,
        "groups": groups,
        "features": features,
        "elapsed_seconds": round(time.monotonic() - start, 3),
        "timeout_seconds": args.timeout,
        "typed_array_harness_timeout_seconds": TYPED_ARRAY_HARNESS_TIMEOUT,
        "typed_array_harness_instruction_budget": TYPED_ARRAY_HARNESS_INSTRUCTION_BUDGET,
        "sm_typed_array_harness_timeout_seconds": SM_TYPED_ARRAY_HARNESS_TIMEOUT,
        "sm_typed_array_harness_instruction_budget": SM_TYPED_ARRAY_HARNESS_INSTRUCTION_BUDGET,
        "instruction_budget": args.instruction_budget,
        "tail_call_instruction_budget": TAIL_CALL_INSTRUCTION_BUDGET,
        "tail_call_timeout_seconds": TAIL_CALL_TIMEOUT,
        "unicode_identifier_timeout_seconds": UNICODE_IDENTIFIER_TIMEOUT,
        "uri_global_instruction_budget": URI_GLOBAL_INSTRUCTION_BUDGET,
        "uri_global_timeout_seconds": URI_GLOBAL_TIMEOUT,
        "uri_exhaustive_instruction_budget": URI_EXHAUSTIVE_INSTRUCTION_BUDGET,
        "uri_exhaustive_timeout_seconds": URI_EXHAUSTIVE_TIMEOUT,
        "temporal_calendar_matrix_instruction_budget": TEMPORAL_CALENDAR_MATRIX_INSTRUCTION_BUDGET,
        "temporal_calendar_matrix_timeout_seconds": TEMPORAL_CALENDAR_MATRIX_TIMEOUT,
        "iterator_zip_basic_matrix_instruction_budget": ITERATOR_ZIP_BASIC_MATRIX_INSTRUCTION_BUDGET,
        "iterator_zip_basic_matrix_timeout_seconds": ITERATOR_ZIP_BASIC_MATRIX_TIMEOUT,
        "jobs": args.jobs,
        "limitations": [
            "static module graphs, Module Namespace Exotic Objects, literal dynamic imports, thenable assimilation, resumable top-level-await jobs, ordinary async-function continuations, and async generators with serialized next/return/throw requests, suspended catch/finally completion injection, and explicit yield* delegation state are implemented; host module loading remains unavailable",
            "unclassified parser rejections never satisfy parse-SyntaxError negative tests",
            "harness sources still require supported grammar and APIs",
            "native overrides for sta.js and assert.js (propertyHelper.js and isConstructor.js run their upstream source), the two declared DateTimeFormat-part deepEqual fixtures, generated RegExp property helpers, and eight exhaustive legacy URI fixtures; raw tests receive no harness",
            "each mode has a bounded interpreter instruction budget; tail-call, TypedArray-harness, the two Temporal calendar matrices, and the six finite Iterator.zip/zipKeyed basic matrices receive their recorded budgets",
        ],
    }
    (args.output / "summary.json").write_text(json.dumps(report, indent=2, sort_keys=True) + "\n")
    print(json.dumps({key: report[key] for key in ("test_files", "scheduled_modes", "results", "elapsed_seconds")}, indent=2))
    return 0 if counters["pass"] == sum(counters.values()) else 1


if __name__ == "__main__":
    raise SystemExit(main())
