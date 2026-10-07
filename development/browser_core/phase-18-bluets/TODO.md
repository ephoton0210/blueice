# Phase 18 — BlueTS / BlueTSC completion worklist

[← Plan](PLAN.md) · [integration contract](INTEGRATION_CONTRACT.md) · [test interface](TEST_INTERFACE.md) · [DOM/event decision](../phase-13-bluejs-engine/DOM_EVENT_BINDINGS.md)

The goal is a supported BlueTS page that runs, interacts, debugs, and enforces
its declared boundaries through the real BlueIce processes. [PLAN.md](PLAN.md)
holds design history and delivered evidence. This file holds the ordered work
and its acceptance checks; checked leaves show prerequisites for the next step.

Work one leaf at a time, in the order below. Finish its implementation and
public-boundary test before checking it off. For the BlueTS modularity audit
C3.1.3.4.7.5, finish the entire audit and all required splits, run the full
workspace gate, then make one English-message commit; do not commit interim
leaves. Any over-1,300-line file proposed as unsuitable for splitting must be
reported together and discussed with the user before it can be an exception.
A design-only leaf needs a reviewable decision instead of a runtime test.
Headings show dependencies; they are not tasks to finish in one commit.
Keep design history in PLAN.md and defer capabilities or syntax that do not
close the current leaf.

**Current leaf: K.4.1 (narrowing design first).** Sections A to J are complete (summarized below); J.6 closed in its
measured form (`COMPATIBILITY_INVENTORY.md`). Section K closes the gaps that inventory lists.
Every item has an ID (`<section>.<item>[.<step>]`, e.g. `B4.2.2`); commit
messages and PLAN.md cite
these IDs, and a parent is checked only when all its steps are.


## Completed work (summary)

The leaf-by-leaf acceptance records for A to J (about 650 checked items) were condensed here on
2026-10-04. The full text is in git history (`git log -p -- development/browser_core/phase-18-bluets/TODO.md`,
last full version at commit `1f3dbc935`) and the delivered evidence, designs and measurements are in
[PLAN.md](PLAN.md), [PLAN_HISTORY.md](PLAN_HISTORY.md) and [COMPATIBILITY_INVENTORY.md](COMPATIBILITY_INVENTORY.md).
Every item below is checked; nothing in A to J is open.

### P0 — interactive and debuggable pages

- **A. Child connected to the live core DOM.** The script socket (IPC v3) requires the exact
  document generation on every request and a launcher-issued, per-core child capability; a stale
  generation, wrong tab or predecessor core is denied before any mutation. The core serves bounded
  DOM calls while the child executes (capped calls and wait time, unrelated traffic untouched), and
  each request/reply is bound to the live tab and document and revoked on reload, tab close and core
  cutover.
- **B. First DOM and event profile.** Node wrappers and their roots live in the child and are
  rejected after removal, reload or realm close. Implemented: `document.getElementById`,
  `textContent` get/set, `createElement`, `createTextNode`, `appendChild`, click listeners rooted in
  the child VM with a core click dispatched before default navigation, and the first event-loop
  semantics. Host typings are generated only for implemented members; the event object's `readonly`
  shape is enforced; a supported BlueTS page runs through it.
- **C. Source debugging and metadata lifetime.** Pause/resume/step for module roots and nested frames
  with distinct identities, bounded source-free Stack and Scopes, authorized values within fixed
  budgets, exceptions reported at the original BlueTS position (debugger v39), nested/module
  locations, breakpoints, symbols and types mapped to the original source, metadata invalidated on
  reload, close, cache eviction and hibernation, and the whole route tested through launcher, core
  and child.
- **D. Direct-page acceptance.** Real supervised-child fixtures for classic and ESM graphs (type-only
  import elision, authorized resolver identity, original coordinates); a contract failure on a real
  page produces a bounded report that leaks no page body or script source; multiple tabs, reload,
  policy isolation and per-tab/child resource attribution are proven through real sockets.

### P1 — strict contracts and compiler service

- **E. Runtime contracts.** Copied document-text and origin results have named contracts; every
  ingress/egress records owner and position; strict-runtime rejects a missing or unchecked
  boundary. A pure bounded validator runs before data enters the VM, cost is attributed to the
  initiating tab, valid/malformed/cyclic/deep/oversized inputs are tested, and the emitted
  strict helper is versioned and bound to the artifact so direct and emitted ESM reject the same
  value.
- **F. Registered projects and MCP.** Canonical input/config/output roots and a closed source graph
  are pinned per project; only owner-exposed projects reach a client; build results are bound to
  generation and fingerprint, bounded, staged atomically and absent on compiler error. The MCP
  adapter negotiates exact session/project/generation capabilities, exposes build only with explicit
  output-write authority, and was exercised through a real MCP client.

### Release gate

