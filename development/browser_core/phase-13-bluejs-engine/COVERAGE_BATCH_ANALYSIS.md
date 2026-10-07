# BlueJS Coverage Batch Analysis

## Current verified source

Measurement: `2026-10-07T10:29:28.165513+00:00`. Run: `20261007-173453-3cd81e5f`.
Source fingerprint: `f597b400fb6d359005f4f3276186ac7e92a50ef9dfa76ccfba197ecdfe5ca782`.

The current inventory has **139/169 complete production files**. The remaining 30 files miss 254 raw lines, 38 functions and 609 regions.

All 134 files complete before this batch remain complete, including the 95 original files, all 22 selected D5/D4 files and all 51 preceding modified production files. Semantic outcome changes and per-file coverage percentage regressions are zero.

| Verification gate | Current result |
| --- | --- |
| BlueJS Rust | 334 targets; 4,118 passed, zero failed, 4 existing ignored |
| Complete Test262 | 102,921 applicable modes passed; 4 excluded and 1 stale modes |
| Semantic comparison | 102,926 contracts; zero changes |
| Workspace runtime tests | 2,170 passed, 0 failed, 60 ignored |
| BlueJS Rustdoc tests | 2 passed, 0 failed, 0 ignored |
| Self-test tooling contracts | 82 passed, 0 failed, 0 ignored |
| Static checks | Workspace Clippy with warnings denied; 115 task Rust files formatted; diff check passed |

All source and fixtures were prepared before execution. The common-fixture correction partition selected two Rust owners before this complete run. Completion uses only this full run's fresh atomic LLVM profiles and executable maps. Function instantiations use LLVM maxima, never complementary-branch unions.

## Implemented common boundaries

| Boundary | Owner | Contract and independent consumers |
| --- | --- | --- |
| Capability classification | [`heap/capabilities.rs`](../../../backend/bluejs/src/heap/capabilities.rs), [`vm/classification.rs`](../../../backend/bluejs/src/vm/classification.rs) | One fallible heap lookup; immutable callable/constructible/HTMLDDA bits. Foreign/reverse overrides and revoked-Proxy capabilities remain intact. No callbacks or managed allocation. Public method lookup stays fallible. |
| Temporal receiver data | [`vm/temporal/receiver.rs`](../../../backend/bluejs/src/vm/temporal/receiver.rs) | Exact native brand validation precedes observable argument access. Instant, PlainTime, withCalendar and zoned algorithms consume an owned slot snapshot. Arguments retain separate property-bag/coercion/range/provider errors. |
| Temporal native kind mapping | [`vm/temporal/receiver_kind.rs`](../../../backend/bluejs/src/vm/temporal/receiver_kind.rs) | The shared dispatcher's non-Temporal path touches this table without entering receiver-data algorithms. The mapping is preserved verbatim. |
| Host registration capacity | [`vm/host_registration.rs`](../../../backend/bluejs/src/vm/host_registration.rs) | Share checked u32 arithmetic and reserve the entire one/two-slot tag range before allocation or publication. Family validation, roots and installation errors remain in their producers. |
| Enumeration records | [`vm/for_in.rs`](../../../backend/bluejs/src/vm/for_in.rs) | Move for-in creation and stepping out of private-property/class metadata code. Existing traps, lifetime checks and cleanup remain unchanged. |
| Promise reactions | [`vm/promise_reactions.rs`](../../../backend/bluejs/src/vm/promise_reactions.rs) | Separate retained reaction registration from species/capability resolution. A pure record operation appends pending reactions or creates fulfilled/rejected jobs; invalid Promise ingress remains fallible. Then entry points retain receivers and handlers through allocation/species callbacks and restore their stack on either outcome. Record membership cannot disappear: production never removes records and every allocation root batch retains them. |
| Private declarations | [`vm/properties.rs`](../../../backend/bluejs/src/vm/properties.rs) | Reads and writes share one checked declaration lookup after receiver-brand validation. A real compiled class and an absent selector verify the identical TypeError; foreign heap ingress remains fallible. No compiler metadata is fabricated. |
| Intl adapters | [`vm/intl/list_format.rs`](../../../backend/bluejs/src/vm/intl/list_format.rs), [`vm/intl/duration_format.rs`](../../../backend/bluejs/src/vm/intl/duration_format.rs) | Independent ListFormat and DurationFormat owners preserve the original algorithms, provider errors and observable iteration/options. The former list_duration file is replaced by these owners. |
| Retained payload accounting | [`ast/retained_payload.rs`](../../../backend/bluejs/src/ast/retained_payload.rs) | One monomorphic checked accumulation helper preserves unavailable children and real usize overflow. The constant nonzero limb divisor is explicit; allocation-backed size products remain checked. |
| Disposal ingress | [`vm/builtins/resource_management.rs`](../../../backend/bluejs/src/vm/builtins/resource_management.rs) | Remove the optional explicit-method input that no production caller supplied. Nullish async resources, adopt/defer producers, suppression and actual callbacks retain their behavior. |

