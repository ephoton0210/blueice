# BlueJS Coverage Batch Analysis

## Current verified source

Measurement: `2026-10-08T08:31:19.980982+00:00`. Run: `20261008-154319-0641e2d9`.
Source fingerprint: `ffee5cd5ed8ea212b581b06a304f35e6073e0c4818c80388f8661992435303c9`.

The current inventory has **153/169 complete production files**. The remaining 16 files miss 146 raw lines, 13 functions and 249 regions.

All 150 files complete before this batch remain complete, including the 95 original files, all 22 selected D5/D4 files and all 68 preceding modified production files. Semantic outcome changes and per-file coverage percentage regressions are zero.

| Verification gate | Current result |
| --- | --- |
| BlueJS Rust | 334 targets; 4,135 passed, zero failed, 4 existing ignored |
| Complete Test262 | 102,921 applicable modes passed; 4 excluded and 1 stale modes |
| Semantic comparison | 102,926 contracts; zero changes |
| Workspace runtime tests | 2,170 passed, 0 failed, 60 ignored |
| BlueJS Rustdoc tests | 2 passed, 0 failed, 0 ignored |
| Self-test tooling contracts | 84 passed, 0 failed, 0 ignored |
| Static checks | Workspace Clippy with warnings denied; 120 task Rust files formatted; diff check passed |

Related unit and ordinary-library boundary partitions passed before this complete run. All corrections in each batch preceded verification. Completion uses only this full run's fresh atomic LLVM profiles and executable maps. Function instantiations use LLVM maxima, never complementary-branch unions.

## Implemented common boundaries

| Boundary | Owner | Contract and independent consumers |
| --- | --- | --- |
| Capability classification | [`heap/capabilities.rs`](../../../backend/bluejs/src/heap/capabilities.rs), [`vm/classification.rs`](../../../backend/bluejs/src/vm/classification.rs) | One fallible heap lookup; immutable callable/constructible/HTMLDDA bits. Foreign/reverse overrides and revoked-Proxy capabilities remain intact. No callbacks or managed allocation. Public method lookup stays fallible. |
| Temporal receiver data | [`vm/temporal/receiver.rs`](../../../backend/bluejs/src/vm/temporal/receiver.rs) | Exact native brand validation precedes observable argument access. Instant, PlainTime, withCalendar and zoned algorithms consume an owned slot snapshot. Arguments retain separate property-bag/coercion/range/provider errors. |
| Temporal native kind mapping | [`vm/temporal/receiver_kind.rs`](../../../backend/bluejs/src/vm/temporal/receiver_kind.rs) | The shared dispatcher's non-Temporal path touches this table without entering receiver-data algorithms. The mapping is preserved verbatim. |
| Host registration capacity | [`vm/host_registration.rs`](../../../backend/bluejs/src/vm/host_registration.rs) | Share checked u32 arithmetic and reserve the entire one/two-slot tag range before allocation or publication. Family validation, roots and installation errors remain in their producers. |
| Enumeration records | [`vm/for_in.rs`](../../../backend/bluejs/src/vm/for_in.rs) | Move for-in creation and stepping out of private-property/class metadata code. Private records retain their visited/chain/key objects across Proxy calls. Existing non-payload slot replacements cannot grow storage; actual metadata creation, chain/visited growth and Proxy errors remain fallible. Foreign heap ingress and temporary-root cleanup are verified. |
| Promise reactions | [`vm/promise_reactions.rs`](../../../backend/bluejs/src/vm/promise_reactions.rs) | Separate retained reaction registration from species/capability resolution. A pure record operation appends pending reactions or creates fulfilled/rejected jobs; invalid Promise ingress remains fallible. Then entry points retain receivers and handlers through allocation/species callbacks and restore their stack on either outcome. Record membership cannot disappear: production never removes records and every allocation root batch retains them. |
| Private declarations | [`vm/properties.rs`](../../../backend/bluejs/src/vm/properties.rs) | Reads and writes share one checked declaration lookup after receiver-brand validation. A real compiled class and an absent selector verify the identical TypeError; foreign heap ingress remains fallible. No compiler metadata is fabricated. |
| Intl adapters | [`vm/intl/list_format.rs`](../../../backend/bluejs/src/vm/intl/list_format.rs), [`vm/intl/duration_format.rs`](../../../backend/bluejs/src/vm/intl/duration_format.rs) | All 23 functions formerly in list_duration map to these complete owners. Observable options, coercions, iterator close, duration record validation and real allocations stay fallible. Embedded list data, infallible String collectors and already-validated duration records supply the private guarantees. List element mapping follows empty-element elision without shifting original UTF-16 values; constructors and resolvedOptions restore temporary roots on either outcome. |
| Retained payload accounting | [`ast/retained_payload.rs`](../../../backend/bluejs/src/ast/retained_payload.rs) | One monomorphic checked accumulation helper preserves unavailable children and real usize overflow. The constant nonzero limb divisor is explicit; allocation-backed Vec byte capacities have a representable layout; aggregate child sizes remain checked. |
| Disposal ingress | [`vm/builtins/resource_management.rs`](../../../backend/bluejs/src/vm/builtins/resource_management.rs) | Remove the optional explicit-method input that no production caller supplied. Nullish async resources, adopt/defer producers, suppression and actual callbacks retain their behavior. |

