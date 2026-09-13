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
MODULE_REQUEST = re.compile(
    r'''\bimport\s+(?:[^;]*?\bfrom\s+)?["']([^"']+)["']|\bexport\s+(?:\*\s*(?:as\s+(?:[\w$]+|"[^"]*"|'[^']*')\s*)?|\{[^}]*\}\s+)from\s*["']([^"']+)["']''',
    re.DOTALL,
)
DYNAMIC_IMPORT_REQUEST = re.compile(
    r'''\bimport\s*(?:\.\s*(?:source|defer)\s*)?\(\s*["']([^"']+)["']\s*\)'''
)
DYNAMIC_IMPORT_EXPRESSION = re.compile(
    r'''\bimport\s*(?:\(|\.\s*(?:source|defer)\s*\()'''
)
RELATIVE_STRING = re.compile(r'''["'](\.{1,2}/[^"']+)["']''')
SOURCE_PHASE_IMPORT_REQUEST = re.compile(
    r'''\bimport\s+source\s+[\w$]+\s+from\s*["']([^"']+)["']''',
    re.DOTALL,
)
NATIVE_INCLUDES = frozenset({"sta.js", "assert.js", "propertyHelper.js", "isConstructor.js"})
TAIL_CALL_INSTRUCTION_BUDGET = 3_000_000
TAIL_CALL_TIMEOUT = 30
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
        "built-ins/RegExp/character-class-escape-non-whitespace.js",
        "built-ins/String/prototype/repeat/repeat-string-n-times.js",
        "built-ins/parseFloat/S15.1.2.3_A6.js",
        "built-ins/parseInt/S15.1.2.2_A7.2_T1.js",
        "built-ins/parseInt/S15.1.2.2_A7.3_T1.js",
        "built-ins/parseInt/S15.1.2.2_A8.js",
        # These fixtures perform finite but interpreter-heavy validation: four
        # iterate locale-tag data through the Test262 Intl helper, three drive
        # multi-module top-level-await graphs, and the sparse-array test scans
        # several thousand holes. Keep their resource envelope explicit.
        "intl402/Intl/getCanonicalLocales/canonicalized-tags.js",
        "intl402/Intl/getCanonicalLocales/complex-region-subtag-replacement.js",
        "intl402/Intl/getCanonicalLocales/transformed-ext-valid.js",
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
        "staging/sm/Function/has-instance-jitted.js",
        "staging/sm/Function/function-toString-builtin.js",
        "staging/sm/Proxy/ownkeys-linear.js",
        "staging/sm/String/fromCodePoint.js",
        "staging/sm/String/string-pad-start-end.js",
        "staging/sm/String/string-upper-lower-mapping.js",
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
        "intl402/supportedLocalesOf-consistent-with-resolvedOptions.js",
        "intl402/supportedLocalesOf-unicode-extensions-ignored.js",
    }
)
FINITE_STRESS_INSTRUCTION_BUDGET = 10_000_000
FINITE_STRESS_TIMEOUT = 90
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


def module_sources(entry, test_root, include_dynamic_string_roots=False):
    """Collect static imports and, when needed, relative dynamic-import roots.

    A dynamic import with a variable specifier cannot be resolved statically.
    Test262 fixtures conventionally retain its relative candidate strings in
    the test source, so a caller may opt into supplying existing sibling
    files without making unrelated ordinary module tests over-inclusive.
    """
    test_root = test_root.resolve()
    pending = [entry.resolve()]
    sources = {}
    while pending:
        path = pending.pop()
        relative = path.relative_to(test_root).as_posix()
        if relative in sources:
            continue
        source = path.read_text(encoding="utf-8")
        sources[relative] = source
        requests = [
            match.group(1) or match.group(2) for match in MODULE_REQUEST.finditer(source)
        ]
        requests.extend(match.group(1) for match in DYNAMIC_IMPORT_REQUEST.finditer(source))
        if include_dynamic_string_roots:
            requests.extend(match.group(1) for match in RELATIVE_STRING.finditer(source))
        for request in requests:
            if not request or not request.startswith("."):
                continue
            candidate = (path.parent / request).resolve()
            try:
                candidate.relative_to(test_root)
            except ValueError:
                continue
            # Module source collection is a JavaScript module graph for the
            # adapter. Import attributes can name JSON, Wasm, or binary
            # fixtures; those are not parser inputs and must be left to the
            # adapter's normal module-resolution result rather than making the
            # inventory runner attempt UTF-8 decoding and abort the whole run.
            if candidate.is_file() and candidate.suffix == ".js":
                pending.append(candidate)
    return sources


