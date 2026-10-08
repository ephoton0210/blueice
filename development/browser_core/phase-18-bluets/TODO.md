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

**Current leaf: K.5.2 (generic and dynamic classes).** Sections A to J are complete (summarized below); J.6 closed in its
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

- [x] **K.4.1 Narrowing (XL).** `typeof`, `instanceof`, `in`, `===`/`!==` against literals and `null`/
  `undefined`, discriminant property checks, truthiness, optional chaining, `switch`, early return and
  `throw`, loops and assignments that reset narrowing, closures that capture narrowed `const` vs `let`.
  *Verified scope:* bounded lexical control-flow facts, documented below.
  - *Design decision (2026-10-07):* PLAN.md records a bounded flow graph per execution scope, reusing lexical binding identities and authorized syntax. Graph construction, predicates and evaluation remain separate modules; assignments, joins, loops, abrupt completion and closure boundaries require pinned positive and negative witnesses before implementation.
  - *Failing baseline (2026-10-07):* 97 pinned cases (67 accept, 30 reject), eleven accepted runtime/declaration witnesses. Linux replay reports 82 checker mismatches and ten runtime failures; fixture coverage and the pinned recorder pass. No production implementation is included in the baseline.
  - *Supplemental baseline (2026-10-07):* twenty additional path/execution witnesses bring the corpus to 117 cases (81 accept, 36 reject) and thirteen runtime witnesses. Before the extension, Linux replay records eighteen checker mismatches and two runtime failures; recorder and completeness pass.
  - *Optional receiver boundary (2026-10-07):* six additional pinned controls bring the corpus to 123 cases (83 accept, 40 reject) and fourteen runtime witnesses. All six disagree with the pre-fix Linux CLI. Known-null receivers remain rejected with TS2339; nullable aliases and missing-member metadata require correction.
  - *Verified (2026-10-07):* all 123 pinned verdicts/primary diagnostics and fourteen runtime/declaration witnesses pass; 840 CLI primary messages/related records agree with TypeScript. Format and both-crate all-target Clippy pass. The complete 68-group run passes 1,099 tests and all 128 ignored oracles; two legacy bridge examples use TypeScript-invalid known-null sources. Correcting only those test sources preserves AST/debugger assertions and adds a direct TS2339 refusal control; the full 973-test ordinary suite then passes on identical production bytes. Combined: all 1,101 unique tests pass. Largest production source: 1,185 lines. The three historical M7 iterator wording gaps are resolved without allowances; broader exception flow and dotted capture precision stay recorded in G-T1.
- [x] **K.4.2 Guards and assertions (M).** `x is T`, `this is T`, `asserts x`, `asserts x is T`, `never`
  exhaustiveness (TS2366, TS2678).
  - *Failing baseline (2026-10-07):* 61 pinned programs (30 accept, 31 reject), five runtime/declaration witnesses, including imported signatures and callable aliases. Linux replay records 55 checker mismatches and four runtime/declaration failures; fixture completeness and pinned recorder pass. The shared diagnostic corpus is regenerated to 1,582 programs and 145 templates. Production remains unchanged in this baseline.
  - *Call-position replay (2026-10-07):* the 61-program draft passes the full frozen K.0 gate (1,105 tests, all 130 ignored oracles). Fourteen further pinned controls expand the guard corpus to 75 programs (36 accept, 39 reject); eight invalid call effects and two `undefined`/`void` argument rejections fail before their correction. All five runtime/declaration witnesses and the pinned recorder pass. The shared corpus now contains 1,596 programs and 145 templates; the leaf remains open.
  - *Verified (2026-10-07):* all 75 pinned verdicts/primary and related diagnostics, five runtime/declaration witnesses, and the 123-case narrowing regression pass. Statement/comma call effects follow the pinned parenthesis/initializer/argument controls. Format, format check and both-crate all-target Clippy pass; all 1,105 tests in 69 groups pass, including all 130 ignored oracles. Largest production source: 1,185 lines. G-T1 retains the broader unmeasured predicate/exception/capture forms.