The for-in, ListFormat and DurationFormat owners all meet 100% raw lines, functions and regions. The retained ownership audit maps every former list_duration function to a current complete owner. Their public error protocols and adaptive allocation-refusal/retry matrix run in both unit and ordinary-library builds.

### Raw coverage of isolated owners

| Owner | Raw lines | Raw functions | Raw regions | Status |
| --- | --- | --- | --- | --- |
| [`heap/capabilities.rs`](../../../backend/bluejs/src/heap/capabilities.rs) | 23 / 23 | 2 / 2 | 26 / 26 | Complete |
| [`vm/classification.rs`](../../../backend/bluejs/src/vm/classification.rs) | 46 / 46 | 8 / 8 | 69 / 69 | Complete |
| [`vm/host_registration.rs`](../../../backend/bluejs/src/vm/host_registration.rs) | 15 / 15 | 4 / 4 | 22 / 22 | Complete |
| [`vm/temporal/receiver.rs`](../../../backend/bluejs/src/vm/temporal/receiver.rs) | 43 / 43 | 5 / 5 | 43 / 43 | Complete |
| [`vm/temporal/receiver_kind.rs`](../../../backend/bluejs/src/vm/temporal/receiver_kind.rs) | 24 / 24 | 1 / 1 | 25 / 25 | Complete |
| [`vm/for_in.rs`](../../../backend/bluejs/src/vm/for_in.rs) | 183 / 183 | 23 / 23 | 364 / 364 | Complete |
| [`vm/promise_reactions.rs`](../../../backend/bluejs/src/vm/promise_reactions.rs) | 33 / 33 | 2 / 2 | 37 / 37 | Complete |
| [`vm/intl/list_format.rs`](../../../backend/bluejs/src/vm/intl/list_format.rs) | 251 / 251 | 26 / 26 | 395 / 395 | Complete |
| [`vm/intl/duration_format.rs`](../../../backend/bluejs/src/vm/intl/duration_format.rs) | 391 / 391 | 29 / 29 | 586 / 586 | Complete |

Splitting files changes source denominators and does not, by itself, establish coverage improvement. Every new executable owner remains in the measured inventory and the modified-file acceptance set. All three separated algorithm owners are now complete. The legacy combined file stays removed; its 23 functions are measured through their current owners.

## Test graph and UI

The shared-function registry records 44 function symbols across 15 boundaries and 353 lexical references. The dashboard displays the referencing file, enclosing function and line alongside measured test relationships. Browser verification covers the new panel, live run identity, desktop/mobile layouts and JavaScript errors.

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
| [`vm/intl/list_format.rs`](../../../backend/bluejs/src/vm/intl/list_format.rs) | 8 | Full inventory | Yes |
| [`vm/intl/duration_format.rs`](../../../backend/bluejs/src/vm/intl/duration_format.rs) | 12 | Full inventory | Yes |
| [`vm/properties.rs`](../../../backend/bluejs/src/vm/properties.rs) | 312 | Full inventory | Yes |
| [`ast/retained_payload.rs`](../../../backend/bluejs/src/ast/retained_payload.rs) | 2 | None from source edges | Yes |
| [`vm/host_objects.rs`](../../../backend/bluejs/src/vm/host_objects.rs) | 3 | None from source edges | No; broad fallback |
| [`vm/builtins/native_dispatch/dispatch.rs`](../../../backend/bluejs/src/vm/builtins/native_dispatch/dispatch.rs) | 334 | Full inventory | No; broad fallback |
| [`vm/json.rs`](../../../backend/bluejs/src/vm/json.rs) | 79 | Full inventory | No; broad fallback |
| [`vm/builtins/uint8array.rs`](../../../backend/bluejs/src/vm/builtins/uint8array.rs) | 7 | Full inventory | No; broad fallback |
| [`vm/temporal/instant.rs`](../../../backend/bluejs/src/vm/temporal/instant.rs) | 17 | Full inventory | No; broad fallback |
| [`vm/temporal/plain_time.rs`](../../../backend/bluejs/src/vm/temporal/plain_time.rs) | 40 | Full inventory | No; broad fallback |
| [`vm/builtins/resource_management.rs`](../../../backend/bluejs/src/vm/builtins/resource_management.rs) | 18 | Full inventory | No; broad fallback |
| [`vm/debugger/inspection.rs`](../../../backend/bluejs/src/vm/debugger/inspection.rs) | 3 | None from source edges | No; broad fallback |
| [`vm/builtins/promise_core.rs`](../../../backend/bluejs/src/vm/builtins/promise_core.rs) | 66 | Full inventory | Yes |
| [`vm/temporal/conversion/zoned_conversion.rs`](../../../backend/bluejs/src/vm/temporal/conversion/zoned_conversion.rs) | 15 | Full inventory | No; broad fallback |

