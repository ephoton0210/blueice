# Test262 architecture-first implementation backlog

Requested 2026-09-10. Status: **full inventory measured; full conformance remains open**.
This is the implementation order for the [edition 17 track](ECMASCRIPT_2026.md).
The [recorded analysis](TEST262_ANALYSIS_REPORT.md) includes all workstream counts,
common diagnostics, representative cases and executable hashes.
The existing `test262-summary.json` only groups outcomes by top-level directory
and feature. It does not establish failure root causes or architectural priority.

## Evidence and classification contract

The verified snapshot is `6eec1ac9ee144dafd8f344d73a21f36bfc9f6755`:
53,404 test files, 279 fixture resources, 102,578 execution modes. The baseline
for this work includes the comma/void changes now committed as `401eb8e`:
16,216 pass, 62,701 fail, 22,736 unsupported, 925 timeout, zero harness errors.
It was run with 8 workers, 100,000 instructions and a 2-second case deadline.
The runner exited 1, as expected for a non-passing full inventory.

The new [analyzer](../../../backend/bluejs/test262/analyze.py) reconciles the
summary, unique path/mode pairs, source hashes, metadata and full source inventory
before publishing a report. `items.jsonl` retains **every mode**, its original
outcome/diagnostic, `esid`, description, flags, features and includes, together
with a target workstream, priority, prerequisites and observed blocker.
Targets are inferred from paths/metadata; diagnostics identify the **first
observed symptom**, not every root cause. Dependencies overlap; exclusive target
totals reconcile to the full denominator. Passed negative tests are not errors.

The baseline has 39,447 unclassified parser rejections, 14,768 unresolved-name
errors, 10,160 TypeErrors, 9,166 unsupported harness prerequisites and 3,955
unsupported compiler cases. These symptoms often hide later failures. In
particular, a failed Array test does not establish that its Array algorithm is
the first missing dependency. The generic unsupported-statement diagnostic
covers several AST kinds and must not be labelled as a confirmed try/catch bug.

The pinned `INTERPRETING.md` requires isolated realms, same-realm harness
includes, exact strict/raw/module/async behavior and negative phase/type
matching. Adapter runtime errors currently do not identify whether an include
or the test body threw; treat those cases as requiring reproduction. All
staging, Annex B, ECMA-402 and host-dependent cases remain in the inventory.
Their applicability needs an explicit clause/edition audit, never a silent skip.
Instruction exhaustion (906 baseline modes) and wall/regex deadlines (19) are
separate from semantic failures and should be rerun with recorded budgets.

## Dependency order and exit criteria

| Order | Architecture / test items | Dependencies and acceptance criteria |
| --- | --- | --- |
| P0.1 | Completion records and iterator lifetime: `language/statements/{try,throw,return,break,continue,for-of,switch}`, `language/**/dstr`, nested abrupt evaluation | Distinguish normal/throw/return/break/continue, empty/value completion and label targets; keep host resource aborts distinct. Preserve original thrown values and reverse-order cleanup. Iterator `next`, `done`, `value` errors mark done; elisions never read value; defaults/assignment failures close active iterators. Add catch/finally handlers with stack/scope restoration and rooted pending completions before suspension support. |
| P0.2 | Environments and calls: `language/{global-code,eval-code,arguments-object}`, function/default/rest parameter tests | Build on completion propagation. Persistent global object + declarative records, declaration instantiation even on abrupt script exit, TDZ, parameter/body separation, captured cells, mapped/unmapped arguments, direct/indirect eval and this/new.target. Verify multiple scripts in one realm and independent realms. |
| P0.3 | References and abstract operations: assignment/update/delete/call, computed members, destructuring targets, conversion order | Build on environments. Evaluate and retain references before RHS/value/default evaluation as each production requires; GetValue/PutValue and receiver identity must survive side effects and GC. Add per-production traces and strict/sloppy error-precedence tests. |
| P0.4 | Object internal methods and callable/constructor contracts: Object/Reflect/Proxy prerequisites | Build on references. Complete descriptors, receiver-aware Set/Get, own keys, extensibility, arrays/string exotics, call/construct/newTarget and internal slots. Route later Proxy traps through these same contracts and retain GC barriers. |
| P1.1 | Grammar and early-error taxonomy: Unicode identifiers/escapes, reserved words, strict code, labels, remaining operators | Fix foundational early errors needed by P0 in their own slice; then finish the wider grammar. Separate unsupported grammar from proven SyntaxError so parse negatives cannot pass accidentally. |
| P1.2 | Classes/private names/super/derived constructors | Object contracts, environments and grammar first; private brands, initialization ordering and derived this state need internal slots and call frames. |
| P1.3 | Generators, async functions/iteration, Promise jobs | Rooted resumable frames and completion propagation first. Specify resume/throw/return and microtask ordering; test cleanup across suspension. |
| P1.4 | Modules, import/export, dynamic import, top-level await | Persistent environments, live bindings and async jobs first. Test parsing, resolution, linking and evaluation phases independently, cycles and fixture resolution. |
| P1.5 | BigInt, ArrayBuffer/DataView/typed arrays, shared memory, weak references | Value/internal-slot and GC contracts first. BigInt arithmetic/coercion, buffer detach/resize, views, agent memory model and weak reachability each get a separate design and acceptance suite. |
| P1.6 | `$262` realm/agent/GC/buffer hooks, `$DONE`, async completion | Implement each hook alongside its owning architecture (not only at the end). Validate harness correctness before counting newly enabled tests. Never replace a missing operation with a dummy success. |
| P2.1 | Array/collections/Date/RegExp/JSON/numeric and remaining builtin methods | Consume the P0/P1 contracts. Work one family at a time, ordered by remaining shared dependencies, then failing-mode count. Cover coercion/descriptor/exception edges before declaring a family complete. |
| P2.2 | Remaining ECMA-402 constructors, algorithms and locale data | Reuse the same object/conversion/exception model; track edition 13 and locale-data requirements separately. |
| P3.1 | Unmapped/staging and edition/host applicability audit | Audit alongside implementation; mandatory edition-17 cases move into the appropriate earlier workstream. No exclusion or passing claim follows from a P3 label. |