- **G. Closure without exclusions.** `cargo test --workspace`, real-process suites, `cargo fmt`,
  all-target Clippy, the pinned TypeScript oracle and the workspace coverage gate (90.22% lines in
  CI run 36341770960) passed. The run exposed and fixed cross-platform defects: Windows verbatim
  path aliases, macOS non-blocking fixture streams, the debugger pending-admission race (public
  debugger v43 admission hold), a private child socket start-up race, and Windows Clippy/size
  findings. Only the oracle suites were `#[ignore]`d, and CI ran them explicitly.

### P2 — compatibility after the page gate

- **H. Control flow and functions.** One form at a time, each through checker, emitter, direct
  execution, safe points and the pinned-`tsc` oracle: braced `while`; `try`/`catch`/`finally` with one
  identifier catch binding; immutable-local `typeof` narrowing with residual-flow return paths; and
  a two-tag callback method overload with ambiguity diagnostics.
- **I. Expressions.** First form: `local?.field` reads from an annotated record-or-nullish local,
  lowered to BlueJS `OptionalMember`, with provenance, debugger and contract evidence.
- **J. Toward pinned `tsc` 5.9.3.**
  - *J.1/J.2*: lowering, authority, debugger, contract and conformance decisions recorded per feature.
  - *J.3*: classes (constructors, inheritance, fields, visibility, parameter properties, accessors,
    `#private` names, static blocks, `useDefineForClassFields`, downlevel emit below ES2022), enums and
    `const enum` with the option set, runtime namespaces and declaration merging, tuple-literal context
    in every position, and nested function forms (arrows, function expressions, object methods,
    local functions, `catch` annotations, async/generators/yield).
  - *J.4*: module-kind selection and CommonJS (`import = require`, `export =`, interop, cycles);
    `node10`/`node16`/`bundler` package resolution with `exports`/`imports` conditions and `@types`,
    bound to canonical roots with symlink/escape refusal and exact fingerprints; pinned remote
    declarations fetched only by an explicit command.
  - *J.5*: `.tsx` in every `jsx` mode with factory/import-source options and pragmas; standard
    decorators (ES2022 shape, versioned helpers, metadata); legacy decorators with parameter
    decorators and `emitDecoratorMetadata`. Classic JSX and standard decorators run in the direct
    runtime.
  - *J.6*: inventory of every open gap (`COMPATIBILITY_INVENTORY.md`), 19 differential suites all
    passing, workspace gates green, 92.23% line coverage for the BlueTS crates. "Full parity" stays off.

### Milestones M1 to M5 (all reached)

M1 classes in a real page; M2 everyday class/enum/namespace features; M3 modules and packages;
M4 JSX and decorators; M5 a measured compatibility claim. Each closed on its observable exit test.

**Stop-loss rule kept from J.3 (2026-09-29):** a leaf that reaches deep nesting while chasing `tsc`
edge cases instead of page-facing behavior is closed at its last useful step and its siblings are
completed or recorded as inventory rows; do not open a new leaf for a corner nobody can observe.

## Current boundary

The supervised child runs authorized JavaScript and supported BlueTS classic
and module graphs. Ordinary pages expose copied document text and origin,
with no general live DOM or event API. Separate owner-only HTTP profiles
expose boolean/opaque lookup probes or the exact typed `document` lookup and
`textContent` slice; neither grants creation or events. Classic-root
pause/resume and bounded BlueTS source-span stepping work; nested-frame
pause/step/resume now have distinct public identities and capabilities.
Bounded, source-free Stack and Scopes inspection is available on the opt-in
debugger socket; original BlueTS frame coordinates and bounded runtime values
require separate owner/client grants and same-stream receipts. The
compiler/MCP route supports sealed projects and read-only queries, with no
client registration, build output, or write authority.


## Phase K — close the gaps in the J.6 inventory

**Goal.** Turn the rows of `COMPATIBILITY_INVENTORY.md` section 3 into tested behavior, in an order that
makes every later measurement trustworthy. Phase K does not claim "full parity"; K.14 decides that from
fresh evidence.

**State at the start of K (2026-10-04).** BlueTSC checks and emits a defined subset. Inside it every form
agrees with the pinned `typescript@5.9.3` (19 differential suites, 760 recorded verdicts, about 120 emit
programs). Outside it a form is refused with a precise message, with three kinds of exception that are
*not* refusals and are the most dangerous gaps: an undeclared identifier is not diagnosed, a value imported
from another module is typed `unknown`, and a `const` is not protected from reassignment everywhere.

### K.0 Rules that apply to every leaf

1. **Test first, from `tsc`.** Record the form's accepted/rejected verdicts from the pinned compiler into
   a `*-checker-matrix.tsv` (use `BLUEICE_WRITE_<NAME>_MATRIX=1`), commit the failing replay, then
   implement. A form that emits code also gets an emit-and-run oracle (build with `bluetsc` and `tsc`,
   run both under Node, compare) and, where it has a `.d.ts` form, a declaration comparison.
