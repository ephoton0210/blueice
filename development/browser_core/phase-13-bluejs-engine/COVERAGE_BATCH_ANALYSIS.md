# BlueJS Coverage Batch Analysis

## Current verified source

Measurement: `2026-10-08T10:11:43.221647+00:00`. Run: `20261008-165752-6640389a`.
Source fingerprint: `d74e94b3251170b33fe8f24c7ab6728e5d408264b54d583fe2b5961fc9780a6b`.

The current inventory has **158/169 complete production files**. The remaining 11 files miss 30 raw lines, 0 functions and 88 regions.

All 153 files complete before this batch remain complete, including the 95 original files, all 22 selected D5/D4 files and all 75 previously complete modified production files. Semantic outcome changes and per-file coverage percentage regressions are zero.

| Verification gate | Current result |
| --- | --- |
| BlueJS Rust | 334 targets; 4,148 passed, zero failed, 4 existing ignored |
| Complete Test262 | 102,921 applicable modes passed; 4 excluded modes and 1 stale mode |
| Semantic comparison | 102,926 contracts; zero changes |
| Workspace runtime tests | 2,170 passed, 0 failed, 60 ignored |
| BlueJS Rustdoc tests | 2 passed, 0 failed, 0 ignored |
| Self-test tooling contracts | 85 passed, 0 failed, 0 ignored |
| Static checks | Workspace Clippy with warnings denied; 123 task Rust files formatted; diff check passed |

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
| Retained payload accounting | [`ast/retained_payload.rs`](../../../backend/bluejs/src/ast/retained_payload.rs) | One monomorphic checked accumulation helper preserves unavailable children and real usize overflow. The pinned logical BigInt limb iterator preserves zero/sign/u32 boundaries; allocation-backed Vec byte capacities have a representable layout; aggregate child sizes remain checked. |
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

The shared-function registry records 44 function symbols across 15 boundaries and 353 lexical references. The dashboard displays the referencing file, enclosing function and line alongside measured test relationships. The retained dashboard uses the current run identity and graph evidence.

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
| [`vm/builtins/uint8array.rs`](../../../backend/bluejs/src/vm/builtins/uint8array.rs) | 8 | Full inventory | No; broad fallback |
| [`vm/temporal/instant.rs`](../../../backend/bluejs/src/vm/temporal/instant.rs) | 17 | Full inventory | No; broad fallback |
| [`vm/temporal/plain_time.rs`](../../../backend/bluejs/src/vm/temporal/plain_time.rs) | 40 | Full inventory | No; broad fallback |
| [`vm/builtins/resource_management.rs`](../../../backend/bluejs/src/vm/builtins/resource_management.rs) | 18 | Full inventory | No; broad fallback |
| [`vm/debugger/inspection.rs`](../../../backend/bluejs/src/vm/debugger/inspection.rs) | 3 | None from source edges | No; broad fallback |
| [`vm/builtins/promise_core.rs`](../../../backend/bluejs/src/vm/builtins/promise_core.rs) | 66 | Full inventory | Yes |
| [`vm/temporal/conversion/zoned_conversion.rs`](../../../backend/bluejs/src/vm/temporal/conversion/zoned_conversion.rs) | 15 | Full inventory | No; broad fallback |

The final gate still runs full Test262. Very hot shared classification and dispatch contracts can legitimately select most Rust targets. Local receiver/formatter/reaction edits can use their smaller measured owner sets. Rust still recompiles a changed shared crate; selecting fewer owning harnesses reduces linking and execution work.

## D3/D4 coupling and producer contracts

The shared and independent D3/D4 owners are complete in the current full measurement. Their prerequisites are owned by existing modules; the production inventory remains 169 files. No additional production split was needed.

### Shared prerequisites and order

The following shared contracts required preparation before local D3/D4 work. Priority reflects the breadth of ownership and observable behavior. Every listed owner is complete in the current measurement.