- [x] **K.4.3 Generic inference (XL).** Inference from arguments (candidates, unification, widening),
  from return position and contextual types, constraints (`extends`, `keyof`), defaults, explicit
  arguments, generic function types, generic classes and interfaces; the `instantiate_named` budget
  (`max_type_expansions`) stays and its exhaustion remains a precise diagnostic.
  - *Failing baseline (2026-10-07):* 59 pinned programs (33 accept, 26 reject), three runtime/declaration witnesses. Linux replay records 34 checker/primary-diagnostic mismatches and two runtime/declaration mismatches; completeness and pinned recorder pass. The shared diagnostic corpus contains 1,655 programs and 147 templates. Production remains unchanged in this baseline.
  - *Generic-boundary replay (2026-10-07):* the 59-program draft passes all four focused tests. Eleven further controls expand the corpus to 70 programs (39 accept, 31 reject) and four runtime/declaration witnesses. Linux replay records six checker/diagnostic differences and a missing contextual-inference budget refusal; all four runtime/declaration witnesses, completeness and pinned recorder pass. The shared corpus contains 1,666 programs and 147 templates. The leaf remains open.
  - *Annotated-call replay (2026-10-08):* the 70-program draft passes the full frozen K.0 gate (1,110 tests in 70 groups, all 132 ignored oracles). Four further controls bring the corpus to 74 programs (41 accept, 33 reject); the Linux replay records two TS2344/TS2558 primary-position differences when an earlier variable annotation also has type arguments. All four runtime/declaration witnesses, budget refusal, completeness and recorder pass. The leaf remains open.
  - *Verified (2026-10-08):* all 74 pinned verdicts/primary and related diagnostics, four runtime/declaration witnesses and contextual-inference budget refusal pass through the measured API/CLI paths. Format, format check and both-crate all-target Clippy pass; all 1,110 tests in 70 groups, including all 132 ignored oracles in 35 differential suite files. Largest production source: 1,143 lines. Generic binders, dependent defaults and imported class/function signatures are retained; full operators, generic heritage/methods and broader variance remain in their dependent leaves and G-T2.
- [x] **K.4.4 Overloads (L).** Call/construct/method overload selection in `tsc` order, contextual
  signature selection for callbacks, ambiguity and no-match diagnostics with the right code, implementation
  signature compatibility (TS2394).
  - *Failing baseline (2026-10-08):* 57 pinned programs (30 accept, 27 reject), five runtime/declaration witnesses. Linux replay reports 24 checker/primary-diagnostic differences and one anonymous-signature runtime/declaration compile failure; four other runtime/declaration witnesses, completeness and pinned recorder pass. The shared corpus contains 1,727 programs and 147 templates. Production remains unchanged in this baseline.
  - *Interface/order replay (2026-10-08):* the original 57 overload programs and all 74 K.4.3 generic programs pass. Seventeen further controls extend the overload corpus to 74 programs (39 accept, 35 reject) and six runtime/declaration witnesses. Linux reports 12 checker/primary differences and one additional declaration compile failure; completeness, pinned recorder and the five previous runtime/declaration witnesses pass. The leaf remains open.
  - *Verified (2026-10-08):* all 74 pinned verdicts/primary and related diagnostics and six runtime/declaration witnesses pass. Ordered call/construct/method candidates, direct literal specialization, merged interface groups, callback context and hidden implementation compatibility are retained. Format, format check and both-crate all-target Clippy pass; all 1,114 tests in 71 groups, including all 134 ignored oracles in 36 differential suite files. Largest production source: 1,153 lines. G-T4 retains unmeasured interactions; K.4.5 to K.4.7 and final workspace/coverage verification remain open.
- [x] **K.4.5 Type operators (XL).** `keyof`, `typeof`, indexed access, conditional types (distributive),
  `infer`, mapped types with `readonly`/`?` modifiers and `as` clauses, template-literal types, recursive
  aliases within a depth budget; each operator needs the assignability relation extended.
  - *Failing baseline (2026-10-08):* 78 pinned programs (37 accept, 41 reject), three runtime/declaration witnesses. Linux reports 68 checker/primary-diagnostic differences and two runtime/declaration compile failures; the indexed-access runtime witness, completeness and pinned recorder pass. The shared corpus contains 1,822 programs and 152 templates. Production remains unchanged in this baseline.
  - *Literal-order replay (2026-10-08):* the 78-program draft and all runtime/declaration witnesses pass. Four further controls extend the corpus to 82 programs (39 accept, 43 reject); Linux records two template-union diagnostic-order differences. Both pinned recorders, 652 default-library numeric-cache observations, completeness and six operator-budget controls pass. The shared corpus contains 1,826 programs and 152 templates. The implementation draft remains uncommitted and the leaf stays open.
  - *Verified (2026-10-08):* all 82 pinned verdicts/primary and related diagnostics, three runtime/declaration witnesses, both recorders and six operator-budget controls pass. Explicit operator forms, conditional distribution/infer scopes, mapped modifiers/remaps, bounded recursive expansion and pinned numeric template ordering are retained. Format and both-crate all-target Clippy pass; all 1,120 tests in 72 groups, including all 137 ignored oracles in 37 differential suite files. Largest production source: 1,153 lines. G-T3 retains unmeasured combinations; K.4.6/K.4.7 and final workspace/coverage verification remain open.