The final gate still runs full Test262. Very hot shared classification and dispatch contracts can legitimately select most Rust targets. Local receiver/formatter/reaction edits can use their smaller measured owner sets. Rust still recompiles a changed shared crate; selecting fewer owning harnesses reduces linking and execution work.

## D3/D4 coupling and preparation order

The current complete measurement leaves 4 D3/D4 owners (1 D4 and 3 D3), with 116 missing lines, 13 functions and 160 regions. The shared correction batch has passed full Rust, Test262, workspace and raw coverage preservation gates. Every one of its 15 changed production owners is complete. The production inventory remains 169 files.

### Shared boundaries completed before local work

| Boundary | Current owner | Verified contract |
| --- | --- | --- |
| Native routing and Promise algorithms | `native_dispatch/dispatch.rs`, `promise_core.rs` | Resolving functions and async-from-sync algorithms delegate to Promise ownership. Initial flag publication, thenables/species and real allocation refusal remain fallible. |
| Cross-type Temporal options and bounds | `conversion/options.rs`, `epoch.rs` | Preserve observation/coercion order and malformed UTF-16 errors; bounded conversions follow all supported Temporal producers. |
| General IsArray ownership | `builtins/arrays.rs` | JSON delegates general array classification. Nested/revoked Proxies and foreign heap ingress retain their errors. |
| Promise and asynchronous disposal | `promise_core.rs`, `resource_management.rs` | Preserve settlement, suppression and method-less asynchronous await ticks; cold intrinsic initialization precedes unpublished disposal prototypes. |
| Class, private binding and property producers | `properties.rs` | Real compiler installation order supplies retained class metadata and private owner cells. Receiver validation, callbacks, closure-home growth and copy refusals retain their errors. |

### Remaining prerequisites and independent batches

| Priority | Owner | Prerequisite and local verification |
| --- | --- | --- |
| P1 | `vm/host_objects.rs` | Cold family creation does not initialize Function; method, pair and click installers need an explicit fallible prerequisite. Verify second-click-method rollback, exact key/receiver identity, quotas, listener mutation, cancellation, callback errors, root/heap refusal and reuse. Use one callback type across a generic installer matrix. |
| P1 | `vm/builtins/uint8array.rs` | Root the unpublished buffer through cold prototype creation. Options getters can detach/resize a receiver, so post-options validity remains fallible. Validate read/write spans before the callback-free byte helpers; preserve malformed tails, partial progress and encoder options. |
| P2 | `vm/debugger/inspection.rs` | A public root pause can use uninstalled bytecode, so its missing-generation error is reachable. Nested installed generations, retained ordinary cells and validated active slot bounds have separate producer contracts. Verify TDZ, stale selectors, linked frames and reuse without evaluating getters. |
| P2 | `ast/retained_payload.rs` | Vector allocation bounds and the pinned BigInt digit iterator support local size proofs. Keep child refusal and checked accumulation; cover real owned class initializer variants without executing fabricated VM state. |

These four owners can now progress independently. They use existing complete receiver/storage, registration, compiler and heap boundaries; no additional production module split is required. Calendar identifier foreign ingress also remains in the cumulative modified-file acceptance set. Prepare all source and fixtures in the next batch before related partitions; run one fresh complete inventory after they pass.

## Remaining difficulty and local work

Grades estimate lifecycle/producer/refusal complexity, with missing regions used only to order rows within a grade. D4 needs broad producer and embedding/provider contracts; D3 needs a dedicated state/refusal matrix; D2 has a bounded public fixture or local proof. Counts below come from the current raw LLVM summaries.