| Priority | Shared owners | Coupling and verified contract | Current status |
| --- | --- | --- | --- |
| P0 | `vm/builtins/native_dispatch/dispatch.rs`, `vm/builtins/promise_core.rs` | Promise resolving functions and async-from-sync completion now belong to promise_core; dispatch delegates. The first AlreadyResolved flag write remains fallible. | Verified complete |
| P1 | `vm/temporal/conversion/options.rs`, `vm/temporal/epoch.rs` | Shared option readers and validators belong to conversion/options; producer-validated epoch conversions belong to epoch. | Verified complete |
| P1 | `vm/builtins/arrays.rs` | VM IsArray now belongs to arrays, retaining nested/revoked Proxy and foreign heap ingress errors. | Verified complete |
| P1 | `vm/builtins/promise_core.rs`, `vm/builtins/resource_management.rs` | Settlement/refusal and cold disposal initialization are verified together, including nullish async await ticks and suppression. | Verified complete |
| P1 | `vm/properties.rs` | Class homes precede heritage and field metadata; private owner cells are retained compiler bindings. Actual metadata growth, receiver ingress, callbacks and copy refusals remain fallible. | Verified complete |

The four local owners below retain these prerequisites. Their local state and refusal fixtures can be selected through the graph; changes to shared signatures or data continue to expand the affected selection.

| Owner | Verified producer and observable boundaries |
| --- | --- |
| `vm/host_objects.rs` | Cold method, pair and click installation initializes Function through a fallible prerequisite. A refusal while installing the second click method rolls back the first configurable publication so the same family can retry. Opaque same-heap families belong to append-only VM registries; callbacks accept primitive values and cannot reenter exclusive host-authorized dispatch. Listener mutation, cancellation, exact receivers, quotas, callback errors, root/heap/instruction/string refusal and reuse are covered. |
| `vm/builtins/uint8array.rs` | New backing buffers stay on the operand stack while cold prototypes and views allocate. Brand ingress, immutable writes and post-options buffer validity stay fallible. Validated byte reads and normalized writes execute no callbacks or managed allocation. Malformed tails, partial progress, encoders, cold nurseries and allocation/refusal cleanup are covered. |
| `vm/debugger/inspection.rs` | Public uninstalled root bytecode retains its missing-generation refusal. Installed child generations, retained ordinary capture cells and validated active scope slots supply separate private guarantees. Real uncaptured TDZ and initialized previews do not resume execution. Linked/module/stale selector and budget contracts remain covered. |
| `ast/retained_payload.rs` | Vec byte capacity follows the real allocation layout. Pinned BigInt logical u32 limbs preserve zero, sign and word boundaries without copying. Child refusal and checked aggregate overflow remain part of the accounting protocol. Real owned class initializer AST shapes are counted without executing fabricated VM state. |

The cumulative calendar identifier ingress gap is also complete. A genuine foreign heap object retains its error; real Temporal receiver slots bypass throwing user calendar getters. Shared Promise settlement, Temporal options/epoch conversion, IsArray and class/private metadata owners remain complete.

### Verification and test cost

Related partitions passed 27 Rust cases and 85 tooling contracts before one fresh full inventory. Only full-stage engine profiles establish completion. Partition diagnostics, readiness probes, workspace runs and prior failed/cancelled profiles are excluded.

The graph selects every case in the independent fixture and its ordinary-library wrapper when shared fixture helpers change. A same-source passed exact partition can retire only its actually observed earlier failures when native executable hashes and pass counts match. Changed source/artifacts, different selections, missing logs and zero/ignored cases keep the remaining failure prerequisite. The final full stage runs without repeating the affected stage.

A reproduced compiler wait occurred in macOS dylib signature validation while loading displaydoc. Its SHA-256-identical copy loaded successfully; the original inode and sampled stack were retained before the identical cache bytes were restored through a fresh inode. This recovery ran no Rust tests and changed no production source or compiler settings. Workspace native entry copies are also checked byte-for-byte. The workspace gate completed once. Retained `_dyld_start` samples identify waits before Rust initialization; no workspace test cases were interrupted or repeated. Gate durations are wall time, including process entry waits.

Remaining coverage debt is listed below from this same complete measurement. These untouched local owners can proceed through the existing graph and contracts.

## Remaining difficulty and local work