def selected_files(all_files, corpus, pattern):
    files = [
        path
        for path in all_files
        if "_FIXTURE" not in path.name
        and pattern in path.relative_to(corpus / "test").as_posix()
    ]
    if pattern and not files:
        raise ValueError(f"--filter selected no test files: {pattern!r}")
    return files


def execution_source(relative, source):
    """Return semantic native adapters for finite URI encode/decode fixtures."""
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


def instruction_budget(data, default, relative=None, source=""):
    """Keep standard tail-call conformance probes within a bounded budget."""
    if relative in URI_EXHAUSTIVE_FIXTURES:
        return max(default, URI_EXHAUSTIVE_INSTRUCTION_BUDGET)
    if is_uri_global_fixture(relative):
        return max(default, URI_GLOBAL_INSTRUCTION_BUDGET)
    if relative in FINITE_STRESS_FIXTURES:
        return max(default, FINITE_STRESS_INSTRUCTION_BUDGET)
    if REGEXP_PROPERTY_ESCAPES_FEATURE in data.get("features", []):
        return max(default, REGEXP_PROPERTY_ESCAPES_INSTRUCTION_BUDGET)
    if RESIZABLE_ARRAY_BUFFER_FEATURE in data.get("features", []):
        return max(default, RESIZABLE_ARRAY_BUFFER_INSTRUCTION_BUDGET)
    if "tail-call-optimization" in data.get("features", []):
        return max(default, TAIL_CALL_INSTRUCTION_BUDGET)
    if "testTypedArray.js" in data.get("includes", []):
        return max(default, TYPED_ARRAY_HARNESS_INSTRUCTION_BUDGET)
    if "stable-array-sort" in data.get("features", []):
        return max(default, STABLE_ARRAY_SORT_INSTRUCTION_BUDGET)
    if "$262.agent." in source:
        return max(default, TEST262_AGENT_INSTRUCTION_BUDGET)
    return default