2. **Fixtures are named so no other matrix claims them.** Matrices discover directories by name; a name
   containing `class` is swept into the class matrix. Use a unique prefix per leaf (`unknown-name-*`,
   `narrow-typeof-*`, …) and avoid the words `class`, `enum`, `namespace`, `jsx`, `decorators`, `legacy`
   in a prefix that does not belong to that matrix.
3. **Refusals stay precise.** Never weaken an existing refusal to make a test pass. When a leaf makes a
   refusal obsolete, delete the refusal and move its fixtures to the matrix in the same commit.
4. **Inventory is regenerated, not edited by hand.** Each leaf deletes or narrows its `G-*` row and
   re-runs `python3 tools/inventory_refusals.py`; `COMPATIBILITY_INVENTORY.md` section 4 is replaced by
   its output.
5. **Gates per leaf.** `cargo fmt`, `cargo clippy -p blueice-bluets -p blueice-bluets-bluejs --all-targets
   -- -D warnings`, the two crates' tests, and every ignored oracle with
   `BLUEICE_BLUETSC_ORACLE=<tsc 5.9.3>` and `FORCE_COLOR=0`. Run cargo commands one at a time on the
   shared target (concurrent runs rebuild `bluetsc` under running tests). The full workspace gate and
   coverage run once per milestone, not per leaf.
6. **Authority does not widen.** Nothing in K adds a filesystem, network or runtime API grant. Any leaf
   that reads files (K.2, K.9, K.12) goes through the canonical-root and fingerprint rules of J.4.3.
7. **One commit per leaf**, English message citing the ID, then a short PLAN.md section (`### K.n …`)
   recording design, evidence and recorded gaps, as J.3 to J.6 did.
8. **Track source size.** Review a production source at 1,200 lines. When it approaches or exceeds
   1,300 lines, finish the current change's tests, commit and push the verified change, then record
   and execute a focused split before adding more responsibilities to that file. Refactoring gets
   its own tested commit and push; preserve behavior and source provenance.

### Execution order and dependencies

```
M6  K.1.1 → K.1.2 → K.1.3 → K.1.4 → K.1.5          (no silent wrong answers)
M7  K.2.1 → K.2.2 → K.2.3 → K.2.4;  K.3.1 → K.3.2 → K.3.3   (real projects, TS codes)
M8  K.4.1 → K.4.2 → K.4.3 → K.4.4 → K.4.5 → K.4.6 → K.4.7
    K.5.1 → K.5.2 → K.5.3          (needs K.4.3 for generic classes)
    K.6.1 → K.6.2 → K.6.3
M9  K.7.1 → K.7.2 → K.7.3 → K.7.4;  K.8.1 (needs K.1.5) → K.8.2
    K.9.1 → K.9.2 → K.9.3;  K.10.1 → K.10.2 → K.10.3;  K.11
M10 K.12.1 → K.12.2 (needs K.2 and K.9);  K.13.1 → K.13.2 → K.13.3 → K.13.4
M11 K.14
```

*Why this order.* K.1 first because each of its rows makes the checker accept or mistype a program `tsc`
rejects, which would silently inflate every pass rate measured after it. K.2 and K.3 next because a
real-project corpus and code-level comparison are the only way to measure K.4 to K.9 against `tsc`
instead of against hand-written fixtures. K.4 before K.5 and K.8 because generic classes, inferred
declarations and most real code depend on inference and narrowing. Emit breadth (K.7) comes after the
type system because a new target should be validated on programs that now type-check as `tsc` would.
K.13 is placed late but its items are independent and may be taken at any time a CI run is waiting.
Sizes: S about a day, M a few days, L about a week or more, XL a multi-week piece of `tsc` itself.

---

### K.1 Remove silent divergences — M6 — gaps G-T7, G-T8 and the recorded unknown-name/`const`/imported-type gaps

*Why it is first:* after K.1 a program the checker accepts is one `tsc` accepts for the name, mutation
and import rules.

- [x] **K.1.1 Undeclared identifiers (L).**
  - *Initial gap:* an unknown name in a value or type position was accepted unless a page profile enabled
    `require_declared_global_calls` (calls only). `PLAN.md` records it as a known gap.
  - *Work:* build a scope model shared by the checker passes (module scope, function/block scope,
    parameters, catch bindings, class members via `this`, namespace bodies, imports, ambient host
    declarations, enum members); resolve every identifier token in expressions and every named type
    in annotations; hoisting (`var`, function declarations), the temporal dead zone for `let`/`const`/
    class (TS2448, TS2449), `typeof x` in types, shadowing; globals come from K.1.4's library set and
    owner declarations.
  - *Risk:* false positives on valid programs. Mitigate by running the whole corpus (1090
    fixture directories, 20 suites) and requiring zero new rejections of an accepted entry.
  - *Done when:* a 40+ form accepted/rejected matrix (`unknown-name-*`) agrees with `tsc` and the
    existing suites are unchanged.