Grades estimate lifecycle/producer/refusal complexity, with missing regions used only to order rows within a grade. D4 needs broad producer and embedding/provider contracts; D3 needs a dedicated state/refusal matrix; D2 has a bounded public fixture or local proof. Counts below come from the current raw LLVM summaries.

| Rank | Grade | Source | Missing lines / functions / regions | Remaining work |
| --- | --- | --- | --- | --- |
| 1 | D2 | [`parser/statements.rs`](../../../backend/bluejs/src/parser/statements.rs) | 27 / 0 / 40 | Public negative syntax, escaped labels, ASI, static block and for-head lookahead; no parser-wide weakening. |
| 2 | D2 | [`vm/test262/assertions.rs`](../../../backend/bluejs/src/vm/test262/assertions.rs) | 2 / 0 / 13 | Throwing message conversion, cold Test262Error initialization and exhaustive name dispatch. |
| 3 | D2 | [`heap/object_storage.rs`](../../../backend/bluejs/src/heap/object_storage.rs) | 1 / 0 / 8 | Namespace/arguments cell ownership, Array length deletion and the retained test assertion region. |
| 4 | D2 | [`vm/functions.rs`](../../../backend/bluejs/src/vm/functions.rs) | 0 / 0 / 6 | Bind/hasInstance prototype callbacks, target roots and foreign/bound behavior. |
| 5 | D2 | [`vm/builtins/collections.rs`](../../../backend/bluejs/src/vm/builtins/collections.rs) | 0 / 0 / 5 | Custom constructors, mapper throws, iterator close and Array.from/of creation. |
| 6 | D2 | [`vm/builtins/collection_iteration.rs`](../../../backend/bluejs/src/vm/builtins/collection_iteration.rs) | 0 / 0 / 4 | Cold Function prototype/root refusal, owned-root cleanup and callback mutation. |
| 7 | D2 | [`compiler.rs`](../../../backend/bluejs/src/compiler.rs) | 0 / 0 / 3 | Root declaration name/slot metadata proof and real debugger metadata fixtures. |
| 8 | D2 | [`vm/test262/cases.rs`](../../../backend/bluejs/src/vm/test262/cases.rs) | 0 / 0 / 3 | Actual host callback and exhaustive URI boundaries; no rerun of unrelated exhaustive loops. |
| 9 | D2 | [`vm/builtins/native_dispatch/date.rs`](../../../backend/bluejs/src/vm/builtins/native_dispatch/date.rs) | 0 / 0 / 2 | ToPrimitive/toJSON callback and getter order, nonfinite and foreign Date behavior. |
| 10 | D2 | [`vm/builtins/set_methods.rs`](../../../backend/bluejs/src/vm/builtins/set_methods.rs) | 0 / 0 / 2 | Getter order, noncallable methods, foreign callables and iterator closing. |
| 11 | D2 | [`vm/temporal/dates/construction.rs`](../../../backend/bluejs/src/vm/temporal/dates/construction.rs) | 0 / 0 / 2 | Argument conversion with ordinary property bags and observable getter failures. |

Prepare each local correction batch and its public refusal/reuse fixtures before executing related Rust partitions and conformance families. Run the complete frozen inventory once after those pass. Continue requiring all three raw metrics for every modified production file, preservation of previously complete files and zero semantic/percentage regressions. The current report states completion separately from untouched remaining coverage debt.

## Evidence

- [Current report](TEST262_MACOS_REPORT.md)
- [Machine-readable common boundaries and remaining debt](COVERAGE_SHARED_FUNCTION_IMPACT.json)
- [Full measurement](../../../target/bluejs-selftest/runs/20261008-165752-6640389a/report-data.json)
- [Independent boundary acceptance audit](../../../target/bluejs-selftest/runs/20261008-165752-6640389a/independent-boundary-acceptance-audit.json)
- [Maximum-instantiation gap diagnostic](../../../target/bluejs-selftest/runs/20261008-165752-6640389a/best-instantiation-gaps.json)
- [Current workspace startup observations](../../../target/bluejs-selftest/runs/20261008-165752-6640389a/workspace-startup-observations.json)
- [Self-test workflow and structural contracts](../../../backend/bluejs/selftest/README.md)
