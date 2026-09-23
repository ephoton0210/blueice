# Test262 architecture triage

Snapshot: `72faf8ec1445c55149615e8b35187830783aba1a`.

## Session status (2026-09-23) — numbers below are STALE, not re-measured against current HEAD

This session (branch `feature/test262-remaining-failures`) landed a large batch of real fixes on top of the `d33babc` commit the tables below were measured against, but **did not re-run the full inventory afterward** — the session was explicitly wrapped up without waiting for long-running verification, per direction, rather than guessing at updated numbers. Treat every count below (the target table, the 63-mode breakdown, the diagnostics table) as describing the state **before** everything in this list, not the current one:

- **Cross-realm reverse membrane** (the 10-mode bucket below): mostly fixed. A real reverse-membrane implementation landed (`Test262ReverseValue`, `vm/test262/reverse.rs`, reusing `shadow_realm.rs`'s `ACTIVE` reentrancy mechanism via the new shared `vm/realm_reentrancy.rs`), plus a receiver-is-foreign native-dispatch extension (`IteratorWrapperNext`/`ArrayBuffer.prototype.slice`) and a post-merge `register_active` integration fix. Along the way, two genuine *forward*-direction bugs were also found and fixed (`object_get_own_property`/`has_property` never checking foreign/reverse facades; `object_set_prototype` never checking foreign at all). See commits `4abfe64`, `087e6e9`, `52708f1`.
- **String-size resource policy** (the 4-mode bucket below): fixed. `RuntimeError::StringLimit` is now a catchable `RangeError` (matching V8/SpiderMonkey/JSC), and `staging/sm/JSON/parse-mega-huge-array.js` got its own resource allowance via the existing exact-path mechanism. See commits `66bc85e`, `86388e9`.
- **Grammar gap** (the 1-mode bucket below): fixed — `parse_for_stmt` was missing the `for await` early-error check on 3 of its exit paths. See commit `853f0a7`.
- **Spec-version conflict** (the 1-mode bucket below): confirmed, not fixed, not fixable — verified directly against the live ECMA-262 draft and cross-referenced against upstream `tc39/test262#5112`/`#5113`; see that row's own entry below, already accurate.
- **New `stale_corpus` status**: added to the test262 harness (`run.py`/`analyze.py`) so a fixture like the one above — verified current-spec-correct, with an open upstream Test262 fix pending — gets its own status instead of being folded into `fail`. See commit `a9f497c`.
- **A new regression, found and partially fixed**: fixing the reverse membrane's `Symbol.species` lookup made a previously-unreachable code path reachable for the first time (`staging/sm/TypedArray/slice-bitwise-same.js`'s cross-compartment species-construction case), which panicked. The panic is fixed (commit `7dce4ee`), but a real, deeper limitation was found underneath it and is **not** fixed — see "Reverse-membrane round-trip cache discards mutations" below for the full writeup.
- **"Corpus problem" bucket (19 modes below)**: a fix exists (commit `774ad1f`, on worktree branch `worktree-agent-a8f3aa9080a3c0568`, a general adapter fix — stop strict-prefixing Test262 harness *includes*, only the test body — verified upstream via `tc39/test262#5008`, and independently verified via a full-inventory before/after diff showing exactly 17 of 19 modes fixed, 2 remaining for separate confirmed unrelated bugs) but **has not been merged into this branch yet**.

**Not done, explicitly deferred, before trusting any updated pass/fail count:**
1. Merge `774ad1f` into this branch.
2. ~~Let the full `cargo test -p blueice-bluejs` suite finish~~ — done after this note was first written: the full suite (which was still running when this session started wrapping up) finished with **exit code 0, no failures**, on top of commit `7dce4ee` (before the `774ad1f` merge below). Targeted subsets run earlier for faster feedback — `test262_reverse_membrane.rs` 17/17, `test262_host.rs` 119/119, the typed-array unit tests, clippy, fmt — also all passed and are consistent with this.
3. Run the coverage gate.
4. Rebuild both adapter binaries fresh from the merged HEAD (do not trust a stale one — this session hit that mistake once already).
5. Run a fresh, complete 102,926-mode inventory and reconcile the numbers below against it.
6. Regenerate this file's target table and breakdown via `analyze.py` from that fresh run, preserving the hand-written prose sections (this one, the spec-version-conflict row, the round-trip-cache writeup below).