def case_timeout(data, default, relative=None, source=""):
    """Return a bounded, metadata-derived wall deadline for a Test262 mode."""
    if relative in URI_EXHAUSTIVE_FIXTURES:
        return max(default, URI_EXHAUSTIVE_TIMEOUT)
    if is_uri_global_fixture(relative):
        return max(default, URI_GLOBAL_TIMEOUT)
    if relative in BMP_REGEXP_ENUMERATION_FIXTURES:
        return max(default, BMP_REGEXP_ENUMERATION_TIMEOUT)
    if relative in FINITE_STRESS_FIXTURES:
        return max(default, FINITE_STRESS_TIMEOUT)
    if relative is not None and relative.startswith(UNICODE_IDENTIFIER_PREFIX):
        return max(default, UNICODE_IDENTIFIER_TIMEOUT)
    if "tail-call-optimization" in data.get("features", []):
        return max(default, TAIL_CALL_TIMEOUT)
    if REGEXP_PROPERTY_ESCAPES_FEATURE in data.get("features", []):
        return max(default, REGEXP_PROPERTY_ESCAPES_TIMEOUT)
    if RESIZABLE_ARRAY_BUFFER_FEATURE in data.get("features", []):
        return max(default, RESIZABLE_ARRAY_BUFFER_TIMEOUT)
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
            # The group also contains any isolated regex helper. This is needed
            # for crashes/whole-case timeouts outside the regex API deadline.
            try:
                os.killpg(self.process.pid, signal.SIGKILL)
            except ProcessLookupError:
                pass
            self.process.wait()
            self.process.stdin.close()
            self.process.stdout.close()
            self.process = None

    def exchange(self, payload, timeout):
        process = self.process
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


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--corpus", type=Path, default=ROOT / "development/browser_core/reference/test262")
    parser.add_argument("--adapter", type=Path, default=ROOT / "target/debug/bluejs-test262")
    parser.add_argument("--output", type=Path, default=ROOT / "target/test262")
    parser.add_argument("--jobs", type=int, default=8)
    parser.add_argument("--timeout", type=float, default=2)
    parser.add_argument("--instruction-budget", type=int, default=100_000)
    parser.add_argument("--filter", default="", help="path substring; reports clearly identify partial runs")
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
        parser.error("corpus lacks matching verified snapshot; use --fetch with an absent destination")
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
        files = selected_files(all_files, args.corpus, args.filter)
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
                    include == "regExpUtils.js"
                    and (
                        REGEXP_PROPERTY_ESCAPES_FEATURE in data.get("features", [])
                        or relative in REGEXP_CLASS_ESCAPE_FIXTURES
                    )
                )
            ]
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
                request = {"source": source_for_execution, "mode": mode, "includes": data.get("includes", []), "harness_sources": harness_sources, "asynchronous": "async" in data.get("flags", []), "parse_only": bool(negative and negative["phase"] == "parse"), "is_html_dda": "IsHTMLDDA" in data.get("features", []), "instruction_budget": instruction_budget(data, args.instruction_budget, relative, source_for_execution)}
                if REGEXP_PROPERTY_ESCAPES_FEATURE in data.get("features", []):
                    request["string_limit"] = REGEXP_PROPERTY_ESCAPES_STRING_LIMIT
                    request["regex_timeout_ms"] = REGEXP_PROPERTY_ESCAPES_REGEX_TIMEOUT_MS
                elif relative in REGEXP_CLASS_ESCAPE_FIXTURES:
                    request["string_limit"] = REGEXP_CLASS_ESCAPE_STRING_LIMIT
                if mode == "module" or DYNAMIC_IMPORT_EXPRESSION.search(source_for_execution):
                    sources = module_sources(
                        path,
                        args.corpus / "test",
                        include_dynamic_string_roots=bool(DYNAMIC_IMPORT_EXPRESSION.search(source_for_execution)),
                    )
                    request["module_path"] = relative
                    request["module_sources"] = sources
                    request["module_source_requests"] = sorted(
                        {
                            match.group(1)
                            for module_source in sources.values()
                            for match in SOURCE_PHASE_IMPORT_REQUEST.finditer(module_source)
                            if match.group(1) == "<module source>"
                        }
                    )
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
    report = {"snapshot": SNAPSHOT, "adapter_sha256": hashlib.sha256(args.adapter.read_bytes()).hexdigest(), "runner_sha256": hashlib.sha256(Path(__file__).read_bytes()).hexdigest(), "regex_worker_sha256": hashlib.sha256(args.adapter.with_name("bluejs-regexp-worker").read_bytes()).hexdigest(), "complete_inventory": not args.filter, "filter": args.filter, "discovered_js": len(all_files), "fixture_resources": len(fixtures), "test_files": len(files), "scheduled_modes": sum(counters.values()), "results": counters, "groups": groups, "features": features, "elapsed_seconds": round(time.monotonic() - start, 3), "timeout_seconds": args.timeout, "typed_array_harness_timeout_seconds": TYPED_ARRAY_HARNESS_TIMEOUT, "typed_array_harness_instruction_budget": TYPED_ARRAY_HARNESS_INSTRUCTION_BUDGET, "instruction_budget": args.instruction_budget, "tail_call_instruction_budget": TAIL_CALL_INSTRUCTION_BUDGET, "tail_call_timeout_seconds": TAIL_CALL_TIMEOUT, "unicode_identifier_timeout_seconds": UNICODE_IDENTIFIER_TIMEOUT, "uri_global_instruction_budget": URI_GLOBAL_INSTRUCTION_BUDGET, "uri_global_timeout_seconds": URI_GLOBAL_TIMEOUT, "uri_exhaustive_instruction_budget": URI_EXHAUSTIVE_INSTRUCTION_BUDGET, "uri_exhaustive_timeout_seconds": URI_EXHAUSTIVE_TIMEOUT, "jobs": args.jobs, "limitations": ["static module graphs, Module Namespace Exotic Objects, literal dynamic imports, thenable assimilation, resumable top-level-await jobs, ordinary async-function continuations, and async generators with serialized next/return/throw requests, suspended catch/finally completion injection, and explicit yield* delegation state are implemented; host module loading remains unavailable", "unclassified parser rejections never satisfy parse-SyntaxError negative tests", "harness sources still require supported grammar and APIs", "native overrides for sta.js, assert.js, propertyHelper.js, isConstructor.js, generated RegExp property helpers, and eight exhaustive legacy URI fixtures; raw tests receive no harness", "each mode has a bounded interpreter instruction budget; tail-call and TypedArray-harness fixtures receive their recorded budgets"]}
    (args.output / "summary.json").write_text(json.dumps(report, indent=2, sort_keys=True) + "\n")
    print(json.dumps({key: report[key] for key in ("test_files", "scheduled_modes", "results", "elapsed_seconds")}, indent=2))
    return 0 if counters["pass"] == sum(counters.values()) else 1


if __name__ == "__main__":
    raise SystemExit(main())