The shared-buffer/TypedArray interval proofs, remaining private-owner metadata proofs and Intl provider-wide impossibility reviews remain local follow-up work. This batch does not replace their public errors with assertions.

### Raw coverage of isolated owners

| Owner | Raw lines | Raw functions | Raw regions | Status |
| --- | --- | --- | --- | --- |
| [`heap/capabilities.rs`](../../../backend/bluejs/src/heap/capabilities.rs) | 23 / 23 | 2 / 2 | 26 / 26 | Complete |
| [`vm/classification.rs`](../../../backend/bluejs/src/vm/classification.rs) | 46 / 46 | 8 / 8 | 69 / 69 | Complete |
| [`vm/host_registration.rs`](../../../backend/bluejs/src/vm/host_registration.rs) | 15 / 15 | 4 / 4 | 22 / 22 | Complete |
| [`vm/temporal/receiver.rs`](../../../backend/bluejs/src/vm/temporal/receiver.rs) | 43 / 43 | 5 / 5 | 43 / 43 | Complete |
| [`vm/temporal/receiver_kind.rs`](../../../backend/bluejs/src/vm/temporal/receiver_kind.rs) | 24 / 24 | 1 / 1 | 25 / 25 | Complete |
| [`vm/for_in.rs`](../../../backend/bluejs/src/vm/for_in.rs) | 123 / 132 | 23 / 23 | 314 / 342 | Incomplete |
| [`vm/promise_reactions.rs`](../../../backend/bluejs/src/vm/promise_reactions.rs) | 33 / 33 | 2 / 2 | 37 / 37 | Complete |
| [`vm/intl/list_format.rs`](../../../backend/bluejs/src/vm/intl/list_format.rs) | 231 / 240 | 21 / 26 | 356 / 383 | Incomplete |
| [`vm/intl/duration_format.rs`](../../../backend/bluejs/src/vm/intl/duration_format.rs) | 362 / 378 | 26 / 29 | 535 / 574 | Incomplete |

Splitting files changes source denominators and does not, by itself, establish coverage improvement. Every new executable owner remains in the measured inventory and the modified-file acceptance set. Moved for-in and Intl gaps remain visible under their current owners.

## Test graph and UI

The shared-function registry records 26 function symbols across 10 boundaries and 212 lexical references. The dashboard displays the referencing file, enclosing function and line alongside measured test relationships. Browser verification covers the new panel, live run identity, desktop/mobile layouts and JavaScript errors.

Current observations retain source/test hashes, the frozen snapshot, case selection identity, executable/coverage hashes and profile directories. A complete owning harness retires obsolete paths into historical observations; partial runs preserve other owner paths. Only a successful complete engine inventory establishes a structural body contract. Type, signature, import, attribute, shared-literal and helper changes keep broad selection.

The following explicit-path probes describe prospective body-only changes against the current complete snapshot. They execute no tests and are not timing estimates or case counts. Function-body contracts use measured source relationships; lexical references remain conservative supplementary information rather than a complete Rust call graph.

| Isolated source | Selected Rust targets | Source impact Test262 selection | Verified body contract |
| --- | --- | --- | --- |
| [`vm/classification.rs`](../../../backend/bluejs/src/vm/classification.rs) | 296 | Full inventory | Yes |
| [`vm/temporal/receiver.rs`](../../../backend/bluejs/src/vm/temporal/receiver.rs) | 87 | Full inventory | Yes |
| [`vm/temporal/receiver_kind.rs`](../../../backend/bluejs/src/vm/temporal/receiver_kind.rs) | 300 | Full inventory | Yes |
| [`vm/host_registration.rs`](../../../backend/bluejs/src/vm/host_registration.rs) | 5 | None from source edges | Yes |
| [`vm/for_in.rs`](../../../backend/bluejs/src/vm/for_in.rs) | 173 | Full inventory | Yes |
| [`vm/promise_reactions.rs`](../../../backend/bluejs/src/vm/promise_reactions.rs) | 50 | Full inventory | Yes |
| [`vm/intl/list_format.rs`](../../../backend/bluejs/src/vm/intl/list_format.rs) | 7 | Full inventory | Yes |
| [`vm/intl/duration_format.rs`](../../../backend/bluejs/src/vm/intl/duration_format.rs) | 11 | Full inventory | Yes |
| [`vm/properties.rs`](../../../backend/bluejs/src/vm/properties.rs) | 312 | Full inventory | Yes |
| [`ast/retained_payload.rs`](../../../backend/bluejs/src/ast/retained_payload.rs) | 2 | None from source edges | Yes |