## Reverse-membrane round-trip cache discards mutations (known gap, found 2026-09-23, not fixed)

**Symptom:** `staging/sm/TypedArray/slice-bitwise-same.js` (2 modes) constructs a species result through a *reverse* Test262 facade and observes the wrong element values (previously a panic, fixed in `7dce4ee`; now a clean value mismatch).

**Exact mechanism, with code locations:**

1. `arr` is a child-realm TypedArray. Setting `arr.constructor = Float32Array` (the *parent's* own constructor) crosses that constructor value into the child as a live reverse facade (`Test262ReverseValue`, `vm.rs`), created by `test262_transport_value`'s general "else" branch (`foreign.rs`).
2. `arr.slice(0)` dispatches into the child (foreign receiver → `test262_foreign_typed_array_native_call`, `foreign.rs`). `%TypedArray%.prototype.slice`'s `SpeciesConstructor` step reads `arr.constructor` (the reverse facade), then `Get(constructor, Symbol.species)` — forwarded via `test262_reverse_get` (`reverse.rs`) into the *real* parent `Float32Array[Symbol.species]` accessor, which per spec returns `this`. Non-nullish, so the reverse facade itself is used as the species constructor (`typed_array_species_constructor`, `typed_arrays.rs`).
3. `typed_array_create_foreign_target` (`typed_arrays.rs`) now recognizes this as cross-realm (fixed `7dce4ee` — previously it only checked `test262_foreign_native_function`, the *forward* direction) and calls `Construct` through it. `dispatch_call` (`vm.rs`) routes this to `test262_reverse_call` (`reverse.rs`), which resolves the live parent `Vm` via `resolve_active` and genuinely constructs a fresh, empty `Float32Array` **in the parent's own heap**.
4. `test262_reverse_call` exports that result back into the child via `test262_reverse_export_result` (`reverse.rs`) → `parent.test262_export_foreign_value(child_realm_id, &value)`. Since the value is a TypedArray, this takes `test262_transport_value`'s eager TypedArray-snapshot path (`foreign.rs`, the `typed_array_values` branch): it allocates a **brand-new, independent buffer in the child's own heap** and copies the (currently all-zero) source values into it — a genuine value snapshot, not a live view.
5. Back in `typed_array_slice` (still executing as the child), the code now correctly detects (via the new `test262_reverse_native_function`, `reverse.rs`) that this constructor was cross-realm, but that the *result* is **not** a live forward facade (`test262_foreign_typed_array_info` returns `None` — it's a real local child object). It falls back to an ordinary per-element `typed_array_read_values`/`typed_array_write_values` copy into that local snapshot. **Verified directly** (during debugging, with a temporary readback probe): reading the snapshot back immediately after this write shows the correct values.
6. `typed_array_slice` returns; `test262_foreign_typed_array_native_call` calls `test262_import_foreign_result` → `test262_import_foreign_value` (`foreign.rs`) to bring the result back out to the parent. **This is where the data is lost.** Its very first check is a round-trip identity cache: `if let Some(value) = realm.imported_values.get(&target) { return Ok(value.value.clone()); }`. Because the constructed object was registered in `imported_values` during step 4 (as a stand-in for the *original, still-empty* parent object `test262_reverse_call` constructed), this check fires and returns that **original, pristine, never-mutated parent object** directly — completely bypassing the snapshot step 5 actually populated.

**Why this is a real design gap, not a simple bug:** the round-trip cache exists so a value crossing out, back in, and out again keeps a *stable identity* rather than accumulating facade-of-facade wrappers — correct and necessary in general. It was simply never exercised by a case where the *stand-in itself* gets legitimately mutated after being cached and needs those mutations to survive the trip back out. Every existing forward-direction analog of this problem (a parent-owned buffer that a *child* TypedArray view writes through) is solved by `foreign.rs`'s buffer-mirror mechanism (`test262_foreign_buffer_mirrors`, `test262_foreign_buffer_clone`, `test262_sync_foreign_buffer_mirrors`) — but that mechanism is entirely parent-owned and forward-only: it lets a *parent* register and sync a local clone of a *child's* real buffer. There is no reverse counterpart letting a *child* register something a *parent* (or further ancestor) will later see correctly.

**What a full fix needs (either would work, neither attempted here):**
- **(a) Construct-with-data-upfront:** instead of constructing an empty result then mutating it afterward, pass the source data directly to the species constructor call (TypedArray constructors accept an array-like/iterable argument). This sidesteps the cache entirely, since there is nothing to mutate post-construction. Requires restructuring `typed_array_create_foreign_target`/`typed_array_slice` so the copy data is known *before* the `Construct` call, not after (currently `copy_count` is only finalized after construction, to handle a species constructor that resizes the source).
- **(b) Reverse buffer mirror:** build a mirror-registration mechanism symmetric to `foreign.rs`'s forward one, so a child-side mutation to a reverse-call's result is visible to whichever realm the result crosses back out to, without relying on (and without needing to bypass) the round-trip identity cache. This is architecturally the same scale of work as the reverse facade mechanism itself.

**Current state:** `typed_array_create_foreign_target` correctly classifies both directions (no more panic; `length`/`instanceof` on the result are correct). The underlying value round-trip is not fixed. `staging/sm/TypedArray/slice-bitwise-same.js` remains a failing mode, now for this understood, documented reason. `backend/bluejs/tests/test262_reverse_membrane.rs`'s `typed_array_species_slice_via_a_reverse_facade_constructor_does_not_panic` test only asserts what is actually guaranteed today (no panic, correct shape) and documents this exact gap inline.

## Platform provenance (updated 2026-09-22, stale — see "Session status" above)

This triage was regenerated by `backend/bluejs/test262/analyze.py` from a complete, unfiltered 53,582-file / 102,926-mode inventory of commit `d33babc` on branch `feature/test262-remaining-failures` (not yet merged to `main`), run on **macOS 26.6.2 on Apple silicon (Apple M4, aarch64-apple-darwin)**, 10 logical CPUs, 24 GB RAM, Python 3.12, which completed in 269.255 seconds; the counts below are that run's. Both adapter binaries (`bluejs-test262`, `bluejs-regexp-worker`) were rebuilt from this exact commit immediately before running — an earlier attempt against a stale pre-`vm.rs`-split binary produced an implausible 3,003-fail result and was discarded rather than trusted.

This run exists specifically to verify `d33babc`'s new `excluded` status (the `CanBlockIsTrue`/`CanBlockIsFalse` host-capability reclassification described below), not to supersede the per-platform breakdown tables in [the Ubuntu report](TEST262_LINUX_REPORT.md) or [the macOS report](TEST262_MACOS_REPORT.md), which still reflect the prior `9decbb3`/`eaeb5c1` measurements (group/selection/ECMA-402/coverage/oracle tables — none of that was re-measured here). The totals below reconcile exactly against that prior measurement (102,859 pass either way, same 102,926-mode denominator): this is confirmation that `d33babc` only moves 4 modes from `fail` to `excluded`, not a platform-divergent or regressed result. **Windows has not been re-run against `d33babc`.**

Reconciled 53,582 files / 102,926 modes; complete inventory: True.

Targets are inferred from paths/metadata; blockers are first observed symptoms, not proven root causes. Feature/dependency counts overlap. Target counts are exclusive and reconcile to all modes. Priorities are dependency order, not failure-count order. Passed negatives remain passes; no outcomes are excluded.

| Order | Target | Pass | Fail | Unsupported | Excluded | Timeout | Harness error | Prerequisites |
| --- | --- | ---: | ---: | ---: | ---: | ---: | ---: | --- |
| P0.1 | completion: Completion records, iterator lifetime, catch/finally, control transfer | 9659 | 0 | 0 | 0 | 0 | 0 | — |
| P0.2 | environments: Persistent realms, bindings, parameter environments, arguments, eval | 3947 | 0 | 0 | 0 | 0 | 0 | completion |
| P0.3 | references: Reference evaluation, coercion and observable evaluation order | 6083 | 0 | 0 | 0 | 0 | 0 | completion, environments |
| P0.4 | objects: Internal methods, descriptors, receiver and callable/constructor contracts | 7799 | 0 | 0 | 0 | 0 | 0 | references |
| P1.1 | grammar: Source grammar and classified strict/early errors | 4330 | 3 | 0 | 0 | 0 | 0 | environments, references |
| P1.2 | classes: Classes, private slots, super and derived construction | 17118 | 4 | 0 | 0 | 0 | 0 | objects, grammar |
| P1.3 | suspension: Resumable frames, generators, async functions and promise jobs | 5081 | 0 | 0 | 0 | 0 | 0 | completion, environments, objects, grammar |
| P1.4 | modules: Module linking, live bindings, evaluation and dynamic import | 2667 | 0 | 0 | 0 | 0 | 0 | environments, suspension, grammar |
| P1.5 | storage: BigInt, buffers, typed arrays, shared memory and GC weak slots | 7645 | 2 | 0 | 4 | 0 | 0 | objects |
| P1.6 | host: Test262 realm, agent, GC, buffer and async host hooks | 232 | 0 | 0 | 0 | 0 | 0 | environments, suspension, modules, storage |
| P2.1 | library: Remaining standard builtin algorithms and descriptors | 29545 | 0 | 0 | 0 | 0 | 0 | completion, references, objects |
| P2.2 | intl: ECMA-402 constructors, algorithms and locale data | 6714 | 0 | 0 | 0 | 0 | 0 | library |
| P3.1 | review: Unmapped/staging targets requiring specification and applicability review | 2039 | 54 | 0 | 0 | 0 | 0 | — |

`P0.1`-`P1.6` and `P2.*` are now failure-free at the target-classification granularity above: the 63 remaining failures are concentrated in `P1.1`/`P1.2`/`P1.5` (grammar, class and storage edge cases the classifier still routes there even though the root causes are cross-realm/resource-limit, not grammar/class/storage gaps — see the breakdown below) and `P3.1` (staging/proposal review). `P1.5` also carries the 4 `excluded` modes (Atomics host-capability declaration, not a failure — see below). This does not mean the corresponding features are complete, only that this inventory's remaining failures don't fall in them.

## Remaining-failure breakdown (63 modes, by root-cause class)

Classified by hand against the 63 failing (path, mode) pairs, not by the symptom classifier above. (Down from the prior 67: `d33babc` moved the 4 `CanBlockIsFalse` Atomics modes out of `fail` into their own `excluded` status — see the next section — rather than removing an engine gap.)

| Class | Modes | Representative test | Why |
| --- | ---: | --- | --- |
| Instruction-budget / wall-time | 22 | `staging/sm/Date/dst-offset-caching-1-of-8.js` | A bytecode interpreter's default fuel isn't enough for a few genuinely heavy fixtures (8 DST-cache-table files, `two-digit-years.js`, `toSpliced-dense.js`, a 100,000-live-object WeakMap chain); each was measured and would need an exact-path allowance an order of magnitude past what's reasonable to grant blindly. |
| Corpus problem (not an engine gap) | 19 | `staging/sm/strict/10.6.js` | The shared Annex B strict-mode harness (`non262-strict-shell.js`) runs its "lenient" half through a `"use strict"`-prefixed direct `eval`, so proving sloppy-only behavior (an implicit global, a non-strict `delete`, `arguments` aliasing, ...) is impossible under strict; verified independently by running the identical harness+test concatenation under Node 24, which fails the same way. 6 more (`destructuring-array-default-*.js`, strict mode only) hit the same shape: an assignment-pattern target that is never declared is an implicit global in sloppy code and a `ReferenceError` in strict, and the harness only checks the sloppy answer. |
| Cross-realm "reverse membrane" | 10 | `staging/sm/Date/defaultvalue.js` | A `$262.createRealm()` child realm can run a foreign built-in on a local receiver (built, this session), but a parent-realm object/Proxy/callback re-entering the child (the reverse direction) isn't supported yet; doing it safely needs a proper reverse-membrane exotic-object design, judged disproportionate to add opportunistically. |
| String-size resource policy | 4 | `staging/sm/String/replace-math.js` | The fixture expects a catchable out-of-memory error past the managed string's 1 MiB limit; the host's resource limit is deliberately an uncatchable abort, so it can never satisfy `assert.throws`. |
| `regress` crate RegExp limitation | 4 | `staging/sm/RegExp/unicode-back-reference.js` | The pinned `regress` engine's non-`u`-mode Canonicalize and `u`-mode backreference matching disagree with ECMA-262 on lone-surrogate/BMP-fold edge cases; not fixable without patching or replacing the dependency. |
| Call-depth limit | 2 | `staging/sm/class/superPropChains.js` | A 100-deep constructor chain needs more native stack than the 32-frame limit allows without a byte-budget guard (see CLAUDE.md's Phase 13 paragraph). |
| Spec-version conflict (confirmed stale corpus fixture, not a BlueJS gap) | 1 | `annexB/language/function-code/block-decl-func-skip-arguments.js` | Verified 2026-09-23 directly against the live [ECMA-262 draft](https://tc39.es/ecma262/multipage/ordinary-and-exotic-objects-behaviours.html#step-functiondeclarationinstantiation-web-compat-insertion-point) (`FunctionDeclarationInstantiation`'s Annex B web-compat insertion point): the `funcName is not "arguments"` guard gates only *creating a new* var binding; the sibling step that runs when the block function is evaluated (`funcEnv.SetMutableBinding(funcName, funcObj, false)`) has no such guard and still overwrites an existing `arguments` binding. This fixture (2017) was written against an older edition that appended `"arguments"` to `parameterNames` itself, a step the current algorithm no longer has; the two `staging/sm` fixtures already in this inventory require the current behavior instead, and BlueJS matches it (as do V8 and SpiderMonkey). This is a known, already-reported-and-fixed-upstream corpus bug, not an open question: [tc39/test262#5113](https://github.com/tc39/test262/issues/5113) documents the exact same contradiction, and [tc39/test262#5112](https://github.com/tc39/test262/pull/5112) (open, unmerged as of this snapshot) corrects the fixture's three assertions to expect the current behavior. No BlueJS change is possible or warranted here — flipping the engine to satisfy the stale fixture would contradict the current spec text and regress the two already-correct `staging/sm` fixtures. |
| Grammar gap | 1 | `staging/sm/AsyncGenerators/for-await-bad-syntax.js` | An early-error rule for a specific malformed `for await` loop body isn't classified yet. |

## Host-capability exclusions (4 modes, not counted as failures)

Added by `d33babc`: a fixture's `CanBlockIsTrue`/`CanBlockIsFalse` Test262 flag declares which host `[[CanBlock]]` capability it assumes applies (per `INTERPRETING.md`, "should only be run when [[CanBlock]] ... matches"), not a runtime host hook BlueJS could implement either way. The two flags are mutually exclusive per host.

| Class | Modes | Representative test | Why |
| --- | ---: | --- | --- |
| Atomics host `[[CanBlock]]` declaration | 4 | `built-ins/Atomics/wait/cannot-suspend-throws.js`, `built-ins/Atomics/wait/bigint/cannot-suspend-throws.js` (2 modes each) | BlueJS's `Atomics.wait` genuinely suspends the agent, so this host's declared `[[CanBlock]]` is `true` and every `CanBlockIsTrue` fixture already passes against that behavior. A `CanBlockIsFalse` fixture assumes the opposite; satisfying it would mean making `Atomics.wait` always throw, which would break every already-passing `CanBlockIsTrue` fixture. `canblock_exclusion()` in `run.py` decides this statically and never dispatches the fixture to the adapter, so it is `excluded` rather than `fail` or `unsupported` — a spec-legal host capability choice, not an engine gap. |

## Frequent diagnostics

Exact per-mode evidence, source hashes, esid, includes and dependency labels are in `items.jsonl`.

| Diagnostic (numeric details normalized) | Modes |
| --- | ---: |
| BlueJS instruction budget exhausted | 22 |
| Test262Error: Expected SameValue(«false», «true») to be true | 13 |
| ReferenceError: a is not defined | 6 |
| BlueJS string exceeds # bytes | 4 |
| host declares [[CanBlock]] = true, fixture requires CanBlockIsFalse | 4 |
| uncaught JavaScript value: Object(ObjectId { heap: #, serial: # }) | 4 |
| RangeError: maximum call depth exceeded | 2 |
| Test262Error: Expected SameValue(«true», «false») to be true | 2 |
| Test262Error: Expected SameValue(«undefined», «#») to be true | 2 |
| Test262Error: number Expected SameValue(«#», «#») to be true | 2 |
| TypeError: ArrayBuffer method requires an ArrayBuffer receiver | 2 |
| TypeError: Iterator wrapper next requires an iterator wrapper | 2 |
| Test262Error: AsyncGenerator:for await (;;) ; Expected a SyntaxError to be thrown but no exception was thrown at all | 1 |
| Test262Error: Expected SameValue(«"function arguments() {}"», «"[object Arguments]"») to be true | 1 |

## Measurement limits

A passing result is the runner's observation, not proof of complete feature conformance. Unclassified parser rejections cannot establish required negative early errors. Existing adapter records may not distinguish a runtime exception in an include from one in the test body. Missing APIs, assertion mismatches and timeouts require reproduction before assigning a confirmed engine root cause. Staging, Annex B, Intl and host-dependent cases remain visible pending applicability audit.

## Historical focused P0.4 audits

The following focused filters were measured against the prior `6eec1ac9…` snapshot. They remain useful regression evidence but are not substitutes for the complete `72faf8ec…` inventory above.

### Closure audit

The final targeted realm and Proxy audit records **607/607** passing modes for `built-ins/Proxy`, **60/60** for `built-ins/Proxy/construct`, and **20/20** for `built-ins/Reflect/construct`. The remaining Reflect modes required only that `Date.now` exist as a callable, non-constructor; the Date baseline supplies that contract without claiming complete Date object support. The public `conformance_edges` regression suite passes **37/37**.

### Weak collections, Date and Array

The P0.4 weak-collection implementation was checked with focused filters on the same Test262 snapshot: `built-ins/WeakMap/` is **281/281 pass**, `built-ins/WeakSet/` is **170/170 pass**, and `built-ins/WeakRef/` is now **58/58 pass** at `target/test262-p04-weak-ref-final-2`.

The two previous WeakRef failures were unblocked by a non-placeholder `FinalizationRegistry` substrate: the constructor/callback, weak target and unregister-token cells, strong holdings, `register`, `unregister`, and dead target collection behavior are represented in the VM. Cleanup-job scheduling and callback delivery remain future weak-GC/host work, so this does not claim full FinalizationRegistry conformance.

The corresponding Date audit rose from **166/1,236** to **1,220/1,236** at `target/test262-p04-date-final-4`; all 16 remaining modes require the absent Temporal bridge (`Date.prototype.toTemporalInstant`). The Array audit rose from **4,846/6,119** to **5,169/6,119** at `target/test262-p04-array-final-3`, after species-aware map/filter, `Array.of`, `at`, iterators, and the find family. The remaining Array failures are predominantly `Array.fromAsync`, concat spreadability, copy-by-value, and resizable-buffer work.
