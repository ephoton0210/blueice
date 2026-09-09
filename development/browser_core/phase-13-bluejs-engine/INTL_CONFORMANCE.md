# Intl, regex deadlines and Test262

Requested 2026-09-09, continuing from `2519fc1`. This document records the design before implementation and will record actual validation outcomes.

## Regex isolation

Run the standalone regress algorithm in a sibling `bluejs-regexp-worker` process. Both compilation (including parser literal validation) and matching use a bounded request/reply channel. A parent deadline covers worker IO and algorithm execution; timeout kills and reaps the worker and joins its IO thread before returning a resource error. Later operations start a fresh worker. Do not use detached matching threads or cooperative cancellation as a hard timeout. UTF-16 input and capture ranges/names cross the protocol losslessly. Cache one compiled pattern per worker; no compiled regex executes in the BlueJS process. Discover the installed sibling binary or an explicitly configured `BLUEJS_REGEXP_WORKER` path; fail closed if unavailable. The host must ship the helper beside its executable. Defaults and configurable VM limits will be tested through the public pipeline, including catastrophic backtracking and recovery.

## Internationalization

Target published ECMA-402 edition 13 (June 2026), verified at the [official publication page](https://ecma-international.org/publications-and-standards/standards/ecma-402/), rather than the living 2027 draft. ICU supplies locale/case/collation algorithms and data; BlueJS implements observable argument conversions, option order, builtin objects/descriptors and exceptions. Scope follows the preceding String task: Intl.Collator, Intl.getCanonicalLocales and the three locale-sensitive String methods. This is not the entire ECMA-402 library; Intl.Locale, NumberFormat, DateTimeFormat, DisplayNames, DurationFormat, ListFormat, PluralRules, RelativeTimeFormat, Segmenter and supportedValuesOf remain unimplemented.

## Test262

Pin official tc39/test262 to `6eec1ac9ee144dafd8f344d73a21f36bfc9f6755` (2026-06-29), the latest commit before the June 30 edition approval. This is a reproducible time-based snapshot, not proof that every test belongs to the published edition; report feature metadata and staging cases separately. Read the pinned `INTERPRETING.md` and `CONTRIBUTING.md`. Discover every test except `_FIXTURE` resources, honor raw/noStrict/onlyStrict/module/async and includes, validate YAML metadata and error phase/type, and use fresh realms. Preserve an explicit outcome for every scheduled mode: pass, fail, unsupported, timeout or harness error. Unsupported syntax must not accidentally satisfy parse-negative tests. Full-suite execution and full-suite passing are different claims. Store the revision, counts, per-file outcomes and an aggregate report; do not silently discard mandatory cases or call the current language subset conformant.

## Implemented behavior and deployment

`VmConfig::regex_timeout` defaults to 250 ms per compilation/match. Parser literal validation uses that same default independently of VM configuration; `ParseError.resource` preserves worker/deadline failures instead of mislabelling them SyntaxErrors. Startup has a separate two-second handshake bound. Parent serialization/deserialization and linear metadata scans are outside the regex algorithm deadline. The protocol limits each frame to 16 MiB, validates capture ranges, and preserves UTF-16. A worker caches one pattern per Rust thread, is killed/reaped after a failed transaction and restarts on demand. A worker IO thread only moves bytes; regex work runs exclusively in the child. Regex process memory and ICU/AST/bytecode/allocator storage are not a managed-heap/RSS guarantee.

Build and install `bluejs-regexp-worker` beside every executable embedding BlueJS, or set `BLUEJS_REGEXP_WORKER` to its trusted path. A missing helper fails closed. The standalone algorithm dependency remains regress 0.12.0. ICU4X Collator/Locale/CaseMap use bundled 2.2.0 data, with locale-core pinned at 2.3.0; serde 1.0.229 and serde_json 1.0.151 frame the subprocess protocol.

Collator supports sort/search usage, numeric ordering, caseFirst, sensitivity, ignorePunctuation, locale extension keys and supported collation tailorings. Locale negotiation uses the explicitly advertised language set in `src/intl.rs`, CLDR region/script fallback, and deterministic `en-US` default; best-fit currently uses the same matching policy as lookup. Casing validates every requested locale, uses the first one and preserves lone surrogates. Canonicalization rejects invalid language tags, underscores, duplicate variants and extensions before applying ICU aliases. Bound comparison identity, descriptors, constructor prototype lookup and GC cycles are tested. General global environment semantics remain a broader engine limitation.

The Test262 assertion host also required basic Error/TypeError/RangeError/SyntaxError/ReferenceError/EvalError/URIError constructors, Error.prototype.toString and cause properties. These are not a claim of complete Error builtins or exception/catch semantics. Host-installed names and typeof now consult the existing global object; a persistent multi-script lexical environment is still absent.

## Test-content review

Reviewed public-pipeline tests for locale coercion/getter order, invalid locale/options, language-specific casing, UTF-16 boundaries, numeric/accent/case/punctuation comparisons, extension overrides, bound function identity/receiver behavior, constructor prototypes, tiny-nursery GC and reclamation of the Collator/compare cycle. Repeated heap-ceiling tests exercise builtin and harness bootstrap failures. The review removed the obsolete fixed-root locale path from pure String algorithms.

Regex tests exercise catastrophic matching and compilation, all String regex protocols, observable lastIndex state after abrupt completion, recovery, absent/terminated/malformed workers, invalid capture ranges, oversized frames and outgoing requests, and timeout/error distinction. Four small transport unit tests supplement subprocess/public-pipeline tests for outgoing bounds and disconnected channels; engine files remain in the coverage gate.

Runner tests check metadata modes, negative phase/type matching, rejection of unclassified parser negatives, fresh realms, raw mode, unsupported includes/module/async, resource limits, protocol failures and supervisor termination/restart. Review corrected module+raw metadata, nested `sm/` includes, core assertion helper availability, host-hook misclassification, and harness temporary-root cleanup. Every output record was independently reconciled against unique path/mode pairs and the file inventory.

## Actual validation, 2026-09-09

- **238 default Rust unit/integration tests and two doc examples pass.** Three Python supervisor/metadata tests pass.
- BlueJS default-suite coverage: **7,064/7,064 lines (100%)**, **745/745 functions**, **94.68% regions**. No engine or adapter file exclusions; coverage is not specification completeness.
- Node.js v24.19.0: **22,216 isolated scripts pass**, including 612 new locale/collation/canonicalization cases and the previous 131,072 batched code-unit escape comparisons.
- Workspace build/all-target Clippy with `-D warnings` and workspace tests pass. Socket tests ran with their required local-network permissions.
- Full Test262 inventory: **53,404 test files**, **102,578 modes**, **8,466 pass**, **57,546 fail**, **36,566 unsupported**, **0 harness errors**, **0 timeouts** in this run. The dedicated adversarial regex tests separately verify actual timeout behavior.

The [checked-in summary](test262-summary.json) includes hashes and all feature/group counts. Local full records are `target/test262/results.jsonl` and `results.jsonl.gz`; regenerate them with the [runner instructions](../../../backend/bluejs/test262/README.md). The complete inventory run exits **1**, correctly: most of the suite does not pass. Unsupported includes alone account for 17,344 modes. Modules, async jobs, classes/destructuring, exceptions, complete globals/builtins and additional Intl constructors must be implemented before claiming complete Test262/ECMA-402 conformance.