The final gate still runs full Test262. Very hot shared classification and dispatch contracts can legitimately select most Rust targets. Local receiver/formatter/reaction edits can use their smaller measured owner sets. Rust still recompiles a changed shared crate; selecting fewer owning harnesses reduces linking and execution work.

## Remaining difficulty and local work

Grades estimate lifecycle/producer/refusal complexity, with missing regions used only to order rows within a grade. D4 needs broad producer and embedding/provider contracts; D3 needs a dedicated state/refusal matrix; D2 has a bounded public fixture or local proof. Counts below come from the current raw LLVM summaries.

| Rank | Grade | Source | Missing lines / functions / regions | Remaining work |
| --- | --- | --- | --- | --- |
| 1 | D4 | [`vm/host_objects.rs`](../../../backend/bluejs/src/vm/host_objects.rs) | 94 / 12 / 103 | Listener mutation, cancellation, callback errors, quotas and registry publication rollback. |
| 2 | D4 | [`vm/builtins/native_dispatch/dispatch.rs`](../../../backend/bluejs/src/vm/builtins/native_dispatch/dispatch.rs) | 21 / 2 / 88 | Validated buffer/boxed/iterator reads, actual Promise refusal and locked BigInt to_f64 guarantee. |
| 3 | D4 | [`vm/properties.rs`](../../../backend/bluejs/src/vm/properties.rs) | 20 / 3 / 50 | Private brands/uninitialized fields, class homes, for-in traps, copy growth and cleanup. |
| 4 | D4 | [`vm/intl/duration_format.rs`](../../../backend/bluejs/src/vm/intl/duration_format.rs) | 16 / 3 / 39 | Duration property coercion, options, provider errors and real parts/resolved-options allocation refusal. |
| 5 | D4 | [`vm/for_in.rs`](../../../backend/bluejs/src/vm/for_in.rs) | 9 / 0 / 28 | Private record producer/lifetime review and observable Proxy own-key, descriptor and prototype behavior. |
| 6 | D4 | [`vm/intl/list_format.rs`](../../../backend/bluejs/src/vm/intl/list_format.rs) | 9 / 5 / 27 | List option coercion, provider errors, element/part correspondence and real result-allocation refusal. |
| 7 | D3 | [`vm/json.rs`](../../../backend/bluejs/src/vm/json.rs) | 12 / 0 / 41 | Raw JSON freezing, reviver/replacer/coercion, boxed/foreign data and parser lookahead. |
| 8 | D3 | [`vm/builtins/uint8array.rs`](../../../backend/bluejs/src/vm/builtins/uint8array.rs) | 15 / 1 / 25 | Base64 malformed tails/partial writes/progress, hex handling, cold prototype and encoding proof. |
| 9 | D3 | [`ast/retained_payload.rs`](../../../backend/bluejs/src/ast/retained_payload.rs) | 5 / 0 / 22 | Generated class AST shapes and raw generic-instantiation accounting. |
| 10 | D3 | [`vm/temporal/instant.rs`](../../../backend/bluejs/src/vm/temporal/instant.rs) | 6 / 6 / 20 | Epoch/range arithmetic, malformed timezone UTF-16 and duration allocation. |
| 11 | D3 | [`vm/temporal/plain_time.rs`](../../../backend/bluejs/src/vm/temporal/plain_time.rs) | 7 / 3 / 17 | Malformed UTF-16 rounding/digits options, bounded duration construction and formatter allocation. |
| 12 | D3 | [`vm/builtins/resource_management.rs`](../../../backend/bluejs/src/vm/builtins/resource_management.rs) | 5 / 2 / 15 | Nullish async await, suppression, fixed helper source, prototypes and successful retry. |
| 13 | D3 | [`vm/debugger/inspection.rs`](../../../backend/bluejs/src/vm/debugger/inspection.rs) | 2 / 0 / 10 | Real uncaptured binding preview, paused generations, TDZ and stale selectors. |
| 14 | D3 | [`vm/builtins/promise_core.rs`](../../../backend/bluejs/src/vm/builtins/promise_core.rs) | 0 / 0 / 7 | Species/capability re-entry, retained promise lookup and genuine reaction errors. |
| 15 | D3 | [`vm/temporal/conversion/zoned_conversion.rs`](../../../backend/bluejs/src/vm/temporal/conversion/zoned_conversion.rs) | 1 / 1 / 3 | Timezone/provider/locale options and validated epoch conversion. |
| 16 | D2 | [`parser/statements.rs`](../../../backend/bluejs/src/parser/statements.rs) | 27 / 0 / 40 | Public negative syntax, escaped labels, ASI, static block and for-head lookahead; no parser-wide weakening. |
| 17 | D2 | [`vm/test262/assertions.rs`](../../../backend/bluejs/src/vm/test262/assertions.rs) | 2 / 0 / 13 | Throwing message conversion, cold Test262Error initialization and exhaustive name dispatch. |
| 18 | D2 | [`vm/builtins/arrays.rs`](../../../backend/bluejs/src/vm/builtins/arrays.rs) | 0 / 0 / 12 | Cold ordinary-array creation refusal and callback observation order. |
| 19 | D2 | [`heap/object_storage.rs`](../../../backend/bluejs/src/heap/object_storage.rs) | 1 / 0 / 8 | Namespace/arguments cell ownership, Array length deletion and the retained test assertion region. |
| 20 | D2 | [`vm/builtins/numbers.rs`](../../../backend/bluejs/src/vm/builtins/numbers.rs) | 2 / 0 / 7 | Already-selected numeric formatting arms, finite/nonfinite formatting and precision coercion order. |
| 21 | D2 | [`vm/builtins/general.rs`](../../../backend/bluejs/src/vm/builtins/general.rs) | 0 / 0 / 6 | Intrinsic initialization and real coercion/get_method errors. |
| 22 | D2 | [`vm/functions.rs`](../../../backend/bluejs/src/vm/functions.rs) | 0 / 0 / 6 | Bind/hasInstance prototype callbacks, target roots and foreign/bound behavior. |
| 23 | D2 | [`vm/builtins/collections.rs`](../../../backend/bluejs/src/vm/builtins/collections.rs) | 0 / 0 / 5 | Custom constructors, mapper throws, iterator close and Array.from/of creation. |
| 24 | D2 | [`vm/builtins/collection_iteration.rs`](../../../backend/bluejs/src/vm/builtins/collection_iteration.rs) | 0 / 0 / 4 | Cold Function prototype/root refusal, owned-root cleanup and callback mutation. |
| 25 | D2 | [`compiler.rs`](../../../backend/bluejs/src/compiler.rs) | 0 / 0 / 3 | Root declaration name/slot metadata proof and real debugger metadata fixtures. |
| 26 | D2 | [`vm/test262/cases.rs`](../../../backend/bluejs/src/vm/test262/cases.rs) | 0 / 0 / 3 | Actual host callback and exhaustive URI boundaries; no rerun of unrelated exhaustive loops. |
| 27 | D2 | [`vm/builtins/native_dispatch/date.rs`](../../../backend/bluejs/src/vm/builtins/native_dispatch/date.rs) | 0 / 0 / 2 | ToPrimitive/toJSON callback and getter order, nonfinite and foreign Date behavior. |
| 28 | D2 | [`vm/builtins/set_methods.rs`](../../../backend/bluejs/src/vm/builtins/set_methods.rs) | 0 / 0 / 2 | Getter order, noncallable methods, foreign callables and iterator closing. |
| 29 | D2 | [`vm/temporal/dates/construction.rs`](../../../backend/bluejs/src/vm/temporal/dates/construction.rs) | 0 / 0 / 2 | Argument conversion with ordinary property bags and observable getter failures. |
| 30 | D2 | [`vm/temporal/conversion/calendar_fields.rs`](../../../backend/bluejs/src/vm/temporal/conversion/calendar_fields.rs) | 0 / 0 / 1 | Calendar arguments, missing required calendar and getter ordering. |

Prepare each local correction batch and its public refusal/reuse fixtures before executing related Rust partitions and conformance families. Run the complete frozen inventory once after those pass. Continue requiring all three raw metrics for every modified production file, preservation of previously complete files and zero semantic/percentage regressions. The overall 100% modified-file goal remains open wherever the current report says Incomplete.

## Evidence

- [Current report](TEST262_MACOS_REPORT.md)
- [Machine-readable common boundaries and remaining debt](COVERAGE_SHARED_FUNCTION_IMPACT.json)
- [Full measurement](../../../target/bluejs-selftest/runs/20261007-173453-3cd81e5f/report-data.json)
- [Common-boundary acceptance audit](../../../target/bluejs-selftest/runs/20261007-173453-3cd81e5f/common-boundary-acceptance-audit.json)
- [Maximum-instantiation gap diagnostic](../../../target/bluejs-selftest/runs/20261007-173453-3cd81e5f/best-instantiation-gaps.json)
- [Self-test workflow and structural contracts](../../../backend/bluejs/selftest/README.md)