- [x] **K.1.2 Mutation of immutables (M).**
  - *Today:* `readonly` properties and some enum members are checked; assignment to a `const`, an
    imported binding, or a `for (const …)` variable is not diagnosed everywhere.
  - *Work:* TS2588 (const), TS2632 (import), TS2540 (readonly), compound assignment, `++`/`--`,
    destructuring targets, `for…of`/`for…in` heads, and the same inside nested functions and class
    members.
  - *Done when:* `immutable-*` matrix agrees with `tsc`.
- [x] **K.1.3 Types of imported values (M).**
  - *Today:* `import { x } from "./m"` types a value `x` as `unknown`; `.d.ts` output prints `unknown`.
  - *Work:* export the checked type of every exported variable, function, class and enum from the
    module checker (the project is already checked in dependency order), bind it at the import site for
    named, default, namespace and `import = require`/`export =` forms in ESM and CommonJS; handle cycles
    (a binding used before its module finishes gets its declared type or the annotation requirement
    `tsc` reports as TS7022-style circularity).
  - *Done when:* `imported-type-*` matrix and a declaration comparison (`import-decl-*`) agree with `tsc`.
- [x] **K.1.4 Minimal standard library (L).**
  - *Today:* no `lib.d.ts`; host types come only from owner declarations.
  - *Work:* write, from the specified ECMAScript surface and not by copying `lib.*.d.ts`, a versioned
    declaration set (`Array`, `ReadonlyArray`, `String`, `Number`, `Boolean`, `Object`, `Function`,
    `Promise`, `Map`, `Set`, `WeakMap`, `Math`, `JSON`, `Symbol`, `Error` family, `Iterable`/`Iterator`/
    `Generator`, `Date`, `RegExp`) selected by `target`, never emitted, never a runtime grant, and
    replaceable by the owner. It must parse under BlueTS's own declaration subset (which forces
    K.4 items such as overloads and index signatures to be introduced first or the set to stay small;
    record each omission).
  - *Done when:* programs using only these types have verdicts equal to `tsc` with `--lib` set to the
    same subset (`lib-*` matrix); the set's version is in the fingerprint and manifest.
- [x] **K.1.5 Inferred return types (M).**
  - *Today:* getters, methods and exported functions without a return annotation are refused where the
    type is needed (declaration output, accessor pairs); inference exists only for some expression forms.
  - *Work:* infer from `return` expressions (union of returns, `void`, `never`, async wrapping in
    `Promise`, generator `Generator<Y,R,N>`), literal freshness and widening rules, recursion guard.
  - *Done when:* `infer-return-*` matrix, plus declaration output for functions without annotations
    equal to `tsc`'s.

#### K.1 source refactoring queue

- [x] **K.1.R.1 Split existing near-limit compiler/runtime sources after the K.1.2 gate.**
  The 2026-10-05 baseline audit found the production sources below. Split them by the listed
  responsibilities, preserve behavior, run K.0's two-crate gate, then commit and push before K.1.3.

  | Source | Lines before K.1.2 | Planned split |
  | --- | ---: | --- |
  | `backend/bluets/src/checker/module/binding.rs` | 1205 | Import and export binding |
  | `backend/bluets/src/checker/module/binding/classes.rs` | 1520 | Constructor/body checks and class type helpers |
  | `backend/bluets/src/checker/module/binding/classes/overrides.rs` | 1487 | Inheritance and signature compatibility helpers |
  | `backend/bluets/src/bin/bluetsc.rs` | 1693 | Command/options/config parsing |
  | `backend/bluets-bluejs/src/expression.rs` | 1234 | Call/construction and assignment lowering |
  | `backend/bluets-bluejs/src/lowering.rs` | 1340 | Class and module lowering helpers |

#### M6 milestone gate