An individual API may be needed earlier as a prerequisite; record that dependency
instead of advancing an entire library ahead of its execution model. Full
Test262 support requires all applicable rows, not merely P0 or the largest group.

## First P0.1 slice: iterator completions and rooting

Design before implementation, 2026-09-10. Official published edition-17 clauses
retrieved and read: [IteratorNext](https://262.ecma-international.org/17.0/#sec-iteratornext),
[IteratorStep](https://262.ecma-international.org/17.0/#sec-iteratorstep),
[IteratorStepValue](https://262.ecma-international.org/17.0/#sec-iteratorstepvalue),
[IteratorClose](https://262.ecma-international.org/17.0/#sec-iteratorclose), and
[IteratorDestructuringAssignmentEvaluation](https://262.ecma-international.org/17.0/#sec-runtime-semantics-iteratordestructuringassignmentevaluation).

Keep compiler-owned fixed-width bytecode and the existing managed heap. Add an
elision instruction that advances an iterator without obtaining its value.
Share next/result/done validation with value-consuming iteration. Iterator
records retain exhaustion/abrupt state, and cleanup only closes still-active
records. A rest array must remain on the VM root stack while subsequent iterator
callbacks allocate; an unrooted Rust `Vec<Value>` cannot own live JS objects.
Preserve the original throw over errors from `return()`, and close nested active
iterators inside out. This is the first part of P0.1, not complete catch/finally
or generalized completion-record support.

Public regressions were run before implementation: elisions invoked a throwing
value getter; rest errors invoked `return()`; an inner iterator error closed
both inner and outer instead of only outer. An allocation-pressure regression
also reproduced a stale managed object in rest collection. All four regressions
failed before the corresponding fixes.

### Results of this slice

The full after-run reconciles all 102,578 modes: **16,226 pass, 62,697 fail,
22,736 unsupported, 919 timeout, zero harness errors**. Comparing every path/mode
against the baseline found **zero pass-to-nonpass regressions**. Four modes
changed from assertion failure to pass:

| Test path under `test/` | Modes | Change |
| --- | --- | --- |
| `language/expressions/assignment/dstr/array-rest-iter-thrw-close-skip.js` | sloppy, strict | Iterator-origin throw no longer calls `return()` |
| `language/expressions/assignment/dstr/array-elem-trlg-iter-rest-thrw-close-skip.js` | sloppy, strict | Same guarantee after an earlier yielded element |

Six other modes changed from timeout to pass (RegExp emoji sequences and Unicode
identifier sweeps); these are timing variation, **not attributed semantic gains**.
The four public iterator regressions additionally cover elision value-getter
suppression, exhaustion, next/done/value/non-object errors, parameter and for-of
consumers, nested close order, original thrown-object identity and rest GC
reachability. The independent Node oracle passes 22,271 isolated scripts.

Validation: 249 default BlueJS tests plus two doc examples, nine Python
runner/analyzer tests, workspace build/all-target Clippy (`-D warnings`) and
workspace tests pass. Workspace socket tests required execution outside the
socket-restricted sandbox. The no-exclusion BlueJS line gate reports
**8,488/8,488 lines (100%)**, 881/881 functions and 94.18% regions. The existing
unary AST test was extended for `void` alongside public malformed-expression
and inclusive bytecode-budget cases. `cargo fmt --all -- --check` still reports
pre-existing formatting differences across the workspace; the new Rust test
file is rustfmt-clean. No workspace coverage claim is made by the BlueJS gate.

The completed P0.1 subtask is iterator completion state and rest rooting. The
next P0.1 subtask remains **general Completion records plus catch/finally**:
design handler metadata, pending completion roots, lexical/operand-stack unwind,
return/break/continue interception and distinct uncatchable host resource aborts.
Then execute P0.2 and P0.3 in the order above. No P0 workstream or builtin family
is marked fully complete by these four Test262 gains.

## Reproduction and continuation

Use a unique output directory for each run; do not run two writers against the
same results file or rebuild its adapter during measurement.

```sh
cargo build -p blueice-bluejs --bins --offline
python3 backend/bluejs/test262/run.py --jobs 8 --output target/test262-architecture-baseline
python3 backend/bluejs/test262/analyze.py --run target/test262-architecture-baseline --output target/test262-architecture-baseline/analysis
python3 -m unittest discover -s backend/bluejs/test262 -v
```

For each slice: select representative failing modes and their prerequisites,
write public regressions, record red results, implement, run focused Test262 and
regression/coverage gates, then reconcile a full before/after inventory. Compare
unique path/mode pairs, including pass-to-nonpass changes, rather than totals
alone. Preserve raw outcomes so improved parsing that exposes a later missing
dependency is not confused with a newly passing test. Never mark this backlog
complete until the full applicable inventory passes with audited host behavior.