| Rank | Grade | Source | Missing lines / functions / regions | Remaining work |
| --- | --- | --- | --- | --- |
| 1 | D4 | [`vm/host_objects.rs`](../../../backend/bluejs/src/vm/host_objects.rs) | 94 / 12 / 103 | Listener mutation, cancellation, callback errors, quotas and registry publication rollback. |
| 2 | D3 | [`vm/builtins/uint8array.rs`](../../../backend/bluejs/src/vm/builtins/uint8array.rs) | 15 / 1 / 25 | Base64 malformed tails/partial writes/progress, hex handling, cold prototype and encoding proof. |
| 3 | D3 | [`ast/retained_payload.rs`](../../../backend/bluejs/src/ast/retained_payload.rs) | 5 / 0 / 22 | Generated class AST shapes and raw generic-instantiation accounting. |
| 4 | D3 | [`vm/debugger/inspection.rs`](../../../backend/bluejs/src/vm/debugger/inspection.rs) | 2 / 0 / 10 | Real uncaptured binding preview, paused generations, TDZ and stale selectors. |
| 5 | D2 | [`parser/statements.rs`](../../../backend/bluejs/src/parser/statements.rs) | 27 / 0 / 40 | Public negative syntax, escaped labels, ASI, static block and for-head lookahead; no parser-wide weakening. |
| 6 | D2 | [`vm/test262/assertions.rs`](../../../backend/bluejs/src/vm/test262/assertions.rs) | 2 / 0 / 13 | Throwing message conversion, cold Test262Error initialization and exhaustive name dispatch. |
| 7 | D2 | [`heap/object_storage.rs`](../../../backend/bluejs/src/heap/object_storage.rs) | 1 / 0 / 8 | Namespace/arguments cell ownership, Array length deletion and the retained test assertion region. |
| 8 | D2 | [`vm/functions.rs`](../../../backend/bluejs/src/vm/functions.rs) | 0 / 0 / 6 | Bind/hasInstance prototype callbacks, target roots and foreign/bound behavior. |
| 9 | D2 | [`vm/builtins/collections.rs`](../../../backend/bluejs/src/vm/builtins/collections.rs) | 0 / 0 / 5 | Custom constructors, mapper throws, iterator close and Array.from/of creation. |
| 10 | D2 | [`vm/builtins/collection_iteration.rs`](../../../backend/bluejs/src/vm/builtins/collection_iteration.rs) | 0 / 0 / 4 | Cold Function prototype/root refusal, owned-root cleanup and callback mutation. |
| 11 | D2 | [`compiler.rs`](../../../backend/bluejs/src/compiler.rs) | 0 / 0 / 3 | Root declaration name/slot metadata proof and real debugger metadata fixtures. |
| 12 | D2 | [`vm/test262/cases.rs`](../../../backend/bluejs/src/vm/test262/cases.rs) | 0 / 0 / 3 | Actual host callback and exhaustive URI boundaries; no rerun of unrelated exhaustive loops. |
| 13 | D2 | [`vm/builtins/native_dispatch/date.rs`](../../../backend/bluejs/src/vm/builtins/native_dispatch/date.rs) | 0 / 0 / 2 | ToPrimitive/toJSON callback and getter order, nonfinite and foreign Date behavior. |
| 14 | D2 | [`vm/builtins/set_methods.rs`](../../../backend/bluejs/src/vm/builtins/set_methods.rs) | 0 / 0 / 2 | Getter order, noncallable methods, foreign callables and iterator closing. |
| 15 | D2 | [`vm/temporal/dates/construction.rs`](../../../backend/bluejs/src/vm/temporal/dates/construction.rs) | 0 / 0 / 2 | Argument conversion with ordinary property bags and observable getter failures. |
| 16 | D2 | [`vm/temporal/conversion/calendar_fields.rs`](../../../backend/bluejs/src/vm/temporal/conversion/calendar_fields.rs) | 0 / 0 / 1 | Calendar arguments, missing required calendar and getter ordering. |

Prepare each local correction batch and its public refusal/reuse fixtures before executing related Rust partitions and conformance families. Run the complete frozen inventory once after those pass. Continue requiring all three raw metrics for every modified production file, preservation of previously complete files and zero semantic/percentage regressions. The overall 100% modified-file goal remains open wherever the current report says Incomplete.

## Evidence

- [Current report](TEST262_MACOS_REPORT.md)
- [Machine-readable common boundaries and remaining debt](COVERAGE_SHARED_FUNCTION_IMPACT.json)
- [Full measurement](../../../target/bluejs-selftest/runs/20261008-154319-0641e2d9/report-data.json)
- [Shared coupling acceptance audit](../../../target/bluejs-selftest/runs/20261008-154319-0641e2d9/shared-coupling-acceptance-audit.json)
- [Maximum-instantiation gap diagnostic](../../../target/bluejs-selftest/runs/20261008-154319-0641e2d9/best-instantiation-gaps.json)
- [Self-test workflow and structural contracts](../../../backend/bluejs/selftest/README.md)