- [x] **M6 Close K.1 with the complete workspace and coverage gates.**
  K.1.1 to K.1.5 and K.1.R.1 are implemented and passed their two-crate gates.
  Run the existing CI workflow on the pushed K.1.5 commit, including full workspace
  build/test/lint, every pinned oracle, workspace line coverage at least 90%, and
  the independent BlueJS 88% line floor. Record the exact revision and run in PLAN.md.
  Completed on `8343929b7`; CI run [37387952696](https://github.com/ephoton0210/blueice/actions/runs/37387952696)
  passed all 29 jobs. Workspace lines: 90.48%; independent BlueJS lines: 93.00%.

### K.2 Read real project configuration — M7 — gaps G-M2, G-C2

- [x] **K.2.1 `tsconfig.json` (L).**
  - *Verified (2026-10-06):* 69 pinned configs (59 accept, 10 reject), all normalized options and file sets agree; canonical confinement, owner overlays, declaration inputs, graph output layout and strict-helper relocation covered. K.0: format and Clippy pass; 1,047 tests across 58 target/doctest groups pass, including all 117 ignored oracle tests.
  - *Portability correction (2026-10-06):* sibling precedence retains native Windows parent/prefix paths. The complete corrected K.0 gate passes 1,070 tests in 61 groups, including all 122 ignored oracles; largest production source 1,172 lines.
  - *Work:* a JSONC reader (comments, trailing commas); `compilerOptions` that map to existing options
    (`target`, `module`, `moduleResolution`, `jsx*`, `experimentalDecorators`, `emitDecoratorMetadata`,
    `esModuleInterop`, `useDefineForClassFields`, `preserveConstEnums`, `isolatedModules`, `outDir`,
    `declaration`, `sourceMap`, `strict`…); `files`/`include`/`exclude` globs; `extends` (relative and
    package, merged as `tsc` merges); unknown options are an error naming the option. Every path goes
    through the canonical-root checks; `bluetsc.json` remains for owner-only settings (import maps,
    package roots, remote declarations, strict boundaries) and is merged on top.
  - *Done when:* a corpus of 25+ real-world-shaped configs (`config-*` directories) produces the same file
    set and option interpretation as `tsc --showConfig`, compared as normalized JSON.
- [x] **K.2.2 Independent strictness flags (L).**
  - *Verified (2026-10-06):* 32 pinned on/off configurations (16 accept, 16 reject), plus 39 boundary controls and Node/declaration parity. Diagnostic settings change cache identity without changing valid JavaScript. Legacy suites remain unchanged. K.0: format and Clippy pass; 1,060 tests across 60 target/doctest groups pass, including all 120 ignored oracles. Largest production source: 1,172 lines.
  - *Work:* `strict` family flags individually selectable; each flag changes diagnostics only (never
    emit); `noImplicitAny` and `strictNullChecks` are the large ones because the checker currently
    assumes both. Add `noUnusedLocals`/`noUnusedParameters`/`noImplicitReturns`/
    `noFallthroughCasesInSwitch`/`exactOptionalPropertyTypes`/`noUncheckedIndexedAccess`/
    `useUnknownInCatchVariables`.
  - *Done when:* each flag has an on/off matrix pair agreeing with `tsc`, and the default (all strict)
    behavior of every existing suite is unchanged.
- [x] **K.2.3 CLI surface (S).**
  - *Verified (2026-10-06):* 30 pinned native project observations (24 accept, 6 reject), including command/config overrides, default project search, source/emitted lists, pretty selection and exit codes. Node output and exact declarations agree; input/config collisions are refused before publication. K.0: format and Clippy pass; 1,067 tests in 61 target/doctest groups pass, including all 122 ignored oracles. Largest production source: 1,172 lines.
  - *Work:* `--noEmit`, `--showConfig`, `--listFiles`, `--listEmittedFiles`,
  `--pretty`, `--project`, exit codes; compared with `tsc` on the same project directory.
- [x] **K.2.4 Option-combination oracle (S).**
  - *Verified (2026-10-06):* 768 pinned project combinations; all verdicts agree, 384 emitted programs match Node output and 384 noEmit configurations publish nothing. Declaration-enabled cases match exact `.d.ts` output and source maps retain their provenance structure. K.0: format and Clippy pass; 1,069 tests across 61 target/doctest groups pass, including all 122 ignored oracles. Final K.2 CI [37428756632](https://github.com/ephoton0210/blueice/actions/runs/37428756632) passes all 29 jobs on `62f6a440d`; workspace line coverage 90.43%, independent BlueJS 93.00%.
  - *Work:* Extend `option_combinations_oracle.rs` with the new options;
  keep it the template: one multi-feature program × the cartesian product of emit-affecting options.

### K.3 TypeScript diagnostic codes — M7 — gap G-C1

*Before K.4 so that every type-system leaf can compare codes and spans, not only a verdict.*

- *Baseline (2026-10-07):* pinned codes, message templates, related information and UTF-16 spans recorded for all 1,398 entries of the eleven language checker matrices and strictness matrix; 135 observed templates. The first failing structured-diagnostic replay and explicit subset-refusal list are committed before implementation.

- *Source audit baseline:* 72 further pinned diagnostic witnesses cover lexical errors and semantic families outside the language matrix; their failing replay is recorded before the remaining mappings. The language corpus alone does not close K.3.1.

- [x] **K.3.1 Code and message mapping (M).** A table from every BlueTSC diagnostic to the `TSnnnn` code
  and message template recorded from the pinned compiler (`tools/` script that runs `tsc` over the
  fixture corpus and extracts the first code per entry); `BTSnnnn` stays as an alias in machine-readable
  output. Diagnostics with no `tsc` counterpart are listed explicitly.
  - *Verified (2026-10-07):* 263 base message families plus semantic context refinements, a reproducible 2,121-template TypeScript 5.9.3 catalog, 1,398 primary code/template replays and 97 additional diagnostic source witnesses. Machine JSON preserves BTS aliases and explicit owner/subset reasons. K.0: format and Clippy pass; 1,083 tests in 64 groups pass, including all 124 ignored oracles. Largest production source: 1,185 lines. Position and presentation parity remain in K.3.2/K.3.3.
- [x] **K.3.2 Position parity (M).** Compare line, column and length of the primary span with `tsc` in
  every accepted/rejected matrix; each mismatch is a tracked row until zero (expect many: BlueTS spans
  are statement-granular in places).
  - *Test-first baseline (2026-10-07):* 800 primary coordinates are absent from machine JSON; derived byte spans differ in 543 entries of the 1,398 language/strictness cases. Another 867 configuration/CLI/option-combination cases add 16 tracked primary failures (14 unstructured configuration/argument failures and two missing source positions). Four pinned controls cover UTF-16 columns and LF/CRLF/CR. Every mismatch remains tracked until zero.
  - *Verified (2026-10-07):* zero primary position differences across 1,398 language/strictness and 867 configuration/CLI/option-combination cases; UTF-16 and original source module identity are enforced. Failed parser sources remain authorized and fingerprinted without reloading. K.0: format and Clippy pass; 1,091 tests in 65 groups pass, including all 125 ignored oracles. Largest production source: 1,185 lines.
- [x] **K.3.3 Presentation (S).** Related information, `--pretty` output, `--diagnostics`-style summary,
  and exit codes as pinned `tsc`: 0 on success, 1 when diagnostics block outputs through
  `noEmitOnError`, and 2 for ordinary diagnostic completion (including `noEmit`).
  - *Test-first baseline (2026-10-07):* 336 primary rendered-message differences and 57 missing related-information cases are tracked across the 800 rejected corpus entries. Twenty-two actual CLI observations record plain/pretty source context, related context, multiple diagnostics, CRLF, summary fields, exact exit status, emitted artifacts and Node execution. Failing replay is committed before implementation. Resource measurements are checked by schema, not compared to TypeScript's different implementation costs.

  - *Measured type-fact gaps:* `presentation-type-fact-gaps.json` retains exactly three source-hashed G-T1/K.4.1 iterator-flow witnesses. Codes, positions and related metadata agree; BlueTSC's checked `string | number` and TypeScript's narrowed `string` are both asserted exactly. All other rendered messages require equality. K.0 passes after the affected recorder replay; final M7 closure is recorded below.
  - *Verified (2026-10-07):* 22 actual CLI observations pass, 800 primary related-information records agree, and all messages outside the three measured G-T1 witnesses match. Format and both-crate all-target Clippy pass; 1,097 unique tests in 67 groups pass, including all 126 ignored oracles. The initial Linux recorder path-normalization failure is preserved in the full gate log and corrected by a passing two-test replay on unchanged backend inputs. Largest production source: 1,185 lines.
- [x] **M7 Close K.2/K.3 with fresh final-source workspace/platform/coverage evidence.** Run the existing CI workflow on the pushed implementation SHA: complete workspace build/test/fmt/all-target Clippy, every pinned oracle, all platform jobs, workspace line coverage at least 90% and independent BlueJS line coverage at least 88%, with no new exclusions. Earlier K.2 CI evidence does not close this final K.3 gate.

  - *Verified (2026-10-07):* final-source CI [37590055063](https://github.com/ephoton0210/blueice/actions/runs/37590055063) on `ed8b6d088` passes all 29 jobs, including complete workspace build/test/fmt/all-target Clippy across 25 platform configurations and both pinned TypeScript oracle jobs. Workspace line coverage is 90.60% (181,085 lines, 17,026 missed); independent BlueJS line coverage is 93.00% (77,553 lines, 5,427 missed). No coverage exclusion is added. K.2/K.3 and M7 are complete; the three measured G-T1 iterator-flow wording gaps remain for K.4.1.

### K.4 Type system — M8 — gaps G-T1 to G-T6

*Order matters: each leaf builds on the previous. The checker's flat string-keyed maps (`types`, `values`,
`functions`) were built for a subset; K.4.1 should start with a short design leaf that decides whether
narrowing needs a flow graph or can stay structural, recorded in PLAN.md before code.*

- [ ] **K.4.1 Narrowing (XL).** `typeof`, `instanceof`, `in`, `===`/`!==` against literals and `null`/
  `undefined`, discriminant property checks, truthiness, optional chaining, `switch`, early return and
  `throw`, loops and assignments that reset narrowing, closures that capture narrowed `const` vs `let`.
  *Today:* one immutable-local `typeof` form (H.3).
- [ ] **K.4.2 Guards and assertions (M).** `x is T`, `this is T`, `asserts x`, `asserts x is T`, `never`
  exhaustiveness (TS2366, TS2678).
- [ ] **K.4.3 Generic inference (XL).** Inference from arguments (candidates, unification, widening),
  from return position and contextual types, constraints (`extends`, `keyof`), defaults, explicit
  arguments, generic function types, generic classes and interfaces; the `instantiate_named` budget
  (`max_type_expansions`) stays and its exhaustion remains a precise diagnostic.
- [ ] **K.4.4 Overloads (L).** Call/construct/method overload selection in `tsc` order, contextual
  signature selection for callbacks, ambiguity and no-match diagnostics with the right code, implementation
  signature compatibility (TS2394).
- [ ] **K.4.5 Type operators (XL).** `keyof`, `typeof`, indexed access, conditional types (distributive),
  `infer`, mapped types with `readonly`/`?` modifiers and `as` clauses, template-literal types, recursive
  aliases within a depth budget; each operator needs the assignability relation extended.
- [ ] **K.4.6 More types (L).** Index signatures, readonly arrays/tuples, `bigint`, `symbol`/`unique
  symbol`, `satisfies`, `as const`, optional/rest/named tuple elements, `unknown`/`never` flow rules,
  `enum`-literal inference, `void` vs `undefined` rules.
- [ ] **K.4.7 Compatibility details (L).** Excess-property checks in every position, weak types,
  `strictFunctionTypes` vs method bivariance, optional vs `undefined`, variance annotations (`in`/`out`),
  `exactOptionalPropertyTypes`.

### K.5 Remaining class forms — M8 — gap G-T9

- [ ] **K.5.1 Abstract and implements (M).** `abstract` classes/members/constructors, `implements`
  (checked structurally), `override` and `noImplicitOverride`; emit erases all of them.
- [ ] **K.5.2 Generic and dynamic classes (L; needs K.4.3).** `class C<T> extends B<T>`, generic methods
  and static members rules, class expressions (named and anonymous, `NamedEvaluation`), computed and
  string-literal member names, index-signature members, `declare` fields.
- [ ] **K.5.3 Retire class refusals (S).** Remove the refusals K.5.1/K.5.2 make obsolete (redeclaring a
  member of an imported base, accessor type mismatch, definite assignment through a branch, getter without
  annotation) and move their fixtures into the class matrix.

### K.6 Module syntax — M8 — gap G-T10

- [ ] **K.6.1 Default and re-exports (M).** `export default <expression>`, anonymous default function and
  class, `export { x } from`, `export * from`, `export * as ns from`; ESM and CommonJS emit (including
  live bindings through re-export and the `__exportStar`-style semantics written from the spec).
- [ ] **K.6.2 Type-only and attributes (S).** All `import type`/`export type` forms, inline `type`
  modifiers, import attributes (`with { type: "json" }`), `export as namespace`.
- [ ] **K.6.3 Ambient and augmentation (L).** `declare module "x" { … }` (ambient and augmentation),
  `declare global`, global augmentation from a module, triple-slash `reference path/types/lib`,
  shorthand ambient modules; all under the J.4 authority rules (a declaration never creates a runtime
  binding).

### K.7 Emit breadth — M9 — gaps G-E1 to G-E4

- [ ] **K.7.1 Targets (XL).** `ES2015`–`ES2019`, `ES2021`, `ES2023`, `ESNext`, then `ES5`; helpers
  (`__awaiter`, `__generator`, `__spreadArray`, `__values`, `__read`, class/destructuring/rest lowering)
  written from the specified semantics, versioned like the class and decorator helpers, recorded in the
  manifest and fingerprint. Validate each target by running its output under a Node build or a VM
  profile that rejects newer syntax (use BlueJS's parser at the matching edition as the syntax check).
- [ ] **K.7.2 Module systems (L).** `AMD`, `UMD`, `System`, per-file module kind for `node16`/`nodenext`
  with `.mts`/`.cts`, `verbatimModuleSyntax`, `importHelpers`, `noEmitHelpers`.
- [ ] **K.7.3 Output options (M).** `removeComments`, `newLine`, `emitBOM`, `inlineSources`, `sourceRoot`,
  `mapRoot`, `downlevelIteration`, `stripInternal`, `preserveValueImports`/`importsNotUsedAsValues`.
- [ ] **K.7.4 Decorators on new targets (M).** Standard decorators, auto-accessors and legacy decorators on
  every K.7.1 target, plus the standard forms still refused (decorated private members, computed names,
  classes in namespaces, `export default @d class`, `super` in static members, decorated class
  expressions).

### K.8 Declarations — M9 — gaps G-D1, G-D2

- [ ] **K.8.1 Inferred `.d.ts` (L; needs K.1.5).** Declaration output from inferred types, generic
  declarations, overloads of any shape, merged namespaces, `export =`, `declare module`, accessor pairs.
- [ ] **K.8.2 Declaration options (S).** `declarationDir`, `emitDeclarationOnly`, `declarationMap`,
  `isolatedDeclarations` (with its own diagnostics).

### K.9 Resolution extras — M9 — gap G-M1

*All under the canonical-root, symlink-refusal and fingerprint rules of J.4.3; each new input file read
goes into the resolver's observation record.*

- [ ] **K.9.1 Package features (M).** `typesVersions`, `.d.mts`/`.d.cts`/`.mts`/`.cts` entry points,
  package self-name imports, `moduleResolution: classic`.
- [ ] **K.9.2 Path mapping (M).** `paths`, `baseUrl`, `rootDirs`, `types`/`typeRoots` and automatic `@types`
  inclusion; the owner's roots bound every mapped path.
- [ ] **K.9.3 JSON and JavaScript sources (L).** `resolveJsonModule` (typed from the JSON), `allowJs`/
  `checkJs` with `.js` files treated through the supported subset and no JSDoc types.

### K.10 JSX and decorator residuals — M9 — gap G-C3

- [ ] **K.10.1 JSX typing (L; needs K.4.3).** Type arguments on tags, generic components,
  `JSX.ElementType`, `LibraryManagedAttributes`, `IntrinsicClassAttributes`, `defaultProps`, duplicate
  attribute and `children`-specified-twice diagnostics, namespaced attribute typing.
- [ ] **K.10.2 Runtime JSX namespace (M).** Read the automatic runtime's `JSX` namespace from
  `<jsxImportSource>/jsx-runtime` through package resolution.
- [ ] **K.10.3 Decorator typing (M).** Return-type compatibility of a replacement with the decorated
  element (standard and legacy), `lib.decorators.d.ts`-equivalent context types.

### K.11 Source maps — M9 — gap G-E5 (L)

Token-level mappings with `names` and `sourcesContent`; compared with `tsc` by decoding both maps and
comparing the mapping sets for the supported constructs (not the text); debugger mappings for generated
helper frames and the relocated decorator text.

### K.12 Projects — M10 — gap G-M2

- [ ] **K.12.1 References and build (XL).** `references`, `composite`, `--build` with `.tsbuildinfo`
  (own format allowed; semantic parity: what is rebuilt and in what order is compared).
- [ ] **K.12.2 Incremental and watch (L).** `--incremental` and `--watch` with cache keys bound to the
  graph and configuration fingerprints of J.4.3; invalidation tests as in J.4.3 (changed manifest, nearer
  package, repointed symlink).

### K.13 Platforms and test health — M10 — gap G-M3 and recorded flakes

- [ ] **K.13.1 Windows oracle (M).** Launch `tsc.cmd`, handle drive/UNC/verbatim paths in the harness,
  run or skip-with-reason the symlink tests, add Windows legs (x86_64 and arm64) to the
  `typescript-oracle` CI matrix.
- [ ] **K.13.2 Hosted CI confirmation (S).** The extended `typescript-oracle` job has only run locally.
  Run it on GitHub, fix what only hosted runners show (Node/npm versions, runtime limits of the 222 s
  `typescript_oracle` suite), record the run.
- [ ] **K.13.3 Launcher start-up flake (M).** `out_of_process_debugger::scope_relations::
  launcher_relates_nested_parent_roots_but_not_child_local_slots` failed once with the launcher exiting
  status 1 before creating its rendezvous socket under the load of 33 parallel processes. Capture the
  launcher's stderr in the harness, reproduce under load, fix the cause or bound it with a reported retry;
  never silence it.
- [ ] **K.13.4 Per-file coverage (M).** Offline tests to bring each new emitter above 90% lines:
  `emitter/decorators.rs` (81.7%), `emitter/legacy_decorators.rs` (75.1%), `emitter/jsx.rs` (87.9%),
  `package_resolution.rs` (88.7%), `bluets-bluejs/src/jsx_direct.rs` (80.1%) at the 2026-10-02 measurement.
  Also give `llvm-tools` a documented setup (the pinned toolchain's `LLVM_COV`/`LLVM_PROFDATA`).

### K.14 Re-measure and decide — M11

Regenerate `COMPATIBILITY_INVENTORY.md` from the suites, recompute the pass rate over the versioned case
list (fixture directories and programs at that commit, per suite), run the three gates of J.6.3 on every
CI platform, and write the decision into PLAN.md: which inventory rows are closed, which remain, and
whether any parity wording is justified. The words "full parity" stay off while any required row is open;
a justified narrower statement (for example "the strict-mode subset listed in section 2 of the inventory
agrees with tsc 5.9.3 on N cases") is the expected outcome unless K.4, K.7 and K.12 all close.