- [x] **K.4.6 More types (L).** Index signatures, readonly arrays/tuples, `bigint`, `symbol`/`unique
  symbol`, `satisfies`, `as const`, optional/rest/named tuple elements, `unknown`/`never` flow rules,
  `enum`-literal inference, `void` vs `undefined` rules.
  - *Failing baseline (2026-10-08):* 84 pinned programs (38 accept, 46 reject), three runtime/declaration witnesses. Linux reports 51 checker/primary-diagnostic differences and two runtime compile failures; completeness and the pinned recorder pass. The shared diagnostic corpus records 1,910 programs and 163 templates. Production remains unchanged in this baseline.
  - *Supplemental baseline (2026-10-08):* The isolated 16-control supplement expands the corpus to 100 programs (46 accept, 54 reject), with six runtime/declaration witnesses. The 84-program implementation passes all four focused oracle tests and Clippy; its unit replay exposes two legacy opaque-receiver regressions that remain open. Before the supplement fixes, Linux records 13 checker/primary differences, including four compiler panics on interface index signatures, and two runtime/declaration failures. Completeness and the pinned recorder pass. Evidence: `/private/tmp/blueice-k14-linux/blueice-k46-supplement-red-gate.log`. The shared corpus records 1,926 programs (882 accept, 1044 reject) and 164 templates. Production implementation changes remain unstaged; K.4.6 stays open.
  - *Verified (2026-10-08):* All 100 pinned programs (46 accept, 54 reject), exact primary/related diagnostics, six Node/runtime and every-declaration witnesses and the pinned recorder pass. The frozen-source Linux K.0 gate passes format, format check and both-crate all-target Clippy with warnings denied; all 1,124 tests in 73 groups pass, including all 139 ignored oracles in 38 differential suite files. All 47 frozen changed backend files match the host snapshot. Largest production source: 1,153 lines. G-T5 retains unmeasured combinations; K.4.7 and final workspace/coverage verification remain open.
