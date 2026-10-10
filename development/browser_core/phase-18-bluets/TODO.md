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

**Current leaf: K.7.3 output options (M9). The K.7.2 CLI loader split is verified: main CLI 945 lines, loader 276 lines. K.7.1 and its Windows regex
deadline correction pass the full K.0 gate and all 29 hosted CI jobs on
`93622d64f` (run [38027752712](https://github.com/ephoton0210/blueice/actions/runs/38027752712)).
Workspace line coverage is 94.68%; independent BlueJS is 99.30%.
M8 workspace and complete hosted CI are verified on `1f76ff28c`.** Sections A to J are complete (summarized below); J.6 closed in its
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
- [x] **K.5.2 Generic and dynamic classes (L; needs K.4.3).** `class C<T> extends B<T>`, generic methods
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
  - *Expression-target baseline (2026-10-09):* Nine controls expand the corpus to 160 programs (103 accept, 57 reject), thirty runtime and thirty-six exact declaration witnesses. The preceding frozen 88-file implementation passes format, both-crate all-target Clippy and all 1,150 tests in 79 groups, including all 146 ignored oracles in 42 suite files. An independent ES2020 replay then records eight runtime failures, five declaration differences and one checker rejection for static fields/blocks, private fields/methods, derived and nested class expressions, and per-evaluation computed keys. Internal class identities leak into after-class JavaScript and private stores escape expression scope. Literal assertions in a computed-key getter lose their target; the invalid assertion control reports TS2322 instead of TS2352. Empty instance/export declarations also need exact TypeScript formatting. Shared diagnostic corpus: 2,269 programs (1,074 accept, 1,195 reject), 194 templates. Evidence: `/private/tmp/blueice-k14-linux/blueice-k52-151-pass-full-status.json`, its frozen 88-file source hashes, `blueice-k52-expression-target-red.json` and isolated pinned recorder logs. The baseline commits fixtures, target-aware harness, direct witnesses and plans before lowering changes. K.5.2 remains open until these supported-target boundaries pass.
  - *Verified (2026-10-09):* All 160 pinned programs (103 accept, 57 reject), exact primary/related diagnostics, 30 Node runtime and 36 exact declaration witnesses, the pinned recorder and four direct class tests pass. Generic identities, captured self/constructor aliases, computed keys, index contracts and `declare` erasure compose with both field modes. ES2020 expression wrappers retain per-evaluation private/static stores and contextual names; original assertion spans reach the direct AST route. The frozen-source Linux K.0 gate passes format and both-crate all-target Clippy with warnings denied; all 1,151 tests in 79 groups pass, including all 146 ignored oracles in 42 suite files. All 90 changed backend hashes match the tested source. Shared diagnostic corpus: 2,269 programs, 194 templates. Generated inventory: 166 refusal sites in ten areas; largest changed production source: 1,194 lines. Evidence: `/private/tmp/blueice-k14-linux/blueice-k52-final-full-status.json` and its frozen source hashes. G-T9 retains K.5.3 and unmeasured combinations; K.5.3/M8 and final hosted workspace/coverage verification remain open.
  - *Hosted CI correction (2026-10-09):* Initial workspace CI [37845418066](https://github.com/ephoton0210/blueice/actions/runs/37845418066) on `8132479c4` passes both pinned BlueTS oracle jobs but exposes an IPC fixture race on macOS 26 arm64: concurrent allocations use the same timestamp and `create_dir` fails with `AlreadyExists`. The test helper adds an atomic sequence; all 136 IPC tests, its sixteen-thread isolation/cleanup regression, format and all-target Clippy pass on Linux. Production IPC behavior and coverage exclusions are unchanged. Evidence: `/private/tmp/blueice-k14-linux/blueice-k52-ipc-fixture-status.json` and the retained macOS failure log. Fresh final-source hosted CI follows the K.5.3 push.
- [x] **K.5.3 Retire class refusals (S).** Remove the refusals K.5.1/K.5.2 make obsolete (redeclaring a
  member of an imported base, accessor type mismatch, definite assignment through a branch, getter without
  annotation) and move their fixtures into the class matrix.

  - *Failing baseline (2026-10-09):* 35 isolated pinned programs (18 accept, 17 reject), 16 Node and 17 exact declaration witnesses cover independent accessor read/write types, inferred getters, imported field/accessor/method redeclarations and constructor branch initialization. On verified K.5.2 source `8132479c4`, the public CLI/Rust replay records 27 verdict/primary differences, 14 emission failures and two declaration-format differences; completeness and the pinned recorder pass. All three direct regression tests fail, including an unsafe acceptance when a constructor returns before initializing its field. The shared corpus records 2,304 programs (1,092 accept, 1,212 reject), 195 templates; all prior observations are unchanged. Evidence: `/private/tmp/blueice-k14-linux/blueice-k53-baseline/` (`rust-baseline-status.json`, `runtime-red-status.json`, `direct-red-status.json` and frozen source hashes). No production implementation is included in this baseline. The derived parameter-property branch refusal remains separate: pinned TypeScript rejects assignment mode with TS2401, but accepts ES2022 define mode and emits a `this.extra` assignment before `super`, which throws ReferenceError under Node. Existing deferred coverage preserves this measured limitation. K.5.3 remains open.

  - *Verified (2026-10-09):* All 35 pinned programs, exact primary/related diagnostics, 16 Node runtime and 17 exact declaration witnesses, the live recorder and all three direct regressions pass. Accessor read/write types survive generic and imported surfaces; imported member kinds retain override diagnostics; bounded merged interface heritage contributes erased members; constructor branches and early returns enforce definite initialization. Three obsolete deferred class fixtures return to the ordinary matrix; the measured branch-super parameter-property limitation remains. The frozen-source Linux K.0 gate passes format and both-crate all-target Clippy with warnings denied; all 1,158 tests in 81 groups pass, including all 148 ignored oracles in 43 suite files. All 99 backend hashes match. The strict replay metadata correction changes no verdict or diagnostic in the 2,304-program, 195-template shared corpus and preserves every prior 2,269 observation. Generated inventory: 162 refusal sites in ten areas; largest changed production source: 1,196 lines. Evidence: `/private/tmp/blueice-k14-linux/blueice-k53-final-full-status.json`, `blueice-k53-final-full-report.json`, frozen hashes and `blueice-k53-metadata/comparison.json`. K.5 is locally verified; K.6–K.10, M8 and final hosted workspace/coverage verification remain open.

### K.6 Module syntax — M8 — gap G-T10

- [x] **K.6.1 Default and re-exports (M).** `export default <expression>`, anonymous default function and
  class, `export { x } from`, `export * from`, `export * as ns from`; ESM and CommonJS emit (including
  live bindings through re-export and the `__exportStar`-style semantics written from the spec).
  - *Failing baseline (2026-10-09):* 74 pinned programs (56 accept, 18 reject), 41 Node and 47 declaration witnesses cover default expressions and anonymous/named declarations, alias/snapshot behavior, named/star/namespace re-exports, transitive surfaces, diamonds, conflicts, cycles, evaluation and imported identities under ESM/CommonJS. On verified K.5.3 source `13079a4f3`, the public CLI replay records 74 verdict/primary differences and 47 emission failures; completeness and the live recorder pass. All 41 pinned Node programs run successfully before BlueTS failures are collected. Three direct export regressions fail; the declaration-only runtime-authority control passes. Format and both-crate all-target Clippy pass; all 217 frozen backend hashes match. Shared corpus: 2,378 programs, 198 templates, 1,148 accept, 1,230 reject; every prior 2,304 observation is unchanged. Two embedded fixture roots are normalized exactly as in the existing shared replay, with all codes, positions, related information and verdicts retained. Evidence: `/private/tmp/blueice-k14-linux/blueice-k61-baseline/` (`report.json`, `rust-baseline-status.json`, `comparison.json`, `normalization-comparison.json` and source hashes). The baseline contains no production change and stays unpushed until the implementation passes K.0. Twelve isolated probes retain a pinned limitation: ES2020 anonymous default classes with private fields are accepted but fail under Node in both module and field modes; named private and anonymous public-field controls run normally. The runtime matrix uses valid public-field witnesses without weakening comparisons. K.6.1 remains open; K.5.3 final-source CI [37859872555](https://github.com/ephoton0210/blueice/actions/runs/37859872555) is still running on its exact pushed SHA.
  - *Verified (2026-10-09):* All 74 pinned programs (56 accept, 18 reject), exact primary/related diagnostics, 41 Node and 47 exact declaration witnesses, the live recorder and five direct tests pass. Incremental re-exports refresh reused and changed dependency types; anonymous default function/class names and identifier snapshot versus live alias semantics are verified. The existing direct arrow refusal and the measured ES2020 anonymous-private default-class refusal remain, with positive controls. Five further pinned Node controls confirm emitted default names and snapshots. The frozen-source Linux K.0 gate passes format, both-crate all-target Clippy with warnings denied, all 1,019 ordinary tests, and the complete 1,169-test run in 83 groups, including all 150 ignored oracles in 44 suite files. All 255 backend hashes match. Shared corpus: 2,378 programs, 198 templates; generated inventory: 160 refusal sites in ten areas. No diagnostic allowance or runtime grant is added. Evidence: `/private/tmp/blueice-k14-linux/blueice-k61-full-fifth-{status,report}.json`, frozen hashes/logs, committed private/cache/name failing regressions and `blueice-k61-default-names-reference/report.json`. Parser/emitter reach 1,207/1,208 lines; the queued split must be separately tested, committed and pushed before K.6.2. Hosted full-workspace/platform/coverage verification remains open.
  - *Generator failing regression (2026-10-09):* Four supported synchronous and four retained async-refusal controls are accepted, executed and declaration-emitted by pinned TypeScript 5.9.3/Node before collecting BlueTSC differences on `d8b9e6a68`. Only CommonJS anonymous synchronous default generation fails: emitted `function default_1*` is invalid JavaScript; ESM and named controls match exact output/declarations. Async generators retain the mandatory frontend refusal and publish no output. Format and both-crate all-target Clippy pass, all five frozen backend hashes match. Evidence: `/private/tmp/blueice-k14-linux/blueice-k61-generator-red-{status,report}.json`, logs/hashes and the independent `blueice-k61-generator-probe.json`. This test-only baseline is committed before the insertion fix and remains unpushed until K.0 passes. K.6.1 reopens for this correction; K.6.2 remains open.
  - *Generator correction verified (2026-10-09):* The CommonJS synthetic name is inserted after the generator star. All four supported synchronous controls match pinned Node names/output and exact declarations; all four async controls retain the mandatory refusal and no output publication. `module-exports-v2.1` enters artifact/cache identity. The integrated frozen nine-file Linux gate passes format, all-target Clippy with warnings denied, 1,019 ordinary and all 1,170 BlueTS/bridge tests in 84 groups, including every 151 ignored oracle in 45 suite files, plus 139 IPC and 20 real core subprocess tests. All hashes match; inventory remains 160 sites in ten areas and all BlueTS production sources stay below 1,200 lines. Evidence: `/private/tmp/blueice-k14-linux/blueice-k61-generator-ci-{status,report}.json` and logs/hashes. The IPC frame-prefix regression is separately committed before its repair. K.6.1 closes again; K.6.2/K.6.3 and full hosted CI remain open.
  - *Hosted CI fixture correction (2026-10-09):* Full CI `37879746425` on `5e5bcb241` exposes the SLE x86_64 HTTP origin-limit fixture assumption that successive freed ephemeral ports are distinct. A Linux probe reproduces duplicate ports. Reserve all 17 listeners together, assert distinct ports, then close them before the real requests; retain all fetch/origin-limit assertions. Format, engine all-target Clippy, all 507 engine tests and 20 focused repetitions pass on the frozen source, with launcher binaries built first. Evidence: `/private/tmp/blueice-k14-linux/blueice-k6-ci-origin-final-{status,report}.json`, probe/repeat records and logs/hashes. Final-source full hosted CI remains open.
- [x] **K.6.2 Type-only and attributes (S).** All `import type`/`export type` forms, inline `type`
  modifiers, import attributes (`with { type: "json" }`), `export as namespace`.
  - *Failing baseline (2026-10-09):* On verified source `d8b9e6a68`, 86 pinned programs (63 accept, 23 reject), 54 successful Node references and 60 declaration witnesses retain all named/default/namespace/import-equals and inline type forms, arbitrary module names, type queries, type-only stars/namespaces, attributes, valid JSON imports and UMD namespace restrictions. The public CLI records 68 verdict/primary differences and 63 emission differences across 59 witnesses. Two direct graph regressions fail; ordinary type-only declaration elision and runtime-authority refusal controls pass. Format, both-crate all-target Clippy, live recorder and shared completeness pass; all 257 frozen backend hashes match. The shared corpus now has 2,464 programs and 211 templates (1,211 accept, 1,253 reject); every prior 2,378 complete observation is unchanged. Evidence: `/private/tmp/blueice-k14-linux/blueice-k62-baseline-final-{status,report}.json`, logs/hashes and `blueice-k62-shared-comparison.json`. Explicit inline imports of `.d.ts` files retain pinned TS2846; valid extensionless elision controls remain. ESNext accepts the measured legacy assertion and duplicate-attribute controls; observations are not replaced by assumed restrictions. Positive JSON witnesses require owner-bounded resolution/asset emission before this leaf can close. No production change; this baseline stays unpushed until its implementation passes K.0. The separately measured K.6.1 generator output defect takes priority. K.6.2 remains open.
  - *Resolution-mode failing regression (2026-10-09):* Two additional pinned programs select import/require package conditions for the same source/specifier, directly and through type-only re-exports. Both TypeScript/Node controls run to `3` and emit declarations before collecting BlueTSC failures on production baseline `46f908786`; BlueTSC selects the import type for both and reports four TS2322 errors. Format, both-crate all-target Clippy and all six test hashes pass; all 23 draft production files are restored from the committed baseline for this replay. Evidence: `/private/tmp/blueice-k14-linux/blueice-k62-resolution-red-{status,report}.json`, logs/hashes and both independent condition probes. Per-edge resolution mode, canonical owner bounds and fingerprints are required before K.6.2 closes; this supplemental test-only baseline stays unpushed until complete implementation verification.
  - *Relative-resolution/identity failing baseline (2026-10-09):* Pinned trace confirms `.ts` → `.tsx` → `.d.ts`. Public resolver tests require this priority, earlier-candidate invalidation and canonical-root symlink refusal. The public CLI artifact fingerprint must change with an observed manifest even if its chosen source does not. On committed production `10e688937`, format/all-target Clippy pass; 14 controls pass and all three new assertions fail. All 5,176 backend hashes match the test-only snapshot; no production draft is included. Evidence: `/private/tmp/blueice-k14-linux/blueice-k62-relative-red3-{status,report}.json`, frozen hashes and trace. Commit before production; keep unpushed until K.0 passes.
  - *JSON asset failing controls (2026-10-09):* Before JSON production support, three public owner-CLI controls require original asset bytes, nested JSON-derived types, content-sensitive artifact fingerprints, explicit effective `resolveJsonModule`, malformed-input refusal and canonical symlink bounds. The disabled-option control passes; valid asset and canonical-root controls fail because JSON is unresolved. Format/all-target Clippy pass; all 317 frozen draft-source hashes match. The existing 86-program committed pinned baseline supplies the independent positive JSON references. Evidence: `/private/tmp/blueice-k14-linux/blueice-k62-json-red-{status,report}.json` and logs/hashes. This additional test-only commit precedes JSON production work and stays unpushed until K.0 passes. K.6.2 remains open.
  - *JSON direct-entry failing regression (2026-10-09):* The complete 86-program pinned matrix now passes all verdict/primary/related, 54 Node and 60 exact declaration witnesses; the two conditional-resolution programs, three JSON asset controls and 18 resolver/cache controls pass. A new direct test on the JSON-support draft exposes an empty executable graph for a JSON entry; the runtime-import refusal and four earlier direct controls remain. Format/all-target Clippy pass; all 322 hashes match. Evidence: `/private/tmp/blueice-k14-linux/blueice-k62-sixteenth-{status,report}.json` and logs/hashes. Commit the public refusal witness before correcting that entry guard; the full K.0 and hosted CI gates remain open.
  - *Contextual import failing regression (2026-10-09):* Four additional pinned 5.9.3 programs accept `type` as an ESM/CommonJS default binding and a CommonJS value/type-only `import =` binding. All four independent Node programs print `42` and emit declarations before BlueTS reports parser failures. The eighteen-round frozen gate passes format/all-target Clippy and restores class discovery, all 18 CLI controls, all 123 frontend controls and the pure-type namespace `typeof` refusal. One inline `type Missing` primary span still needs refinement. Evidence: `/private/tmp/blueice-k14-linux/blueice-k62-eighteenth-{status,report}.json` and logs/hashes. Commit this public contextual baseline before changing its parser; K.6.2 and the full gates remain open.
  - *JSON parser budget failing regression (2026-10-09):* Four public closed-loader controls retain Unicode asset bytes and owner source-byte, token and schema-depth bounds. Byte/depth refusal and the positive control pass; JSON ignores `max_tokens = 2` and incorrectly emits an asset. Format and both-crate all-target Clippy pass, and all 326 frozen backend hashes match. The preceding complete gate executes all 1,187 tests in 90 groups with no ignored tests left: 1,186 pass and only the shared diagnostics live recorder fails. Both shared record files have 28 ordering differences after two fixture renames, with identical 2,464-case identity sets. Evidence: `/private/tmp/blueice-k14-linux/blueice-k62-json-limits-red-{status,report}.json`, `blueice-k62-twentieth-{status,report}.json` and `blueice-k62-diagnostics-order-proof.json`, logs/hashes. The test-only commit precedes enforcement of the existing token limit; source ordering and the complete K.0/final-source hosted gates remain open.
  - *JSON default-property failing regression (2026-10-09):* Two additional pinned 5.9.3 programs distinguish a JSON module's default object from the object's own `default` property. The accepted reference prints `42` and emits an inferred `number` declaration; the rejected string assignment reports exact TS2322 text/location and emits nothing. BlueTS instead reports TS2339 on both property accesses. Format and both-crate all-target Clippy pass; all 327 frozen backend hashes match. The prior complete frozen gate passes all 1,191 tests in 91 groups, including every 155 ignored oracle, and all 1,036 ordinary tests; the actual 2,464-case/211-template live diagnostics record is byte-identical after ordering correction. Evidence: `/private/tmp/blueice-k14-linux/blueice-k62-json-default-red-{status,report}.json`, `blueice-k62-final-first-{status,report}.json` and independent `blueice-k62-json-default-probe/` records, logs/hashes. Commit this failing public baseline before protecting the module default binding from field-name collisions. K.6.2 and final-source hosted CI remain open.
  - *Verified (2026-10-09):* All 86 pinned matrix programs (63 accept, 23 reject), 54 Node and 60 exact declaration witnesses pass, together with eight supplemental pinned resolution-mode/contextual-name/JSON-default programs, five direct controls and public JSON asset, resource-limit, canonical-root and observation/cache controls. Type-only syntax is erased only from JavaScript; declarations preserve its static surfaces. JSON uses the existing lexer token budget, retains original asset bytes and protects the module default binding from an object field named `default`. Runtime-profile refusals remain. Two fixture renames are reflected in deterministic shared record ordering without changing any recorded observation. The frozen-source Linux K.0 gate passes format, both-crate all-target Clippy with warnings denied, 1,036 ordinary tests and all 1,192 tests in 92 target/doctest groups, including every one of the 156 ignored oracles in 49 suite files. All 327 frozen backend hashes match. The actual shared diagnostics record is byte-identical to the committed 2,464-program/211-template record. Evidence: `/private/tmp/blueice-k14-linux/blueice-k62-final-second-{status,report}.json`, frozen hashes and logs. Generated inventory: 161 refusal sites in 11 areas. Largest production source: `compiler.rs`, 1,224 lines; execute queued K.6.R.2 as a separate tested commit/push before K.6.3. Full hosted workspace/platform/coverage CI remains open, including the unresolved macOS real-core navigation timeout on prior source.
- [x] **K.6.3 Ambient and augmentation (L).** `declare module "x" { … }` (ambient and augmentation),
  `declare global`, global augmentation from a module, triple-slash `reference path/types/lib`,
  shorthand ambient modules; all under the J.4 authority rules (a declaration never creates a runtime
  binding).
  - *Failing baseline (2026-10-09):* On production `4e975cef`, 48 pinned TypeScript 5.9.3 programs (28 accept, 20 reject) retain ambient named/implicit/merged/shorthand modules, module/global augmentation and `reference path/types/lib`. All 28 independent Node/declaration controls run before BlueTS comparison. The public CLI reports 46 verdict/primary differences and 26 emission fixture differences. Format and both-crate all-target Clippy pass; completeness and both pinned recorders pass. Shared replay records 74 diagnostic and 20 presentation differences. All 157 frozen backend hashes match. The shared corpus contains 2,512 programs and 216 templates; all prior 2,464 full observations, templates and 638 strict replay roots are unchanged. Evidence: `/private/tmp/blueice-k14-linux/blueice-k63-baseline-final-{status,report}.json`, `blueice-k63-corpus-baseline-report.json`, `blueice-k63-shared-comparison.json` and `blueice-k63-baseline-final-oracle-reuse.json`, logs/hashes. No production change; commit this test baseline before implementation and keep it unpushed until K.0 passes. K.6.3 and full hosted CI remain open.

  - *Imported-global baseline supplement (2026-10-09):* Four imported-global context controls and the required MPL headers expand the pinned ambient corpus to 52 programs (30 accept, 22 reject), and the shared corpus to 2,516 programs with the same 216 templates. Previous observations retain their exact content after header position adjustment. The frozen draft passes format and both-crate Clippy; its 36 ambient checking, 44 shared diagnostic and 14 presentation differences keep K.6.3 open. See PLAN.md and `/private/tmp/blueice-k14-linux/blueice-k63-context-red-report.json` for the measured scope. Commit only the test supplement before its production repair.

  - *Global augmentation cache baseline (2026-10-09):* Three public compiler controls expose incorrect incremental reuse after either a global augmentation or its imported type changes. TypeScript and fresh BlueTS reject the unconnected consumer with TS2322, while the incremental draft accepts both; the alias-scope TS2304 control passes. The frozen 5,368-input gate passes format and both-crate all-target Clippy, with one test passing and two failing. The earlier complete draft replay runs all oracles: 1,192 pass and four known ambient/shared tests fail, zero ignored, in 93 groups. See PLAN.md and `blueice-k63-cache-red-report.json`. Commit the failing cache tests before production invalidation repair. K.6.3 remains open.

  - *Global augmentation cache draft (2026-10-09):* Following baseline `5086a2a85`, the old/new dependency closure invalidates consumers after an affected global augmentation or its transitive import changes. The frozen 5,368-input Linux gate passes format, both-crate all-target Clippy and all three regressions. Ordinary replay has 1,038 pass, three known ambient/shared failures and 158 ignored in 94 groups, with no additional failing target. See PLAN.md and `blueice-k63-cache-green-report.json`. Production remains an uncommitted draft; named ambient modules, module augmentation, references and the complete K.0 gate remain open.


  - *Static graph/cache baseline (2026-10-09):* The frozen draft now matches all 52 ambient/reference programs, 30 Node/declaration witnesses, shared diagnostics and three global-cache regressions. Six independent pinned graphs agree with fresh BlueTS; incremental referenced globals still reuse an accepted, unconnected consumer. Named and module-augmentation cache controls pass, as do four exact owner-edge/resource/runtime-authority controls. The frozen 5,374-input baseline passes format and both-crate Clippy, with six new tests passing and one failing; all source hashes match. Evidence: `blueice-k63-static-red-report.json`, `blueice-k63-reference-first-report.json` and `blueice-k63-static-cache-probe/report.json`. Commit the failing static-cache tests before repair and keep them unpushed until K.0 passes. K.6.3 and complete hosted CI remain open.

  - *Reference cache/library baseline (2026-10-09):* The frozen reference invalidation draft passes all seven static controls, three earlier cache regressions and all 1,048 ordinary tests in 96 groups, with 158 ignored oracles. A further pinned TypeScript probe verifies the 100 library selector labels and accepted `dom`/`es5` controls. The new public refusal test fails because a known, unavailable library is incorrectly reported as absent (TS2726); four other boundary controls and three earlier cache controls pass. Format and both-crate Clippy pass, all source hashes match. See PLAN.md, `blueice-k63-static-green-report.json`, `blueice-k63-library-red-report.json` and the library catalog probe. Commit this test-only baseline before repair; full K.0 and hosted CI remain open.

  - *Identical referenced-root baseline (2026-10-09):* The known-library refusal repair passes. Two new public regressions fail when a canonical declaration with identical contents is selected by both a path reference and owner/project roots; pinned TypeScript accepts both root arrangements. The frozen 5,375-input gate passes format and both-crate Clippy, with five boundary controls and three earlier cache controls passing, two identical-root controls failing, and all source hashes matching. Metadata proof records 97 unique labels from 100 pinned library entries. See PLAN.md, `blueice-k63-reference-roots-red-report.json` and its independent probe. Commit the test-only baseline before the deduplication repair; K.6.3 and full hosted CI remain open.

  - *Ambient re-export baseline (2026-10-09):* The earlier frozen draft passes all 1,209 tests in 96 groups, including every 158 ignored oracle, with matching hashes and a byte-identical 2,516-case recorder. Independent composition controls expose failed type-only re-exports from named ambient surfaces. Twenty-four pinned additions expand the matrix to 76 programs (42 accept, 34 reject), with 42 Node/declaration witnesses, and the shared corpus to 2,540 programs with the unchanged 216 templates. Every prior full observation is unchanged. The expanded frozen 5,475-input baseline passes format/Clippy, completeness/live recording, but checking and emission fail on the new forms. See PLAN.md, `blueice-k63-reexports-red-report.json` and corpus/probe evidence. Commit the test supplement before static export propagation repair; keep it unpushed until expanded K.0 passes. Original hosted CI is terminal with 27/29 success: macOS 15 Intel navigation and the final CI gate fail; coverage and both pinned oracle jobs pass. K.6.3 and full hosted CI remain open.


  - *Exported ambient value baseline (2026-10-09):* The 76-program draft passes all 1,209 tests in 96 groups, including every 158 ignored oracle, with matching hashes and a byte-identical 2,540-case recorder. Four pinned controls expand the ambient corpus to 80 programs (44 accept, 36 reject), 44 Node/declaration witnesses, and the shared corpus to 2,544 programs/217 templates; every prior full observation is unchanged. The final frozen 5,492-input baseline passes format and both-crate Clippy; public controls have one pass and two failures, and expanded checking/emission fails. Checked misses TS1362/1377, valid type queries retain runtime imports, and type-export declarations change source quotes. TranspileOnly retains its explicit unchecked policy and receives no ambient physical resolution. See PLAN.md, `blueice-k63-runtime-red-third-report.json` and independent recorder/probe evidence. Commit this test-only supplement before repair, keep it unpushed until expanded K.0 passes. K.6.3 and full hosted CI remain open.


  - *Exported-type correction draft gate (2026-10-09):* Following baseline `f86740e44`, Checked value use retains TS1362/1377 export origins, legal value-query imports are elided from JavaScript, declarations preserve source quotes, and `module-exports-v4` enters cache identity. All 80 pinned ambient programs, 44 Node/declaration witnesses and three public controls pass. An independent pinned proof corrects one old quote-normalizing unit expectation. Final format, both-crate all-target Clippy, 16 static/cache controls and all 1,054 ordinary tests in 97 groups pass; the complete K.0 gate passes all 1,212 tests in 97 groups, including every 158 ignored oracle, with zero failures and zero ignored tests. All 5,492 frozen hashes match. See PLAN.md, `blueice-k63-runtime-final-second-{status,report}.json` and retained earlier failure reports. Inventory is regenerated to 162 sites/11 areas; largest production source is 1,182 lines. The 2,544-case live diagnostic record is byte-identical. K.6.3 is complete in its measured scope; commit/push its implementation and baselines. M8 workspace and complete hosted CI remain open.


#### K.6 source refactoring queue

- [x] **K.6.CI.R Split native debugger execution control.** The CI root-refusal repair leaves `backend/core/engine/src/script/javascript/debugger_support.rs` at 1,266 lines, near the requested 1,300-line limit. Commit/push the verified refusal repair first, then move entry/root execution-control methods into a focused child module without changing opaque identities, validation, scheduling, capability grants or source-free replies. Test and commit/push this refactor separately before more production work.
  - *Verified (2026-10-09):* Move the entry/root execution-control methods into `debugger_support/execution_control.rs`. Mechanical comparison preserves every moved method byte and string literal; the parent only removes those methods and declares the child module. Parent source is 959 lines and child source is 317 lines. The independent frozen five-file Linux gate passes format, engine all-target Clippy with warnings denied, launcher binary build, the public completed-module/classic regression and all 508 engine tests. All hashes match. Evidence: `/private/tmp/blueice-k14-linux/blueice-k6-root-refactor-final-{status,report}.json`, frozen hashes/logs and `blueice-k6-root-refactor-mechanical-report.json`. This refactor gets a separate commit/push; full normal final-source hosted CI remains open.

- [x] **K.6.R.2 Split closed-project loading and fingerprint construction.** K.6.2's production audit measures `compiler.rs` at 1,224 lines, past the 1,200-line review threshold. Finish and commit/push the verified K.6.2 leaf first. Then move `ProjectBuilder` and graph-loading state into a focused compiler child module and fingerprint construction into another child module, preserving public APIs, canonical owner bounds, per-edge resolution modes, observation identity, incremental reuse and source provenance. Run the complete K.0 gate and commit/push the refactor separately before K.6.3.
  - *Verified (2026-10-09):* Mechanical comparison after formatting preserves graph-loader and fingerprint bodies and string literals, apart from required `pub(super)` visibility. Source sizes are `compiler.rs` 756, `project_builder.rs` 348 and `fingerprint.rs` 140 lines; largest production source is the CLI at 1,165 lines. Public APIs, canonical bounds, per-edge modes, resolver observations, work limits, cache identity/reuse and source provenance are preserved. The frozen Linux gate passes format, BlueTS/bridge/engine all-target Clippy with warnings denied, all 507 engine tests, 1,036 ordinary BlueTS/bridge tests and all 1,192 tests in 92 groups, including every 156 ignored oracle in 49 suite files. All 331 backend hashes match, and the actual 2,464-program diagnostics record is byte-identical. Evidence: `/private/tmp/blueice-k14-linux/blueice-k62-refactor-final-{status,report}.json`, logs/hashes and `blueice-k62-refactor-mechanical-report.json`. Inventory remains 161 sites in 11 areas. Full hosted workspace/platform/coverage CI remains open; test-only navigation/HTTP diagnostics are integrated for the unresolved macOS timeout, without changing assertions or timeouts.

- [x] **K.6.R.1 Split module-declaration AST records and declaration emission.** After the verified K.6.1 implementation commit/push, move those responsibilities out of `parser.rs` (1,207 lines) and `emitter.rs` (1,208 lines), preserving the public API, emission and source provenance. Run K.0's complete two-crate gate, then make a separate English refactor commit and push before K.6.2.
  - *Verified (2026-10-09):* Module records retain their public re-exports in `parser.rs`; declaration emission and its export/type-parameter helpers move to `emitter/declarations.rs`. Mechanical comparison preserves every AST field, declaration code body and string literal. Source sizes are parser 1,150, module records 68, emitter 863 and declarations 357 lines; all BlueTS production sources stay below 1,200. The complete frozen four-file K.0 gate passes format, both-crate all-target Clippy with warnings denied and all 1,169 tests in 83 groups, including every one of the 150 ignored oracles in 44 suite files. All source hashes match. Evidence: `/private/tmp/blueice-k14-linux/blueice-k6-refactor-final-{status,report}.json`, logs/hashes and `blueice-k6-refactor-mechanical-report.json`. Refusal inventory regenerated to 160 sites in ten areas. The refactor gets its own commit/push; K.6.2/K.6.3 and hosted complete CI remain open.

**M8 final verification is complete (2026-10-10).** Final source `1f76ff28c`
passes the frozen Linux workspace gate: all-target build, 6,861 tests in 504
groups, three Intl Node matrices, all-target workspace Clippy and format;
all 5,493 backend/build inputs match. K.6.3's separate K.0 gate runs every
158 BlueTS/bridge oracle (1,212 passing tests). Original final-source CI
[37949582570](https://github.com/ephoton0210/blueice/actions/runs/37949582570)
passes all 29 jobs, with all required build/test/lint/Intl/format steps across
25 platforms, both pinned oracle jobs, coverage and the final gate. Each
hosted oracle executes all 158 tests with zero failures and zero ignored.
Workspace line coverage is 94.77% (201,590 lines, 10,552 missed); independent
BlueJS is 99.53% (81,248 lines, 384 missed), with no new coverage exclusion.
The per-face font change preserves selection, metrics and navigation
assertions/timeouts. Earlier K.6.3 source `e75d62128` also passes all 29 original
CI jobs. Successful isolated/full font diagnostics do not establish the prior
timeout's inner cause or prove the font change necessary. See PLAN.md and
`blueice-font-cold-workspace-report.json`,
`blueice-font-cold-final-source-ci-final-report.json` and the hosted oracle/
coverage/required-step proofs. K.7 through K.10 remain open.

### K.7 Emit breadth — M9 — gaps G-E1 to G-E4

- [x] **K.7.1 Targets (XL).** `ES2015`–`ES2019`, `ES2021`, `ES2023`, `ESNext`, then `ES5`; helpers
  (`__awaiter`, `__generator`, `__spreadArray`, `__values`, `__read`, class/destructuring/rest lowering)
  written from the specified semantics, versioned like the class and decorator helpers, recorded in the
  manifest and fingerprint. Validate each target by running its output under a Node build or a VM
  profile that rejects newer syntax (use BlueJS's parser at the matching edition as the syntax check).
  - *Isolated failing baseline (2026-10-10):* 187 programs cover eleven targets and seventeen forms. Pinned TypeScript 5.9.3 accepts 180 and rejects seven; every accepted reference passes Node execution, exact declarations and pinned Acorn 8.15.0 edition validation. The repository recorder reproduces all observations. The shared record grows from 2,544 to 2,731 cases/219 templates, with every old observation unchanged. On unchanged `1f76ff28c` production, the public replay has two passing and four failing tests: target/config verdicts, emitted output, helper-version manifest identity and existing ES2020 logical-assignment syntax. Format, both-crate all-target Clippy and shared completeness pass. Evidence: `blueice-k71-test-baseline-second-{status,source-hashes}.json`, `blueice-k71-shared-reference-proof.json` and logs. Commit the failing baseline on the isolated branch; keep it unpushed and leave implementation pending until the M8 complete hosted CI gate closes. BlueJS currently has no edition-selecting parser API; Acorn is independent test evidence, not completion of that requirement.
  - *Independent syntax witnesses (2026-10-10):* 55 authored JavaScript examples record 605 pinned edition decisions (403 accept/202 reject), including ES5 controls for property keywords and future-looking string/comment text. All 55 parse through the current public BlueJS API; the live independent recorder, completeness, format and both-crate all-target Clippy pass. Evidence: `blueice-k71-syntax-profiles-{status,source-hashes}.json`. Older-edition rejection remains pending, and no production source is changed.
  - *Module-paired failing baseline (2026-10-10):* Both existing module kinds expand the corpus to 374 programs (360 accept/14 reject). Every accepted reference passes Node/declarations/edition parsing, and all paired observations agree; the original 187 CommonJS observations remain unchanged. The shared record now has 2,918 cases/219 templates with all previous 2,731 observations unchanged. Format, both-crate all-target Clippy and completeness pass. The expanded target replay remains two pass/four fail; all 402 primary and fourteen presentation differences in shared replay belong to targets. Evidence: `blueice-k71-esm-*-proof.json`, `blueice-k71-esm-targets-status.json` and `blueice-k71-esm-baseline-final-third-status.json`. Production is unchanged, the baseline stays isolated/unpushed, and M8 must complete before implementation.
  - *Control-flow failing baseline (2026-10-10):* Twelve more forms cover return/throw/finally, delegated and async iterator closing, rejected await, lexical receivers, getter/default ordering and tuple spread. All 638 pinned programs (600 accept/38 reject) are recorded; all 600 Node/declaration/syntax references pass. TS2496/TS2556 rejection controls are retained with independently accepted companions. The shared 3,182-case/221-template record preserves all previous 2,918 observations. An ordinary build/declaration replay covers all accepted emit paths. Format and both-crate all-target Clippy pass; target replay is two pass/five fail, and all 714 primary/38 presentation differences belong to targets. Evidence: `blueice-k71-control-flow-*-proof.json` and frozen final reports/logs. Production is unchanged and K.7.1 remains isolated/unpushed pending M8 and implementation.
  - *Edition implementation in progress (2026-10-10):* Public BlueJS `SyntaxEdition`, `parse_with_edition` and `parse_module_with_edition` select all eleven recorded versions. An iterative AST walk and consumed-token lexical checks match all 605 original observations (403 accept/202 reject); all 55 latest ASTs remain identical to the original library. Another 341 pinned context observations (264 accept/77 reject) cover nested templates, Unicode, RegExp and property-name controls. Actual native replays and live Acorn pass; Cargo format, three-crate all-target Clippy and four bridge syntax tests pass. Full BlueJS regressions pass: 4,109 tests in 335 groups, zero failures; the final bridge routing passes all five syntax tests and all four ignored BlueJS Node differential tests pass. The live context oracle is routed through the existing bridge CI job. Production stays isolated/uncommitted/unpushed; target lowering, helpers, library/iteration options and the complete K.0 gate remain open. Evidence: `blueice-k71-edition-*-report.json`, `blueice-k71-edition-cargo-first-status.json` and `blueice-k71-context-oracle-ci-routing-proof.json`.
  - *Configuration and first lowering in progress (2026-10-10):* Explicit ECMAScript `lib` selection and `downlevelIteration` are read through the API/configuration, recorded in artifacts and bound to fingerprints. All 29 pinned library verdicts match; sixteen focused tests, the six existing library oracles and eighteen CLI regressions pass. Additional owned library profiles are cumulative; existing ES2020/ES2022 declaration source bytes are retained. ES2020 binding logical assignments now preserve Node results and pass the ES2020 syntax floor. An original nullish helper has a versioned manifest identity; all 22 target/module fingerprints are distinct. A further Node comparison matches pinned TypeScript and native ES2021 for getter reads, short circuits, nested assignments, throws, helper-name collisions and `await`. BlueJS now accepts the named async export; its full ordinary suite passes 4,110 tests and all four Node oracles. Two new Acorn controls preserve all 341 original context observations and expand the record to 363 (279 accept/84 reject). Braced statement boundaries, computed object methods and explicitly annotated named async generators now pass their pinned public regressions. An original ES2015 exponentiation transform passes Node effect-order, declaration and edition checks. The latest frozen replay (`blueice-k71-async-generator-implementation-second-status.json`, 7,006 files) passes format, three-crate all-target Clippy, 241 BlueTS unit tests, all nine focused context/library tests and all 22 protocol declarations. Explicit receiver parameters also pass thirteen pinned diagnostic sources and Node/direct-runtime arity and declaration comparisons. The receiver replay (`blueice-k71-explicit-this-implementation-third-status.json`, 7,011 files) reduces target differences to 88 across four forms. Array-rest calls, numeric/string reduce contexts and rest declaration ellipses then pass eight pinned diagnostic sources and Node/direct-runtime/declaration comparisons. The latest frozen replay (`blueice-k71-rest-array-implementation-third-status.json`, 7,017 files) passes format, three-crate all-target Clippy, 241 unit tests and all 22 protocol declarations; target differences fall to 66 across binding patterns and guarded catch yields, with three failing ordinary target tests. Remaining lowering/checker work and K.0 are open. Production remains uncommitted/unpushed.
  - *Binding, flow and declaration progress (2026-10-10):* Twelve pinned catch/try/finally controls, ten variable-pattern observations (including both relocated source-name rejections), ten constant-template observations and their live recorders are covered. Flat object/array defaults and rest have retained AST records and direct BlueJS lowering; getter/default effects and exact declarations match native Node/TypeScript. Exported/ambient, re-exported and nested variable patterns remain precise refusals. All 638 target verdicts and primary diagnostics now match; all 600 accepted target declarations, the owned target declarations and the shared diagnostic matrix pass in `blueice-k71-template-constant-final-gate-status.json` (7,028 files). That ordinary replay passes 1,089 tests but exposes an obsolete destructuring syntax expectation; both original missing-source controls are moved to exact native TS2304 replays. The corrected complete ordinary/every-ignored gate is pending. Original downlevel transforms and the final K.0 closure remain open; production remains isolated/uncommitted/unpushed. Largest production source: 1,190 lines.
  - *Original downlevel progress (2026-10-10):* The corrected complete ordinary gate passes 1,090 tests; every ignored oracle finishes with 179 passes and two target/protocol syntax failures. There are no target build/runtime/declaration differences. Original optional/nullish, object spread and flat object-rest transforms pass format, three-crate all-target Clippy, native effect/declaration controls and the prior target diagnostics/declarations. Enumerable own-value copies include symbols and prototype-named data; ordinary property runs preserve descriptors. Rest defaults execute before copying excluded keys. The latest frozen 7,035-file gate passes 241 unit tests and nine target tests, with one remaining target test reporting 73 older-edition syntax differences, down from 101. Every helper source contributes to fingerprint identity. The subsequent original async, async-iteration and async-generator transforms pass their native request/effect/declaration witnesses and reduce remaining syntax differences to 49, all ES5; every ES2015-or-newer target now passes syntax checks. Indexed calls now pass all six exact pinned acceptance/diagnostic controls, 1,091 ordinary tests and the shared diagnostic/target matrix. See PLAN.md and frozen `blueice-k71-{async-generator-first,indexed-call-broad}-status.json`. Original ES5 lexical/loop/arrow, default/rest, template, flat-pattern, for-of and computed-property lowering then passes the committed scope oracle, format, Clippy and 241 unit tests. Remaining syntax differences fall to 28, with no build/runtime/declaration differences (`blueice-k71-es5-objects-first-status.json`, 7,061 frozen files). Original ES5 array/call spread and named-class/inheritance lowering then passes those gates and all target build/runtime/declaration observations; syntax differences fall to 18, exclusively generator/async/for-await (`blueice-k71-es5-classes-second-status.json`, 7,065 frozen files). Full pre-suspension verification finishes with 1,091 ordinary tests and 184 ignored oracles passing; two ignored tests retain only those ES5 syntax differences (`blueice-k71-es5-baseline-full-status.json`). Original structured generator/async continuations and loop ownership then pass format, Clippy, 241 unit tests, all ten target tests and all three protocol tests: 638 verdicts, 600 declarations/Node/Acorn observations and all 22 protocol configurations match (`blueice-k71-es5-statements-first-status.json`). A separate public-API gate confirms all 600 emitted programs parse through BlueJS at the selected edition (`blueice-k71-emitted-bluejs-first-status.json`). Unsupported named ES5 suspension graphs refuse emission explicitly. Inventory is regenerated; the final whole-leaf K.0 gate remains pending and production is uncommitted/unpushed.
  - *Verified (2026-10-10):* The final 7,071-file frozen-source K.0 gate passes format, three-crate all-target Clippy with warnings denied, all 1,093 ordinary tests and every one of the 187 ignored oracles in 122 target/doctest groups (1,280 unique tests). All 6,939 backend/Cargo file hashes match the tested snapshot. The 638 pinned verdicts, all 600 accepted Node/declaration/Acorn observations, all 600 actual outputs through the edition-selecting public BlueJS parser, every protocol configuration and the shared 3,182-case diagnostic matrix agree. Unsupported named ES5 suspension graphs produce an explicit diagnostic and no artifact. The regenerated inventory contains 197 refusal sites; G-E1 retains unmeasured combinations. The largest changed production source is 1,190 lines, below the 1,200-line review threshold. Evidence: `blueice-k71-final-k0-first-{report,status,source-hashes}.json`, exact logs and `blueice-k71-final-source-size-audit.json`. Commit and integrate this green leaf, then run complete hosted CI on that final source.
  - *Complete CI storage failure (2026-10-10):* Run [38015722230](https://github.com/ephoton0210/blueice/actions/runs/38015722230) on `411a1a41c` proves 6,902 workspace tests in 530 groups with zero failures on RHEL UBI 10 and all 187 ignored oracles in 122 groups on macOS. Its coverage runner exhausts disk space; GitHub check annotations identify `System.IO.IOException: No space left on device` in the runner log writer, so it publishes no coverage result. Disable Cargo incremental and dev/test debug information for CI to reduce build artifact storage. Keep both coverage thresholds and all existing exclusions unchanged; run the complete workflow on the pushed CI fix. This does not change K.7.1 backend/Cargo source hashes.
- [x] **K.7.2 Module systems (L).** `AMD`, `UMD`, `System`, per-file module kind for `node16`/`nodenext`
  with `.mts`/`.cts`, `verbatimModuleSyntax`, `importHelpers`, `noEmitHelpers`.
  - *Isolated failing baseline (2026-10-10):* 95 original programs/configurations cover AMD/UMD/System (3), per-file Node16/NodeNext and verbatim syntax (24), helper options with an original provider (32), and live/default/re-export/type/cycle/order contexts (36). Live pinned TypeScript 5.9.3 confirms 77 accepts and 18 rejects, exact diagnostic origins, declarations and per-file output suffixes. The public BlueTSC CLI differs in 86 decisions on unchanged K.7.1 production. The 7,398-file third snapshot passes format/all-target Clippy and both native completeness and recorder tests; only the public decision replay fails. The initial archive omitted six original dependency files; their native source hashes and root sets are restored before the native replay passes. Native execution observations remain recorded from the original probes; a repository BlueTSC emit-and-run replay is still required before implementation closure. The native-generated `module-systems-checker-matrix.tsv` and final 7,399-file gate pass format, three-crate all-target Clippy, matrix completeness and native replay; only the public decision test fails (two pass/one fail). Commit this failing replay before implementation. Keep this test preparation isolated/unpushed while the complete K.7.1 hosted CI runs on `411a1a41c`.
  - *Native execution failing supplement (2026-10-10):* Original closed AMD/UMD/System loaders, both UMD paths for the wrapper controls, real Node per-file execution and an original helper provider reproduce all 77 accepted native observations in the live recorder. The frozen 7,400-file gate passes format, three-crate all-target Clippy, completeness and live native decisions/declarations/execution. The public BlueTSC replay fails with 86 decision differences and 69 emitted differences (68 unsupported configurations and one CommonJS side-effect-import declaration difference). Eight existing CommonJS contexts match execution/declarations. Evidence: `blueice-k72-execution-baseline-first-{status,source-hashes}.json` and exact logs. All existing K.7.1 production hashes are unchanged; commit this failing supplement before implementation and keep the branch unpushed.
  - *Verified (2026-10-10):* All 106 pinned configurations (82 accept, 24 reject) match native verdicts and primary diagnostic origins; all accepted actual execution, declaration and per-file suffix comparisons pass. Original AMD/UMD/System wrappers preserve measured bindings and cycles; owner-selected Node formats and nearest-package observations revalidate without wider authority. Six no-import/export Node controls require the native TS2354 helper refusal. The immutable 7,445-file final K.0 gate passes format, three-crate all-target Clippy, 1,107 ordinary tests and all 190 ignored oracles (1,297 total; 127 target/doctest groups per test command). Evidence: `blueice-k72-implicit-first-{status,source-hashes}.json` and `blueice-k72-final-verification-proof.json`. Largest production source: CLI 1,210 lines; publish this tested leaf, then separately test and publish the FileLoader split before K.7.3. Broader module/helper/target and declaration-input combinations remain unmeasured.
  - *Verified modularity split (2026-10-10):* FileLoader moves unchanged into a 276-line module; the CLI drops from 1,210 to 945 lines. The separate complete K.0 gate passes format, three-crate all-target Clippy, all 1,107 ordinary tests and all 190 ignored oracles. All frozen source hashes match. Evidence: `blueice-k72-cli-split-first-status.json` and `blueice-k72-cli-split-final-proof.json`. Publish this split separately before K.7.3.
  - *Hosted CI fixture correction (2026-10-10):* Complete CI [38047766020](https://github.com/ephoton0210/blueice/actions/runs/38047766020) on `ec5823ed4` exposes 24 primary-origin differences in the Windows 11 VS2026 arm64 module replay. Windows Git converts the source fixtures to CRLF while their pinned native observations use LF; compiler positions correctly identify the actual CRLF input. A frozen Linux CRLF replay reproduces the same 24 case IDs, with three passes and one failure. Pin the module-system fixture tree to LF in `.gitattributes`. A forced `core.autocrlf=true` checkout retains all 354 original fixture byte sequences. Backend sources, fixture bodies, diagnostic expectations and coverage exclusions are unchanged. Evidence: `blueice-k72-windows-crlf-reproduction-proof.json`, `blueice-k72-windows-checkout-proof.json` and the retained Windows job log. A fresh complete final-source CI gate remains required.
  - *Verified fixture correction (2026-10-10):* The restored LF snapshot passes format, three-crate all-target Clippy and all six module-system tests, including the live TypeScript 5.9.3 recorder and all 82 accepted Node/declaration/output comparisons. All 106 verdicts and primary origins agree. All 7,446 snapshot files match; 7,314 backend/Cargo/workflow files match the previously complete K.7.2 CLI-split K.0 snapshot exactly. Evidence: `blueice-k72-windows-ci-green-status.json` and `blueice-k72-windows-ci-green-source-proof.json`. Publish the fixture fix and rerun the complete hosted CI on its final SHA.
- [ ] **K.7.3 Output options (M).** `removeComments`, `newLine`, `emitBOM`, `inlineSources`, `sourceRoot`,
  `mapRoot`, `downlevelIteration`, `stripInternal`, `preserveValueImports`/`importsNotUsedAsValues`.
  - *Callable/indexed alias failing supplement (2026-10-10):* Forty-eight native controls cover call, construct and index signatures, their combinations, fully internal members and empty aliases in both module modes and all four comment/internal flag combinations. All 48 native executions and public builds succeed; 40 exact public declarations differ. The full native corpus now contains 176 configurations (170 accept, six reject); every previous 128 observation is unchanged. Commit this failing replay before extending the documented-member renderer; retain the existing inferred-variable and constructor-arrow printers. Evidence: `k73-signature-member-baseline.json` and `k73-signature-members-native-proof.json`. K.7.3 remains open.
  - *Verified output-boundary supplement (2026-10-10):* The complete 120-configuration implementation passes its frozen K.0 gate: format, three-crate all-target Clippy, 1,111 ordinary tests and all 194 ignored oracles (1,305 total). The additional eight type-alias controls then reproduce exactly six declaration differences with three passing tests and one failing replay. Their corrected 128-configuration implementation passes format, Clippy, all four output tests (including the live native recorder, 122 actual executions and exact declarations) and the strict-format regression. The complete corrected K.0 gate is running; production remains uncommitted/unpushed and K.7.3 remains open. Evidence: `blueice-k73-final-k0-fourth-proof.json` and `blueice-k73-alias-members-focused-proof.json`.
  - *Type-alias member failing supplement (2026-10-10):* Eight native controls cover member documentation and `stripInternal` on exported object type aliases in both module modes and all four flag combinations. All eight native programs execute successfully; six actual public declaration comparisons differ before correction. The full recorder contains 128 configurations (122 accept, six reject), with every previous 120 observation unchanged. Commit this failing supplement separately and retain exact native declarations before implementing. Evidence: `k73-alias-member-baseline.json` and `k73-alias-members-native-proof.json`. K.7.3 remains open.
  - *Interface/enum member failing supplement (2026-10-10):* Sixteen additional native declaration boundaries record member documentation and `stripInternal` behavior in both module modes and all four comment/internal flag combinations. All sixteen native programs execute successfully; twelve public declaration comparisons fail before correction. The complete recorder now verifies 120 configurations (114 accept, six reject), all 114 actual Node executions, exact declarations and output metadata. Every previous 104 observation is unchanged. Keep this failing replay isolated/unpushed and correct these remaining output boundaries before closing K.7.3. Evidence: `k73-type-member-baseline.json`, `k73-type-member-native-proof.json` and the reproducible native corpus.
  - *Option-validation failing supplement (2026-10-10):* Eighteen additional pinned controls record eight accepts and ten rejects, real native CLI exits/file inventories, exact primary diagnostics, and accepted Node executions/declarations. The frozen public CLI differs in six cases: both `downlevelIteration` command values, three missing-map dependencies, and the invalid `newLine` diagnostic. Command `sourceMap` overrides repair otherwise invalid project combinations. Native CLI configuration-type errors can emit files despite `noEmitOnError`; these observations are retained without assuming all rejected configurations suppress output. Commit the new replay separately before its production correction. Evidence: `k73-output-validation-blue-baseline.json` and the original native recorder/reference corpus. The complete existing K.0 gate remains running; K.7.3 remains open.
  - *Isolated output implementation and member baseline (2026-10-10):* All original 78 native configurations (72 accept, six TS5102 rejects) match public CLI verdicts and primary diagnostic origins; all accepted Node executions, exact declarations, BOM/newline/comment properties and source-map metadata agree. Format and three-crate all-target Clippy pass on the frozen implementation. Sixteen additional class/namespace member controls all execute successfully under pinned TypeScript 5.9.3; their public declaration replay has four passes and twelve failures before the member correction. All original observations remain unchanged. The corpus now contains 94 configurations (88 accept, six reject). Commit the new failing controls separately and keep the implementation isolated/unpushed until the complete K.0 gate and final-source hosted CI pass. Evidence: `blueice-k73-options-implementation-fourth-{status,source-hashes}.json`, `blueice-k73-member-native-evidence.json` and `blueice-k73-member-blue-baseline.json`. K.7.3 remains open.
  - *Member and strict-format correction; map-root baseline (2026-10-10):* All 94 native configurations, Node/declaration observations and the live recorder now pass. The existing strict-runtime CLI/Node regression covers both ES2020/ES2022, both LF/CRLF and both BOM modes; all eight combinations pass without changing contract rejection behavior. Format and three-crate all-target Clippy pass. Ten additional native path/URL/empty-root controls execute successfully; the BlueTSC header/directive replay records four passes and six failures before the root correction. The corpus grows to 104 configurations (98 accept, six reject); only temporary absolute case paths are replaced by `<case>` in source-map observations. All prior observations remain unchanged. Evidence: `blueice-k73-members-implementation-second-status.json`, `k73-root-controls-native.json` and `blueice-k73-root-blue-baseline.json`. The complete K.0 and hosted CI gates remain open.
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