- [x] **K.4.7 Compatibility details (L).** Excess-property checks in every position, weak types,
  `strictFunctionTypes` vs method bivariance, optional vs `undefined`, variance annotations (`in`/`out`),
  `exactOptionalPropertyTypes`.

  - *Failing baseline (2026-10-08):* The pinned corpus contains 78 programs (40 accept, 38 reject), with four runtime/declaration witnesses. Linux records 41 checker/primary-diagnostic differences and two runtime/declaration failures: freshness declaration formatting and variance syntax parsing. Completeness and the pinned recorder pass (two tests pass, two fail). The shared diagnostic corpus records 2,004 programs (922 accept, 1082 reject) and 167 templates. Production remains unchanged from the verified K.4.6 commit `9b292ac12`. Evidence: `/private/tmp/blueice-k14-linux/blueice-k47-red-gate.log`. K.4.7 remains open.

  - *Position baseline (2026-10-08):* Ten further position controls expand the corpus to 88 programs (44 accept, 44 reject), retaining four runtime/declaration witnesses. The original 78-program draft passes all four focused oracle tests and all-target Clippy. Linux replay of the supplement records six missed excess-property rejections in variable/member/bracket/array-element assignments and method/function-property arguments; completeness, pinned recorder and all four runtime/declaration witnesses pass (three tests pass, one fails). A legacy readonly declaration unit assertion still expects the previous one-line formatting and remains open. Evidence: `/private/tmp/blueice-k14-linux/blueice-k47-supplement-red-gate.log`. Implementation changes remain unstaged; K.4.7 stays open.

  - *Verified (2026-10-08):* All 88 pinned programs (44 accept, 44 reject), exact primary/related diagnostics, four Node/runtime and every-declaration witnesses and the pinned recorder pass. The frozen-source Linux K.0 gate passes format, format check and both-crate all-target Clippy with warnings denied; all 1,128 tests in 74 groups pass, including all 141 ignored oracles in 39 differential suite files. All 41 frozen changed backend files match the host snapshot. Largest production source: 1,153 lines. G-T6 retains unmeasured combinations; final K.3/K.4 workspace/coverage verification remains open. M8 remains open for K.5–K.9.

  - *Fixture provenance (2026-10-08):* Ten supplemental fixtures now carry the required MPL-2.0 headers. Their program bodies are unchanged; the pinned compatibility and shared diagnostic recorders regenerate only source positions. The frozen 12-file Linux replay passes format, format check, both-crate all-target Clippy and all eight compatibility/diagnostics tests, including both pinned recorders and all four runtime/declaration witnesses. Production sources and Rust test harness inputs match `4abe6111f`. Evidence: `/private/tmp/blueice-k14-linux/blueice-k47-headers-status.json` and its compatibility/diagnostics logs. Final workspace CI remains open while an unrelated openSUSE repository initialization failure is repaired.

  - *CI bootstrap correction (2026-10-08):* Initial final-source CI [37698111419](https://github.com/ephoton0210/blueice/actions/runs/37698111419) exposes an openSUSE Leap 15.6 x86_64 initialization failure before checkout or Rust build/test: the unused OpenH264 repository cannot connect, so zypper reports exit 106 after installing the required packages. The SUSE bootstrap now disables only `repo-openh264` when its configuration file exists; required repositories and package-install failures retain their existing behavior. The exact corrected CI step succeeds in a fresh Linux openSUSE Leap 15.6 aarch64 container, with all twelve required RPMs verified. Evidence: `/private/tmp/blueice-k47-ci-suse-failure.log` and `/private/tmp/blueice-k47-suse-install.log`. The vendor documents exit 106 as a skipped repository at [zypper(8)](https://manpages.opensuse.org/Leap-15.6/zypper/zypper.8.en.html). Production sources and Rust test harnesses are unchanged. The initial coverage job passes with workspace line coverage 90.65% and independent BlueJS 93.00%; no coverage exclusion is added. A fresh final-source CI run is required after this bootstrap correction.

  - *Final K.3/K.4 verification (2026-10-08):* Final-source CI [37702047861](https://github.com/ephoton0210/blueice/actions/runs/37702047861) on `40de14abd` passes all 29 jobs, including complete workspace build/test/fmt/all-target Clippy across 25 platform configurations and both pinned TypeScript oracle jobs. Workspace line coverage is 90.65% (188,432 lines, 17,621 missed); independent BlueJS line coverage is 93.00% (77,553 lines, 5,425 missed). No coverage exclusion is added. K.3 and the measured K.4 leaves are complete. M8 remains open for K.5–K.9 and the inventory retains unmeasured combinations.

### K.5 Remaining class forms — M8 — gap G-T9

- [x] **K.5.1 Abstract and implements (M).** `abstract` classes/members/constructors, `implements`
  (checked structurally), `override` and `noImplicitOverride`; emit erases all of them.
  - *Failing baseline (2026-10-08):* 95 isolated pinned programs (45 accept, 50 reject), five Node/runtime witnesses and six exact declaration witnesses cover abstract member obligations and constructor types, structural implementations, override rules and imported boundaries. The frozen-source Linux replay records 94 verdict/primary-diagnostic differences and six emission failures; completeness and the pinned recorder pass (two tests pass, two fail). The shared diagnostic corpus contains 2,109 programs (971 accept, 1,138 reject) and 184 templates. Production sources are unchanged from the verified post-Test262 merge source. Evidence: `/private/tmp/blueice-k14-linux/blueice-k51-red-gate.log`. Baseline `05925c6cc` was committed before implementation; final verification follows.
  - *Verified (2026-10-08):* All 95 pinned programs and twelve further constructor capability controls match, including exact primary/related diagnostics; all five Node/runtime and six exact declaration witnesses pass. Abstractness, accessibility and inherited/imported parameters survive constructor aliases; generic constructor assignment uses bounded contextual inference. The frozen-source Linux K.0 gate passes format and both-crate all-target Clippy with warnings denied; all 1,142 tests in 77 groups pass, including all 144 ignored oracles in 41 suite files. Both direct field modes pass; all 46 frozen backend files match the host. Refusal inventory regenerated: 162 sites. Largest changed production source: 1,188 lines. Evidence: `/private/tmp/blueice-k14-linux/blueice-k51-full-status.json` and logs. K.5.2/K.5.3 and M8 remain open.
  - *CI portability correction (2026-10-08):* Full CI [37785330991](https://github.com/ephoton0210/blueice/actions/runs/37785330991) on `30db65f66` exposed the macOS `/var` versus `/private/var` temporary-root alias in the modifier runtime/declaration harness. Canonicalizing the harness root keeps configured output paths within the existing compiler boundary. A Linux replay uses a symlinked `TMPDIR`; cross-platform CI is rerun on the correction.
  - *Final CI verification (2026-10-09):* Corrected-source CI [37797735267](https://github.com/ephoton0210/blueice/actions/runs/37797735267) on `bb580bdf9` passes all 29 jobs: complete workspace build/test/fmt/all-target Clippy across 25 platform configurations, both pinned TypeScript oracle jobs, coverage and the final gate. Workspace line coverage is 95.04% (194,767 lines, 9,664 missed); independent BlueJS line coverage is 99.53% (81,248 lines, 384 missed). No coverage exclusion is added. The macOS oracle temporary-root mismatch is resolved. No BlueTS regression from the merged Test262 branch is observed in these gates. K.5.2/K.5.3 and M8 remain open.
- [ ] **K.5.2 Generic and dynamic classes (L; needs K.4.3).** `class C<T> extends B<T>`, generic methods
  and static members rules, class expressions (named and anonymous, `NamedEvaluation`), computed and
  string-literal member names, index-signature members, `declare` fields.
  - *Failing baseline (2026-10-08):* 83 isolated pinned programs (50 accept, 33 reject), five Node/runtime and six exact declaration witnesses cover every K.5.2 syntax category and imported generic inheritance. The frozen Linux replay records 83 verdict/primary-diagnostic differences and six emission failures; completeness and the pinned recorder pass (two tests pass, two fail). Production remains the verified K.5.1 commit `30db65f66`. Shared diagnostic corpus: 2,192 programs (1,021 accept, 1,171 reject), 190 templates. Evidence: `/private/tmp/blueice-k14-linux/blueice-k52-red-status.json` and `blueice-k52-red-gate.log`. Commit the baseline before implementation; the leaf stays open.
  - *Supplemental baseline (2026-10-09):* Thirteen controls expand the corpus to 96 programs (59 accept, 37 reject), nine runtime and fifteen exact declaration witnesses. The 83-program implementation draft passes its focused oracle and legacy replay. The supplemental Linux replay records four missed TS1166 rejections and failures in eight runtime/declaration fixtures, including a recursive declaration compiler panic; the direct computed-field witness fails in assign mode. Completeness and the pinned recorder pass. The shared diagnostic corpus records 2,205 programs (1,030 accept, 1,175 reject) and 191 templates. Evidence: `/private/tmp/blueice-k14-linux/blueice-k52-supplement-red-matrix.log` and `blueice-k52-supplement-red-direct.log`. This baseline commits only fixtures, harness/recorder metadata and plans; production remains an uncommitted draft and K.5.2 stays open.
  - *Scope baseline (2026-10-09):* Fourteen further controls expand the corpus to 110 programs (69 accept, 41 reject), fourteen runtime and twenty exact declaration witnesses. The 96-program draft passes all four focused oracle tests, both-crate all-target Clippy, the legacy 2,205-program diagnostic replay and both direct field modes. The new Linux replay records six checker/primary differences and one runtime/declaration fixture failure, covering class-expression argument bodies, captured outer generic parameters and inheritance through a class-expression constructor alias. Completeness and the pinned recorder pass; all three direct tests pass. Shared diagnostic corpus: 2,219 programs (1,040 accept, 1,179 reject), 191 templates. Evidence: `/private/tmp/blueice-k14-linux/blueice-k52-96-pass-status.json` and `blueice-k52-scope-red-matrix.log`. This baseline commits only test evidence and plans; production remains an uncommitted draft and K.5.2 stays open.
  - *Capture baseline (2026-10-09):* Twelve controls expand the corpus to 122 programs (77 accept, 45 reject), eighteen runtime and twenty-four exact declaration witnesses. The 110-program draft passes all four focused oracle tests, both-crate all-target Clippy, the legacy diagnostic replay and both direct field modes. The new frozen Linux replay records nine checker/primary differences and failures in three runtime/declaration fixtures for captured named self types, constructor alias chains and inheritance from a generic factory result. Own class parameters and defaults already pass. Completeness and the pinned recorder pass; all three direct tests pass. Shared diagnostic corpus: 2,231 programs (1,048 accept, 1,183 reject), 191 templates. Evidence: `/private/tmp/blueice-k14-linux/blueice-k52-110-pass-status.json` and `blueice-k52-capture-red-matrix.log`. Only test evidence and plans are committed; implementation remains a local draft and K.5.2 stays open.
  - *Import baseline (2026-10-09):* Four cross-module controls expand the corpus to 126 programs (79 accept, 47 reject), retaining eighteen runtime and twenty-four declaration witnesses. On the recorder's frozen 110-program implementation draft, twelve verdict/primary differences remain: nine captured-self/alias differences plus three import differences. An exported class-expression instance loses its private type definition and incorrectly accepts a string assignment from a numeric field; inherited imported constructors also lose their class surface. Three runtime/declaration fixtures still fail. Completeness, pinned recorder and all direct tests pass. Shared diagnostic corpus: 2,235 programs (1,050 accept, 1,185 reject), 191 templates. Evidence: `/private/tmp/blueice-k14-linux/blueice-k52-import-red-status.json`, `blueice-k52-import-red-frozen-source-hashes.json` and `blueice-k52-import-red-matrix.log`. Only fixtures and plans are committed; newer local captured-self/alias edits are excluded from this failing replay. K.5.2 remains open.
  - *Shadow baseline (2026-10-09):* Nine controls expand the corpus to 135 programs (84 accept, 51 reject), twenty runtime and twenty-six exact declaration witnesses. The 126-program draft passes both-crate all-target Clippy, all four focused oracle tests, the legacy 2,235-program diagnostic replay and both direct field modes; source hashes match all 74 changed backend files. The new frozen Linux replay records seven checker/primary differences and failures in two runtime/declaration fixtures: distinct same-named class/method binders are conflated, a returned class's `this` is incorrectly checked in its enclosing function, computed-key `this` needs TS2465, and nested declaration binders require TypeScript's `T_1` spelling. TS2719 retains its TS2208 related origin. Completeness, pinned recorder and all direct tests pass. Shared diagnostic corpus: 2,244 programs (1,055 accept, 1,189 reject), 193 templates. Evidence: `/private/tmp/blueice-k14-linux/blueice-k52-126-pass-status.json` and `blueice-k52-shadow-red-matrix.log`. Only failing test evidence and plans are committed; production remains an uncommitted draft and K.5.2 stays open.
  - *Assignment-origin baseline (2026-10-09):* Eight adjacent controls expand the corpus to 143 programs (90 accept, 53 reject), retaining twenty runtime and twenty-six exact declaration witnesses. The 135-program draft passes all four dynamic oracle tests, including pinned recording, runtime and declaration comparisons. Its full replay also finds a static-block output whitespace regression; that correction remains an uncommitted draft. An isolated pinned recorder and public-CLI replay of the eight controls record one primary difference (TS2322 instead of TS2719 for a same-named method-local assignment) and two missing TS2208 related origins at assignment/argument boundaries. Six controls for ordinary and union class receivers, inferred/shadowed inherited returns, captured defaults and named-self constructor queries already pass. The shared corpus contains 2,252 programs (1,061 accept, 1,191 reject), 193 templates. Evidence: `/private/tmp/blueice-k14-linux/blueice-k52-shadow-controls-red.json`, the isolated recorder logs and the frozen 79-file source snapshot. The two receiver controls use `dynamic-shadow-this-*` names so legacy class discovery does not also claim them. Only test evidence and plans are committed; K.5.2 stays open until the corrected final source passes the full two-crate gate.
  - *Static/local binder baseline (2026-10-09):* Eight controls expand the pinned corpus to 151 programs (95 accept, 56 reject), twenty-two runtime and twenty-eight exact declaration witnesses. The preceding frozen 83-file implementation completes the full two-crate gate with 1,149 passing tests and one recorder-order failure; format, all-target Clippy and every other ignored/live oracle pass. Recorder metadata now uses directory-name sorting, with all existing observations unchanged. An isolated pinned recorder and unchanged public Linux CLI record eight new verdict/primary differences: static generic arrow/function binders are confused with class parameters, callable annotation binders are missing from lexical lookup, and a method binder rewrites a captured outer function parameter through a local named self type, including one unsafe false acceptance. The rejection retains TS2322 and its TS2208 origin. The shared corpus contains 2,260 programs (1,066 accept, 1,194 reject), 193 templates. Evidence: `/private/tmp/blueice-k14-linux/blueice-k52-static-local-red.json`, its frozen 83-file hashes, the isolated pinned recorder logs and `blueice-k52-143-code-complete-full-status.json`. Only fixtures, recorder metadata and plans are committed before the binder correction; K.5.2 stays open.
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
