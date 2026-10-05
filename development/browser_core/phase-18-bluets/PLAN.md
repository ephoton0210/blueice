# Phase 18 — BlueTS / BlueTSC: TypeScript Front End, Emitter, Type-Aware Debugging, and Runtime Contracts

[← Back to plan](../BROWSER_CORE_PLAN.md)

**Delivered-status history** (per-slice evidence and boundaries) lives in
[PLAN_HISTORY.md](PLAN_HISTORY.md); this file holds the design.

## Objective

Let a BlueIce page opt in to TypeScript source without a build-time `.js` artifact, while also providing BlueTSC for projects that need to compile TypeScript to portable JavaScript. Both paths preserve the reasons to author code in TypeScript: deterministic static diagnostics, source-level debugging, and—where data crosses a trust boundary—runtime validation of an explicit, reifiable contract.

## Standalone implementation and first BlueJS integration status

The direct bridge lowers parenthesized `new Identifier(args)` expressions with normal or spread arguments. Member constructors and omitted parentheses remain excluded.

The direct bridge lowers `delete` only for ordinary dot or bracket property references (without optional chaining). Identifier and non-reference operands remain outside the v1 subset.

The direct bridge lowers relational `in` and `instanceof` expressions. BlueTSC gives both a `boolean` result in the bounded checker; narrowing and custom-instance analysis remain future work.

Direct calls may use identifier, dot-member, or bracket-member callees with normal or spread arguments, preserving the BlueJS receiver. Optional calls remain excluded.

The current direct bridge subset supersedes earlier phase summaries: literals/identifiers, bounded quoted-string and template escapes, template slots containing supported direct expressions, direct calls/constructors with normal or spread arguments, supported unary/binary/logical/conditional/assignment/sequence expressions including relational `in`/`instanceof`, arrays with holes or spread elements (but not both in one literal), identifier/string/numeric/computed object keys with identifier shorthand and spread properties, dot/bracket reads, property assignments/updates/deletion. Nested templates, optional chaining, object methods, and accessors remain outside the subset.

The direct bridge accepts identifier, quoted-string, numeric, and computed object keys plus spread properties. A computed key lowers its supported direct expression to BlueJS `PropertyKey::Computed`; shorthand remains identifier-only, while methods and accessors remain excluded. BlueTSC merges an identifier-bound known record spread source into an inferred literal; computed keys, unknown/non-record spread sources, and general spread expressions remain `unknown` under the bounded static rule.

The direct bridge preserves non-spread array holes as BlueJS AST holes, rather than materializing `undefined` properties. It also lowers normal and spread array elements, but deliberately rejects any one literal that combines a hole and a spread element because BlueJS's spread construction path would otherwise materialize that hole. BlueTSC infers a known `T[]` spread source as `T` for a surrounding array literal; unknown/non-array sources retain the bounded checker's `unknown` element type.

The direct bridge lowers normal and spread call/constructor arguments. For calls to a known local function, BlueTSC expands only a tuple-typed spread into fixed parameter types; ordinary arrays, unknown values, and general iterable analysis remain outside that static rule.

Named local functions may use optional identifier parameters and a final identifier rest parameter. An optional parameter without an initializer lowers to an ordinary JavaScript parameter; within the function body BlueTSC treats its value as `T | undefined`. The direct bridge uses BlueJS's rest-parameter AST form; with an explicit `T[]` annotation, BlueTSC checks every normal or tuple-spread tail argument against `T` and infers generic `T` from the same tail. General array/iterable spread analysis remains outside the direct rule.

Named local functions may use a default parameter whose initializer is in the direct expression subset. BlueTSC retains the original initializer tokens, verifies a typed initializer against the parameter type, and the bridge lowers it directly to BlueJS `Param::default`; default expressions outside the direct subset remain excluded.

Named local function bodies retain ordered initialized local declarations, semicolon-terminated direct-expression statements, and `return` statements. The bridge lowers each structured expression statement to BlueJS in source order, so assignments and direct calls may have their normal local side effects. BlueTSC applies its existing bounded direct-call, property-access, and arithmetic checks to those statements; control flow, exception handlers, and every other unstructured body statement remain opaque and are rejected by the direct bridge.

Named local function bodies may also use `throw expression;`. BlueTSC retains and checks the direct expression, rejects an omitted value or a line terminator immediately after `throw`, and the bridge lowers it to BlueJS `Stmt::Throw` so the VM preserves the thrown JavaScript value. `try`/`catch`/`finally` and all other exception control flow remain outside the v1 direct subset.

Named local function bodies may use a direct-expression `if` condition with braced consequent, braced `else if` branches, and an optional braced `else` body. BlueTSC retains each branch structurally and the bridge emits BlueJS `Stmt::If` nodes with explicit blocks for braced bodies, while lowering an `else if` as a direct alternate rather than inventing an artificial block scope. The checker applies its existing bounded direct-expression checks to every condition and recursively to the branch bodies; it does not claim control-flow narrowing. At this earlier if-only step, unbraced branches, an `else if` with an unbraced branch, loops, and every other control-flow form remained opaque and failed closed at the direct bridge.

For this structured function subset, an explicit return annotation that does not admit `undefined` must terminate every known path with a value `return` or `throw`. A bare `return;` is checked as `undefined`; `void`, `any`, `unknown`, and an annotation admitting `undefined` may fall through. Opaque control flow never supplies a termination proof and remains independently fail-closed at the direct bridge.

### H.1.1 First loop form after release gate

Choose `while (condition) { body }` inside an already structured named local function body, including an existing braced `if` branch. BlueJS already has a structured `Stmt::While` and bounded execution fuel; this form has no `for` head declaration, iterator protocol, or per-iteration binding to invent. The condition must be one supported direct expression and uses ordinary JavaScript truthiness on every iteration. The braced body may contain existing direct expression statements, `return`, `throw`, and braced `if` statements with the same body restriction. Assignments to bindings declared outside the loop are allowed. The direct bridge will lower the typed loop to a BlueJS `Stmt::While` with a real `Stmt::Block` body, without reparsing emitted JavaScript. BlueTSC will check the condition and each recognized body expression with its existing bounded rules and retain original TypeScript source spans; it will not infer control-flow narrowing from the condition.

The loop may execute zero times, so it never proves a required function return, even if the condition is statically `true` or its body returns. An annotated function that requires a value must still have a return or throw path established independently of the loop under the current conservative return-path rule. Every iteration remains subject to the existing VM fuel and host deadline. H.1.2 must show zero and multiple iterations, loop-back safe-point visits that keep the same exact original-source mapping, pause/resume behavior, and a bounded endless-loop failure through the direct page path; it must compare accepted syntax and diagnostics with the pinned TypeScript oracle and deterministic runtime behavior with the direct VM.

This first form excludes unbraced `while`, a second `while` nested inside a loop, body-local `var`/`let`/`const` declarations, `break`, `continue`, labels, `do...while`, classic `for`, `for...in`, `for...of`, and `for await...of`. Top-level and anonymous/arrow-function loops remain outside the current direct bridge. Excluded shapes stay opaque to direct lowering and must fail closed there; the standalone emitter's existing token preservation is not a claim that the direct page path supports them. These restrictions defer loop scope and abrupt-completion semantics until their checker, direct VM, debugger, and oracle evidence can be added together.

H.1.2.1 retains a complete braced `while` as a structured named-function body item, preserving its condition tokens, nested body items, and original statement span. The bounded checker visits the condition and body with its existing direct-expression rules and treats the loop as a possible fall-through path regardless of its body returns. At this stage the direct bridge explicitly rejects the structured item at its source span; H.1.2.2 must install runtime lowering and enforce the selected body subset before a loop can execute. Public BlueTSC and direct-bridge regressions cover the accepted shape, condition/body call diagnostics, return fall-through, and the temporary execution denial.

H.1.2.2 lowers that typed item directly into BlueJS `Stmt::While` with an explicit `Stmt::Block` body. Before lowering a loop, the bridge recursively checks every braced `if` branch in its body and rejects loop-local declarations, nested loops, and opaque control-flow tokens with original source spans. The parser keeps unbraced loops and other loop forms outside this structured item. Direct page-realm tests show numeric truthiness, zero and multiple iterations, a loop inside an existing `if` branch, return and throw completion, and a deliberately endless loop stopped by a small VM instruction budget. Excluded declarations, nested/unbraced loops, break/continue, labels, do-while, and classic for remain denied. H.1.2.3 still owns repeated loop-back safe-point and pause/resume evidence; this runtime result alone does not close debugger mapping.

H.1.2.3 exercises the installed direct BlueTS program through the public BlueJS page-runtime nested debugger. Every paused child instruction across three loop iterations resolves through the generation-validated safe-point map to the exact original BlueTS function-declaration byte span; at least one loop-back offset is visited more than once with that same mapping. The first invocation is stepped until its child returns, then the root resumes; a second independent frame resumes directly. Old frames and map validation fail after navigation invalidates the program generation. The v1 child map's granularity remains the whole original function declaration, as specified by the existing direct-debug contract; this check does not claim a separate loop-condition or body-line source span.

H.1.2.4 adds three pinned TypeScript 5.9.3 oracle fixtures for braced `while`: zero/multiple-iteration output, identical condition/body argument-mismatch diagnostic lines, and the missing-return line for a loop that may execute zero times. The full oracle matrix passed locally using an already cached 5.9.3 installation, without another Node dependency tree. The first local workspace test run reached the 60 GiB shared-target limit and was stopped by the disk guard while the host still had about 579 GiB free. Its large debug-bearing Cargo artifacts were removed with `cargo clean`; the guard now defaults local dev/test debug info to zero and suppresses transient `du` warnings for compiler files removed during its walk while still requiring a numeric size. A fresh single-target run then passed `cargo test --workspace`, workspace all-target Clippy with warnings denied, all-target build, and rustfmt; the final target was about 12 GiB with about 629 GiB host space available. This is a local gate result; CI retains its own ordinary profile, coverage thresholds, and pinned oracle job.

### H.2.1 First try/catch/finally form

Choose a braced `try { body } catch (error) { body }`, a braced `try { body } finally { body }`, or their combined form inside a structured named local function. At least one handler or finalizer is required. The first catch binding is exactly one unannotated identifier, bound as TypeScript `unknown` and visible only inside the catch body; it shadows an outer name there without leaking into the try or finally body. BlueJS already represents this with `Stmt::Try` and a `CatchClause` whose parameter is an identifier pattern. Every try, catch, and finally body may use existing direct expression statements, `return`, `throw`, and braced `if` branches using that same subset; assignments to bindings from the enclosing function are allowed. The direct bridge must construct the BlueJS AST from retained BlueTS tokens and source spans, without reparsing emitted JavaScript.

For ordinary JavaScript completion, catch handles a thrown value from the try body and binds that exact value; it does not handle a throw from its own body. Finally runs once after normal, return, or throw completion of the try/catch path. A normal finalizer preserves the pending completion; a finalizer `return` or `throw` replaces it. Fatal VM resource limits and host failures remain terminal and are not promised catch/finally recovery. The checker validates recognized expressions and returns in each body but grants no control-flow narrowing or required-return proof from the try construct in this first form; a non-void annotated function still needs a separately established trailing return or throw path. H.2.2 must verify the catch binding's scope and unknown type, the completion precedence, direct page execution, source-bound safe points, and stale-generation denial.

This form excludes bare `catch {}`, typed or destructured catch bindings, a second catch, nested try, loop statements or local declarations inside any of the three blocks, and top-level or anonymous/arrow-function try statements. Excluded shapes remain opaque to the direct bridge and fail closed. Their scope, abrupt-completion, and debugger effects need their own checker and runtime evidence before extension.

H.2.2.1 preflights only a complete braced try followed by a single identifier catch, a braced finally, or both. BlueTSC retains separate structured try/catch/finally body items and their original source spans; bare or typed catch bindings and a try without handler/finalizer stay opaque. The direct bridge explicitly rejects even a structured try at its original span until H.2.2.2 checks catch scope and H.2.2.3 installs lowering. Public parser and direct-bridge regressions cover these boundaries; this intermediate parser representation alone does not claim executable try support.

H.2.2.2 walks structured expression and return bodies with the lexical scope in which each item runs. The catch body shadows only its own binding with `unknown`; try, finally, and subsequent function statements retain the outer binding. Catch-origin `unknown` is strictly assignable only to `unknown`, `any`, or a bounded alias/union/intersection that accepts it. This focused rule preserves the checker's existing permissive `Unknown` fallback for expressions it cannot yet infer elsewhere. Return expressions are checked even without an explicit return annotation; required-return analysis remains conservative and grants no terminating path from try. Frontend regressions cover shadowed calls and returns, nested `if`, no-annotation return calls, accepted unknown destinations, and the separate trailing-return requirement. The direct bridge remains closed until H.2.2.3.

H.2.2.3 lowers a validated structured try directly to BlueJS `Stmt::Try`, with an identifier-pattern `CatchClause` and optional finalizer. It validates all three blocks and nested braced `if` branches before constructing the AST, rejecting block-local declarations, loops, nested try, and opaque syntax at their original source spans. Page-realm regressions show exact thrown-value binding, catch shadowing with outer scope restored in finally, normal/return/throw completion, finalizer return/throw precedence, catch rethrow escape, and terminal instruction-budget failure. Direct compiler regressions cover the selected AST and excluded syntax; no emitted JavaScript is reparsed. Nested source-bound safe points, stale-generation denial, the pinned TypeScript oracle, and workspace gates remain H.2.2.4.

H.2.2.4 verifies a live page-realm nested frame through the throwing try, catching branch, and finalizer. Every paused nested instruction resolves through the checked safe-point map to the original named-function declaration span; this is the current v1 child-map granularity, not a distinct try/catch/finally line span. Navigation invalidates that map, frame, and static debugger retention; attaching the same artifact again mints a separate valid generation. Two pinned TypeScript 5.9.3 oracle fixtures compare accepted try/catch/finally syntax and `7:5` emitted output plus the catch-`unknown` call-error line. The full oracle matrix, workspace tests, all-target build, all-target Clippy with warnings denied, rustfmt, and whitespace checks pass. The complete workspace test needed local Unix-socket permission for the `ai-gatekeeper` tests; its first sandboxed attempt failed there, then the same shared-target disk-budget run passed with socket access. The target remains about 13 GiB with about 629 GiB free on the host.

### H.3.1 First bounded narrowing and return-path form

Choose one immutable function-local `const value: string | number = expression` and a braced `if (typeof value === "string") { ... }` or `if (typeof value !== "string") { ... }`, with an optional braced `else`. The `typeof` operand must be that one local identifier; the compared literal may use either quote style. The two primitive union arms are exhaustive: the equality branch sees `string` and the other branch sees `number`, with roles reversed for `!==`. A recognized guard may appear in a named local function using the existing direct-expression, return, throw, and braced-if body subset. The checker uses branch-local type scopes for calls and returns; it never rewrites the runtime value. BlueTSC emission erases only TypeScript annotations, and the direct bridge continues to lower the existing BlueJS `Stmt::If`/`Stmt::Block` AST without reparsing JavaScript.

When a guarded branch terminates with `return` or `throw`, the surviving branch's type becomes the local's type at the next sequential statement. An absent `else` counts as a surviving false branch. If both branches can continue, the post-if type remains the declared `string | number`; if both terminate, the existing structural return-path proof applies. A single guarded branch is not sufficient to prove that a non-void function returns, since both union arms are reachable. Existing opaque syntax remains unable to prove termination and the direct bridge continues to reject it. H.3.2 must test true/false branch calls and returns, an early-return guard and its residual type, both-arm completion, emitted JavaScript, direct page execution, and the pinned TypeScript oracle.

This first form makes no narrowing claim for mutable `let`/`var` bindings, parameters, property reads, aliases or wider unions, equality against other `typeof` tags, compound conditions, nested or repeated guards, or guards inside a loop or try block. Those expressions may still use the previously supported bounded runtime subset, but a type obligation that depends on unimplemented narrowing must remain unproven. Assignments to the selected `const` are outside this form. The checker must not let a branch's narrowed type leak to its sibling or to a path where both branches continue.

H.3.2.1 moves structured function checks out of the near-1,300-line binding file into a focused submodule, with the first guard recognizer in its own child module. The checker recognizes exactly one top-level immutable local annotated `string | number` after its declaration and partitions it for an exact `typeof` comparison against `"string"`. The expression and return walks use the same branch scopes; a terminating branch passes the surviving scope to later statements, while two continuing branches restore the union. Nested/repeated guards, mutable locals, and parameters gain no new narrowing. Public compiler regressions cover both comparison polarities, calls and returns in both arms, early return/throw residual types, non-leakage, excluded bindings, and conservative required-return rejection. The BlueTS crate tests and all-target Clippy pass under the shared-target disk guard. H.3.2.2 and H.3.2.3 still own runtime/emission and oracle/workspace evidence.

H.3.2.2 confirms that BlueTSC erases the local and function annotations while preserving the original `typeof` condition in emitted JavaScript. The direct bridge reuses its existing `Stmt::If` lowering: public page-realm tests execute both `===` branches, inverted `!==` with a residual return, and a guard whose string arm throws its original value. Mutable locals or parameters whose calls still require unproven narrowing are rejected by BlueTS before a direct program can execute. The BlueTS and bridge crate tests and their all-target Clippy gates pass under the disk guard. The pinned oracle, debugger generation checks, and workspace gates remain H.3.2.3.

H.3.2.3 adds two pinned TypeScript 5.9.3 fixtures: one compares emitted output for both equality and inequality guards with early returns, and the other compares the two rejected branch-call diagnostic lines. A live direct-page nested debugger pause resolves to the original guarded function declaration; this is still the v1 whole-function child-span granularity. Navigation invalidates that safe-point map, paused frame, and retained static debugger entry. The full pinned oracle matrix, workspace tests with local Unix-socket access, all-target workspace build and Clippy with warnings denied, rustfmt, and whitespace checks pass under the same shared-target disk guard. The target remains about 13 GiB with about 628 GiB free on the host.

### H.4.1 First callback-bearing method overload form

Choose exactly two same-named method signatures in one interface or record type, each with two required parameters and a `void` result. The first parameter is a distinct string literal tag, such as `"text"` and `"count"`; the second is a non-generic callback function accepting one annotated primitive value and returning `void`. A call supplies the tag and one named function callback. The two signatures are static-only and must be preserved together rather than collapsed to the first field. The first exact literal argument selects one signature, after which its callback parameter is checked by the existing bounded function assignability rule. The method's runtime is one ordinary JavaScript function property supplied by the page; BlueTSC erases the interface and the direct bridge lowers the existing object/member-call expressions without reparsing emitted JavaScript.

A first argument typed as the union of the two literal tags cannot select one signature, even when a callback could accept both payloads. Emit one stable `BTS3003` ambiguity diagnostic at the call span, identifying the method and first-argument type; pinned TypeScript 5.9.3 must report a rejected overload call on the same source line. A tag outside both signatures produces a distinct no-matching-overload diagnostic. A selected tag with a wrong callback produces the existing argument-type mismatch. Duplicate or overlapping tags, more than two signatures, generic/optional/rest method parameters, anonymous callbacks, method inheritance or intersection receivers, and optional member calls remain outside this first overload form and must not silently pick declaration order as a winner.

H.4.2 must test parser retention of both signatures, structural assignment of one runtime implementation to the interface, accepted and rejected callback selection, exact ambiguity and no-match diagnostics, erased emission, direct page execution of both tags, debugger provenance/stale-generation denial, and accepted/rejected pinned TypeScript oracle fixtures. The test runtime can use a typed object literal whose `visit` property names one ordinary implementation function; no new host capability or generated-JavaScript parse path is needed.

H.4.2.1 confirms that both same-named method signatures survive the public interface and record-alias parsers. Bounded record property lookup returns their function types as an intersection in declaration order, consuming expansion fuel for each duplicate beyond the first. A single property keeps its previous lookup cost and type. Until the H.4.2.2 selector recognizes the exact supported pair, a duplicate-name member call fails closed instead of using its first signature; public compiler regressions cover overlapping tags and three signatures. BlueTS crate tests and all-target Clippy pass under the shared-target disk guard, with the existing target near 13 GiB and about 628 GiB free on the host.

H.4.2.2 adds one selector shared by member-call checking and expression-result inference. It accepts only two required `void` methods with distinct unescaped string literal tags and one required named primitive-to-`void` callback parameter each. Exact tag values select a branch even when the call and declaration use different quote styles; the selected callback is checked with existing bounded function assignability. A two-tag union emits one `BTS3003` ambiguity at the exact call span, an unrelated tag emits a separate no-match diagnostic, and a wrong callback emits an argument mismatch. Opaque callbacks fail closed. Property lookup now also retains all matching properties through an intersection receiver, and the method selector refuses inherited/intersection receivers; overlapping tags, larger sets, optional parameters and wrong callback results produce an explicit unsupported-set diagnostic instead of choosing the first signature. Public compiler and checker regressions, the BlueTS crate suite, all-target Clippy, rustfmt and whitespace checks pass under the shared-target disk guard. H.4.2.3 owns erased emission and direct runtime evidence.

H.4.2.3 uses one ordinary page function `dispatch(kind: string, listener: any): void` as the `visit` property of a `Visitor` object. The two `Visitor` signatures type-check that one implementation, and named callbacks for the `"text"` and `"count"` tags update a shared number to `3`. A public BlueTSC regression confirms that the interface and annotations disappear while the function, property and both calls remain in emitted JavaScript. A direct-page regression compiles the original checked BlueTS syntax to BlueJS AST and bytecode, attaches it to a page realm, and observes the final value without parsing emitted JavaScript. The BlueTS and direct bridge crate suites, all-target Clippy, rustfmt and whitespace checks pass using the same shared target and disk guard. H.4.2.4 owns the pinned oracle, debugger generation checks and full workspace gates.

H.4.2.4 adds pinned TypeScript 5.9.3 oracle fixtures for the accepted object method and three rejected calls: a two-tag union, unrelated tag and wrong callback. Both compilers' accepted output prints `3`, while each rejected call yields one BlueTS `BTS3003` on the same source line as a TypeScript overload error. A direct-page debugger pause in `dispatch` maps to its original BlueTS declaration; navigation invalidates its safe-point map, paused frame and static debug record. The full pinned oracle matrix and workspace tests pass, as do all-target workspace build and Clippy with warnings denied, rustfmt and whitespace checks. All Cargo work used the same disk-budgeted target, still about 13 GiB with about 628 GiB available on the host. H.4 is complete; I.1.1 selects the next bounded expression form.

### I.1.1 First optional dot-property read

Select one `local?.field` expression, where `local` is a module-local immutable `const` binding annotated as exactly a non-generic record with one required primitive field plus `null` or `undefined`. A local non-generic interface with that one field may name the record arm. The property name is an identifier and the read is one suffix, not a chain or call. On the non-nullish branch the result is the field's `string`, `number` or `boolean` type; on the nullish branch the result is `undefined`. The existing `??` expression form may consume that result. BlueTSC retains the original `?.` while erasing annotations, and the direct bridge lowers the original tokens to BlueJS `Expr::OptionalMember` so its established short-circuiting bytecode runs in the page realm. This is one optional-property read, not a general optional-chain implementation.

I.2 must prove parser tokens and span, checker type and missing-property diagnostics, emitted JavaScript, and direct runtime for both receiver states without parsing emitted JavaScript. I.3 must verify original-source provenance, stale debugger generation denial, contract behavior and a pinned TypeScript oracle. The unsupported boundary includes optional calls and methods, computed or nested optional chains, assignment targets, function-local or side-effecting receivers, optional fields, larger/opaque unions, generic or imported aliases, and inherited or intersection records. Those forms must not be treated as checked instances of this first form; the direct bridge remains free to reject them as unsupported runtime targets. The checker must not use its permissive `unknown` fallback to satisfy a typed obligation from an unsupported optional read.

I.2.1 confirms at the public parser boundary that `receiver?.value` remains the exact identifier, `?.`, identifier token sequence, with the original byte span on the optional dot. Computed `receiver?.[key]` and call-suffix `receiver?.value()` remain distinct token sequences; they are not silently rewritten into the selected form. This leaf does not yet claim checker or direct runtime support. The BlueTS crate suite, all-target Clippy, rustfmt and whitespace checks pass with the shared disk-budgeted target.

I.2.2 adds a focused checker for a top-level optional read from one preceding module-local annotated `const`. The annotation must have exactly one required primitive record field and one `null` or `undefined` arm; a non-generic local interface may name the record. The inferred type is `field | undefined`, so a bare assignment to `field` rejects and the existing `??` rule can recover a definite primitive. A missing field gets a direct property diagnostic. Computed or call suffixes, side-effecting or mutable receivers, optional fields and function-local reads fail closed instead of falling back to permissive `unknown`; the right side of `??` still receives its normal checks. Public compiler regressions, the BlueTS crate suite, all-target Clippy, rustfmt and whitespace checks pass under the shared-target disk guard. I.2.3 owns emitter evidence.

I.2.3 verifies at the public BlueTSC boundary that the selected optional read keeps its original `receiver?.value ?? 0` runtime syntax after the local interface and all type annotations are erased. The BlueTS crate suite, all-target Clippy, rustfmt and whitespace checks pass under the shared disk-budgeted target. I.2.4 must lower and run both receiver states directly.

I.2.4 adds one direct bridge suffix case for an identifier receiver followed by `?.` and an identifier property, producing BlueJS `Expr::OptionalMember` with a non-computed key. A structural AST regression proves the checked TypeScript tokens go directly to BlueJS without parsing emitted JavaScript. Page-realm regressions execute a known object and both `null` and `undefined` receiver values; short-circuiting plus existing `??` yields `41`. The broader template-substitution path remains outside this form and explicitly rejects optional access at the original substitution token span. BlueTS and bridge crate suites, all-target Clippy, rustfmt and whitespace checks pass with the shared disk-budgeted target. I.3.1 owns live/stale provenance and debugger evidence.

I.3.1 verifies that the optional read's top-level declaration maps to its exact original BlueTS byte range and a bound root safe point. A live page debugger pause uses that safe point; after navigation the predecessor's map and static record fail validation and its root continuation cannot resume. The bridge crate tests, all-target Clippy, rustfmt and whitespace checks pass under the shared-target disk guard. I.3.2 owns contract behavior, the pinned TypeScript oracle and final workspace gates.

I.3.2 confirms that a direct-page optional read over a locally constructed record has one source and no static contract entry or newly installed host callback. If the same `record | null` type is explicitly used at a data boundary, its pure `ContractPlan` accepts the record and `null` while rejecting a missing field or wrong primitive. The accepted pinned TypeScript 5.9.3 oracle fixture prints `41` from both TypeScript and BlueTSC output; rejected fixtures report `number | undefined` assigned to `number` and a missing property on the same source lines in both compilers. The complete pinned oracle matrix, workspace tests including Unix-socket integrations, all-target workspace build and Clippy with warnings denied, rustfmt and whitespace checks pass. All Cargo work reused one disk-budgeted target at about 13 GiB, leaving about 628 GiB free on the host. H and I are complete. J applies only after a user proposes a large feature; no such proposal is pending.

The direct bridge lowers template substitutions containing supported direct expressions to BlueJS expression slots. It tokenizes the original substitution with BlueTS's lexer and constructs BlueJS AST directly; it never calls a BlueJS source parser on BlueTSC output. Nested templates and embedded expressions outside the direct subset remain excluded.

Non-substituted template literals use the same ordinary escape decoder as quoted strings.

The direct bridge decodes ordinary quoted-string escapes (`\\`, quote, `\n`, `\r`, `\t`, `\b`, `\f`, `\v`, and `\0`) to BlueJS strings. Unicode, hexadecimal, legacy octal, and line-continuation escape forms remain deliberately unsupported in this bounded subset.

Record inference and direct lowering accept identifier-keyed object shorthand `{ key }` for a local binding. Direct lowering also accepts computed keys; because their names need not be statically known, the bounded record inference result is `unknown`. Object methods and accessors remain excluded.

The direct bridge lowers simple and compound assignments plus prefix/postfix updates to ordinary dot or bracket property references. The existing template/object/member-call boundaries remain in force.

The initial implementation completes the work that has no BlueJS dependency before page-runtime coupling. The first direct bridge remains host-neutral and deliberately narrower than BlueTSC's emitted-JavaScript matrix:

- `blueice-bluets` accepts only caller-supplied `ModuleLoader` records. The library itself does not read files, URLs, DOM state or host capabilities. `bluetsc` is the separate, project-root-confined filesystem adapter.
- Its pinned `blue-ts-0.1` matrix parses/binds typed variable and function declarations, named `export default function`, local-value `export default name`, and local named value-export declarations, interfaces, aliases, type-only imports/exports, primitive and literal types, records, arrays, tuples, unions, intersections and the corresponding erasable annotations/assertions. It resolves a closed relative module graph, produces stable diagnostics for parse/unsupported syntax, resolution, duplicate names, unknown types and the implemented assignment / return checks, and never emits on an error.
- `bluetsc check` uses that shared pipeline without writes. `bluetsc build` stages ESM `.js`, optional column-provenance source maps and public `.d.ts` files, then replaces the selected output directory only after every artifact has been staged. Its fingerprint includes source content, the pinned language version, target, source-map/declaration modes, runtime-policy label and resolver identity.
- `bluetsc` accepts either one explicit entry or a `bluetsc.json` project file. The latter supports multiple `entries`, project-root-confined `outDir`, `sourceMap`, `declaration`, `target`, `runtimePolicy`, and exact/prefix `imports` mappings. Config flags cannot be mixed with client-side overrides; an import target, entry or output path that escapes the declared root is rejected before compilation, including a syntactically local `outDir` whose existing ancestor is a symlink outside that root. An atomic output directory also cannot contain an input source module, so it cannot replace `src/`. This is the standalone compiler's closed-world resolver, not a page/network loader or an arbitrary package-manager hook.
- A project-root-confined relative import or exact `imports` mapping may target a local `.d.ts` declaration module. Declaration modules are parsed, checked, hashed and retained in VM-independent debug metadata, but are type-only: they never emit JavaScript or a runtime import-map target. BlueTSC rejects a `.d.ts` entry, a value import resolving to one, or runtime content within one. When declaration output is selected, BlueTSC preserves the authorized `.d.ts` source root-relatively and retains consuming `import type` clauses. Separately, a direct page host may supply an exact, already-verified generated `lib.blueice.d.ts` through `CompilerOptions::ambient_declaration_modules`; it is subject to the same source/module limits and build fingerprint, cannot import or re-export, and exposes static ambient declarations only. Direct-page admission also enables the fingerprinted `require_declared_global_calls` policy, which makes a top-level direct call fail with `UnknownName` unless it resolves to a local or verified ambient function. The standalone CLI cannot configure either host-only facility, and BlueTS never acquires ambient, remote, package-manager, or arbitrary `lib.dom.d.ts` declarations on its own.
- Every successful build stages a root-relative `bluetsc.manifest.json` with language version, project fingerprint, target, runtime policy, requested output modes and emitted entries. A configured `imports` map also emits `bluetsc.importmap.json`, translating source `.ts`/`.tsx` mappings to their generated `.js` locations. Both files are published with the artifacts, so they contain no absolute host path and cannot drift from an atomic build.
- The filesystem adapter supplies root-relative source identities to the shared compiler. Therefore artifacts, source maps and the VM-independent `BlueTsDebugInfo` do not expose checkout paths, and relocating an unchanged project does not change its resolver or source-identity contribution to the build fingerprint.
- The standalone compiler emits the VM-independent portion of `BlueTsDebugInfo`: compiler-minted source-content-hash identities, static types, symbols and spans, plus exact reifiable plans for successful non-generic local declarations only. It also contains a bounded, pure contract IR/validator for data-only JSON-like values. Imported, generic, erased or otherwise unreifiable declarations have no static contract handle; neither artifact claims a runtime type tag or validates a live page boundary.
- BlueTSC source maps use Source Map v3 segments at copied, rewritten and erased-source boundaries rather than line-only placeholders. Generated and original columns use UTF-16 code units; CRLF is represented as one source line transition. This gives portable JavaScript builds column-level TypeScript provenance without claiming a BlueJS bytecode safe-point map.
- `IncrementalCompiler` is a reusable, host-neutral single-entry session for development hosts. It reloads the caller-authorized graph to detect changed source or resolution edges, reuses parsed modules with identical bytes, and rebinds/rechecks only changed modules plus their reverse dependencies. It keeps only a successful cache entry and refuses reuse when the entry or any compiler option differs; its work-selection result is observable without exposing a BlueJS VM or page state.
- Generic aliases, interfaces and direct function calls retain their declarations' type parameters, including through local type-only imports. An interface may extend one or more named interfaces, including a generic instantiation; inherited fields participate in bounded structural checking, property lookup, declaration emission and reifiable contract conjunction. Its exported type-only surface carries inherited local declaration fields so an authorized consumer need not import private parent declarations. Direct local calls may either infer or explicitly supply their type arguments. They instantiate bounded structural checks, enforce `extends` constraints, and resolve trailing default type arguments (including in declaration modules). A declaration's type parameter never leaks into surrounding module scope. The checker additionally infers array literal element types, boolean comparisons, boolean-only `&&`/`||` chains, nullish coalescing after removing left-side `null`/`undefined`, unary `typeof`/`void`/boolean/numeric expressions, conditional branch joins, numeric `+`/`-`/`*`/`/`/`%`/`**` expressions, `<<`/`>>`/`>>>`/`&`/`^`/`|` expressions and known-string concatenation. It rejects numeric, exponentiation or bitwise/shift operators whose two operands are known incompatible primitives; it reports an unparenthesized unary exponent base as an ECMAScript early error; and it rejects a strict equality operator whose known `number`, `string` or `boolean` operands are disjoint. Unknown, `any`, union and structural operand rules remain outside this narrow initial rule; method/callback overload resolution and all other general expression inference remain pending.
- The parser rejects a `.tsx` module at its source-identity boundary, even if it has not yet reached a JSX tag. This prevents a TSX project from being treated as ordinary erasable TypeScript; tagged JSX is rejected by the same stable `UnsupportedSyntax` path. It also reports the ECMAScript early errors for an unparenthesized `??` mixed with `&&` or `||`, and a unary expression used directly as an exponentiation base, including in runtime spans that the bounded front end otherwise preserves for emission.
- Legacy CommonJS-oriented `import =` and `export =` forms are likewise rejected as `UnsupportedSyntax`, rather than being emitted as invalid ESM.
- For a direct call to a locally declared function, the checker verifies the accepted argument count and each annotated parameter after bounded generic substitution, whether type arguments are inferred or explicitly supplied. The same rule applies to semicolon-terminated direct-expression statements in a function body. Optional and default-initialized parameters are omittable while preserving a known return type. In a function body, a bare optional parameter has type `T | undefined`, while a default-initialized parameter has type `T`. Signature-only local overload declarations are resolved in declaration order for direct calls and erased from JavaScript; a non-declaration signature must have a compatible local implementation. This intentionally does not claim method calls, callback analysis, constructors, or general expression inference.
- Direct property access on an inferred record or a local/interface type alias is resolved to the declared field type (including a generic instantiation). Optional fields produce `T | undefined`; chained/member-call analysis and arbitrary JavaScript property semantics remain outside this static subset.
- The standalone `ContractPlan` validator accepts per-boundary `ValidationLimits` for depth, collection entries, visited-node fuel and string bytes. Core now applies it only to its two installed immutable host-to-script snapshots (`blueiceDocumentText` and `blueiceDocumentOrigin`) before realm callback capture; their inventory names stable binding IDs, contract IDs, direction, capability and fixed budgets. JSON/Fetch/XHR, URL/query, storage, messaging, foreign-module, extension, DOM-object, provenance, strict-runtime coverage and allocation-attribution enforcement remain deliberately separate work.
- `backend/bluets/tests/typescript_oracle.rs` is the BlueTSC compatibility-oracle job run on every push and pull request. Its test-only `BLUEICE_BLUETSC_ORACLE` environment variable names the pinned TypeScript 5.9.3 `tsc` executable; the all-caps spelling is environment-variable convention, while BlueTSC is the BlueIce compiler name. The job verifies that pin before executing, then runs a fixture matrix covering generic properties, constraints/defaults, explicit direct-call type arguments and generic interface heritage (including local `.d.ts` parents and rejected conflicting inherited fields), optional record fields and optional/default/explicit-`undefined` parameters, local constrained generic function overloads, ordered direct-expression function-body statements and rejected calls within them, function throws and invalid bare throws, braced function `if`/`else if`/`else` bodies and rejected direct calls in their conditions, ordinary and hole-bearing array literals plus array/object literals with spread elements/properties and computed object keys, direct-expression default and array-typed rest parameters plus direct calls/constructors with spread arguments, simple object literal property reads, templates with supported expression substitutions, generic arithmetic, exponentiation, numeric bitwise/shift, compound assignments, identifier/property updates and comma sequences, boolean comparisons and relational membership (`in`/`instanceof`), boolean logical chains, `??`, `typeof`/`void` expressions and conditional branch joins, named default-function, local-value default, and local named-value ESM exports, rejected assignment/call arguments including incompatible known primitive arithmetic, exponentiation or bitwise operands, disjoint strict primitive comparisons, `typeof` annotation mismatch, `??` annotation mismatch, unparenthesized `??`/logical mixing and unparenthesized unary exponent bases, Source Map v3 shape and accepted Node output. It uses ES2022 modules and a temporary `type: module` package boundary, so both BlueTSC and external `tsc` ESM artifacts execute under Node; the local default and named-value fixtures additionally compare each compiler's exact public `.d.ts` output. Rejected fixtures assert the exact BlueTS diagnostic count, stable code and source line, then require the pinned compiler to report the same count and source lines. Node and the external `tsc` are test tools only; neither is linked, spawned, or discovered by BlueTSC or BlueTS.
- `CompilerLimits` makes source bytes/tokens/type nesting, module count/edge count/import depth, aggregate source bytes, generic-expansion work and source-map segments explicit compiler policy. Every limit participates in cache and artifact fingerprints; adversarial unit tests require a stable `ResourceLimit` failure with no output.
- `blueice-bluejs` owns the public `BlueJsProgramV1` wrapper and its `bluejs-program-v1` identity. `backend/bluets-bluejs` is the sole crate that depends on both compilers. It consumes a checked, already-tokenized BlueTS source module or closed module graph and directly constructs the BlueJS AST before BlueJS compiles bytecode; it never gives BlueTSC-emitted JavaScript to the BlueJS parser. Its executable v1 subset is the authoritative current direct-bridge subset summarized above, which supersedes the historical phase-level lists in this document. It retains ECMAScript grammar boundaries by rejecting unparenthesized `??` mixed with `&&` or `||`, and an unparenthesized unary exponent base. ESM mode maps local named/default exports and runtime imports to BlueJS module entries. Every runtime request uses the exact canonical target retained by BlueTS's caller-authorized resolver; only runtime-reachable modules become BlueJS nodes, so type-only `.d.ts` inputs remain static. The parser represents initialized local declarations, direct-expression statements, throws, returns, braced `if` bodies, and braced `else if` chains in functions; it records every other body token as opaque so the bridge rejects rather than silently drops a statement it cannot lower. Re-exports and other runtime shapes not listed in the current subset fail explicitly until their direct lowering is implemented.
- The public [BlueTS ↔ BlueJS integration contract](INTEGRATION_CONTRACT.md) records the shipped structured-program bridge and its still-pending source-provenance, safe-point-map and generated host-typings requirements without adding a BlueJS dependency to BlueTS itself.
- `blueice_engine::script::direct_page::DirectPageScriptHost` binds the in-process direct bridge to a live core `TabManager`: a typed `TabId` resolves the current `Page`, its HTTP(S) URL is canonicalized to a realm origin, and a private replacement-document generation forces realm recreation even for same-origin navigation. A core owner may inject a validated `DirectPageRealmOwner` at construction, keeping VM, realm, program-count, bytecode, and static-debug retention limits outside every script request; an over-budget direct attachment leaves no retained program or debug record. Its `realm_stats` exposes only the owning tab's retained program count, bytecode charge, and VM heap statistics; navigation releases the old charge before a successor document can admit code. The optional `run_session_with_script_requests_and_direct_page_host` lifecycle seam synchronizes only previously admitted realms after each session batch. `DirectPageInlineExecutor` is the separate, explicitly configured lifecycle owner for inline declarations: it generates its selected profile itself, may receive that same validated realm owner, runs a document's opted-in inline declarations once in document order, and retains only bounded source-free reports. An over-budget inline attachment leaves no program or debug record and becomes a source-free rejection report. `run_session_with_script_requests_and_inline_page_executor` invokes that owner after each session batch. `blueice-core --inline-bluets-profile <known-profile>` may construct this executor with a core-selected profile and fixed default compiler policy; omitted means no executor. The source-free `GetBlueTsScriptReports` frontend query drains only the addressed tab's retained `(document generation, ordinal, classic/module kind, fixed outcome category)` records. It neither enables execution nor returns source text, compiler diagnostics, bytecode, or runtime values; a binary subprocess regression exercises a classic script, module script, and rejected typed call after real HTTP navigation. BlueJS now provides a realm-local, GC-safe primitive host-function ABI plus a page-runtime registrar that cannot execute bytecode or inspect the VM; registrations disappear with the realm on navigation. `DirectPageScriptHost` installs the checked-in `core-script-document-text-v1` profile and the composed `core-script-document-context-v1` profile only: the latter adds `blueiceDocumentOrigin(): string`, a copied canonical tuple origin with no path, query, fragment, URL object, DOM node, event, or mutation capability. Both values are rebound after replacement. This is not a general `document` object, automatic production script owner, or BlueJS process launcher.
- The real-process inline regression suite now also opens a second tab in the
  same opt-in `blueice-core` instance, navigates both tabs to independent
  script-bearing HTTP documents, and drains `GetBlueTsScriptReports` through
  each tab's addressed envelope. This proves report isolation at the process
  boundary: draining tab one reveals and removes only tab one's record, leaving
  tab two's record available only to tab two's query. It is evidence for the
  narrow report control plane, not a substitute for the outstanding general
  multi-tab page-host, resource-accounting, or debugger acceptance work.
- A separate binary regression replaces one tab's document through two real
  HTTP navigations and drains the report after each one. The report's distinct
  core-private document generations prove the configured executor observes the
  replacement and executes the new inline declaration once; it does not by
  itself prove every stale-handle/debugger invalidation requirement.
- The equivalent standard-JavaScript binary regression navigates one tab through
  two HTTP documents. Each document asserts that `blueiceDocumentOrigin()` is
  its current core-canonical tuple origin before its bounded report is emitted;
  the two successful, distinct-generation reports prove the executor replaces
  the prior realm and snapshot rather than retaining the first document's
  callback. This remains process evidence for the narrow immutable binding,
  not general DOM, debugger, or stale-handle coverage.
- A two-tab standard-JavaScript session regression drives independent HTTP
  documents through the tab lifecycle and drains each addressed
  `GetBlueJsScriptReports` envelope. Draining the first result neither reveals
  nor consumes the second, proving the source-free JavaScript observation queue
  remains tab-isolated at the core session boundary.
- Another binary regression serves a document whose copied text exceeds the
  document-text binding's fixed one-mebibyte contract budget. The normal HTTP
  navigation completes, but inline admission reports only the fixed source-free
  contract-rejection category before BlueTS/BlueJS program admission. This is
  process evidence for the two installed immutable result boundaries, not
  general strict-runtime contract, allocation, provenance, or foreign-data
  enforcement.
- `blueice_engine::debugger` is the core-side dispatcher for the versioned
  debugger IPC. `blueice-core --debugger-socket <path>` accepts a separate
  peer, performs its independent `Hello` negotiation, and forwards later
  requests through a bounded worker-to-session channel. `ListPageRealms`
  returns at most 128 loaded tab/document-generation identities, with no URL,
  source, program, bytecode, or runtime value; an over-cap list fails closed.
  With a selected live `--inline-bluejs` executor, the session additionally
  serves `ListPrograms`, bounded `ListSafePoints`, `ValidateSafePoint`, and v5
  `SetBreakpoint`/`ListBreakpoints`/`ClearBreakpoint` from the exact tab-owned
  BlueJS registry. These return or retain opaque core-minted program IDs and
  instruction-boundary tuples only; they reject wrong tab, realm, program
  generation, and non-boundary inputs instead of remapping them.
  `ProgramLocations` and the deliberately narrower
  `BreakpointConfiguration` capability are available only at that live seam;
  the latter is an idempotent bounded table cleared at realm replacement, not
  a general VM interruption mechanism. With either `--inline-bluejs` or the
  trusted out-of-process child route plus the debugger socket, v5 also offers
  `ArmRootSafePointBreakpoint`,
  `GetExecutionState`, and `ResumeExecution` for a pending classic
  declaration's exact root-code-unit boundary; `ArmEntryBreakpoint` remains
  the zero-offset compatibility form. A non-entry arm runs BlueJS to that
  exact compiler-verified boundary and retains the root interpreter frame
  (operand stack, bindings, scopes, handlers, iterators, completion state,
  and GC roots) until the owner-session scheduler resumes it. A successful
  `ResumeExecution` exposes only the source-free `Resuming` state for one
  bounded session turn before an idle scheduler turn resumes that frame; it is
  observability, not an execution lease. It exposes no source, bytecode,
  stack, scope, object, or completion value. Modules,
  top-level await, child function code units, re-arming/loop hits, stepping,
  arbitrary nested-frame interruption, exception, and runtime-value features
  remain planned. The real-process regressions verify the v4 `ArmEntry`
  compatibility route and the v5 `Hello`/discovery/`Pending`/non-entry-root-
  arm/`Paused`/`Resuming`/same-frame-completion route. The latter rejects
  cross-program and invalid-state resumes without consuming a pending
  declaration, proves child-code-unit, module, repeat-arm, and stale-realm
  targets fail closed, and checks that its debugger replies never reflect
  fixture source/completion data, VM values, or BlueJS opcode data.
- A launcher real-process regression combines `--out-of-process-bluejs` with
  `--debugger-socket` and uses only the public browser and debugger listeners.
  It navigates two local HTTP documents containing one classic declaration,
  discovers only opaque realm/program/safe-point records through the
  launcher-selected core/child route, then proves the first document's realm,
  program, and safe point all fail with `StaleRealm` after the HTTP reload.
  It does not know, configure, or contact the launcher's private child socket
  or capability.
- A parsed `Page` now discovers the explicit non-portable `application/x-blueice-typescript` and `application/x-blueice-typescript-module` declarations in document order, retaining inline source or an external `src` as data. It never grants loading authority or evaluates them: a future page loader must apply origin, feature-profile, integrity, resolver, and resource policy before assembling the `AuthorizedModuleLoader` for direct admission.
- As a deliberately bounded normal-page fixture seam, `DirectPageScriptHost::execute_inline` may admit one inline declaration only. It derives a canonical module ID from core tab/document-generation/declaration identities and creates a one-module closed loader, so it neither embeds caller text in an identity nor reads an external source. `DirectPageInlineExecutor` can invoke that seam automatically only when a core owner explicitly selects it. By default it rejects and reports external `src` without reflecting the page-controlled URL; `PageScriptSourceAuthorizer` is the sole optional core-owned authority that can turn that declaration into a supplied closed graph plus resolver fingerprint. The executor never fetches, resolves, or falls back itself. `HttpOutOfProcessPageScriptSourceAuthorizer` also implements that direct BlueTS-only authorizer interface, reusing its immutable origin rule, URL/SHA-256 manifest, response checks, static-graph parser, limits, cache key, and resolver fingerprint rather than maintaining a second HTTP policy. The direct executor still receives only a completed graph and redacts every authorization failure to its existing source-free report category. A real core-session/loopback-HTTP regression covers document navigation followed by the manifest-authorized external module; a cross-origin declaration is rejected before fetch without URL reflection. This is a trusted in-process core-construction seam only: the reference binary has no HTTP-authorizer CLI/profile or page/frontend/MCP configuration path. General deployment policy and remaining host bindings remain open.
- The out-of-process portion now has a deliberately narrow shared-realm
  bridge: `OutOfProcessJavaScriptPageExecutor`, selected only when core
  receives both a trusted child endpoint and its per-spawn capability token,
  inventories supported JavaScript and explicit BlueTS declarations under one
  DOM-order ordinal sequence, and turns each inline record into a core-minted
  closed one-module graph. The private v5 child protocol carries a trusted
  language tag; JavaScript follows the normal BlueJS parser while BlueTS uses
  only an `AuthorizedModuleLoader` derived from that graph and child-fixed
  checked options before `blueice-bluets-bluejs` directly attaches it to the
  same `BlueJsPageRuntime`. It redacts child outcomes into separate existing
  JavaScript/BlueTS report lanes and sends exact realm closes on navigation or
  tab removal. Its default constructor rejects external `src` without
  reflecting it. The separately named startup-only constructor accepts one
  immutable core-owned `OutOfProcessPageScriptSourceAuthorizer`; it receives
  the exact tab/document-generation/DOM ordinal/parser language-and-kind/raw
  URL/`src` tuple and may return only an existing typed closed JavaScript or
  BlueTS graph. Core checks the returned language, copies its exact canonical
  IDs, static edges, source bytes, and resolver fingerprint into
  `PageHostModuleGraph`, then rejects a malformed graph or authorizer failure
  source-free. The child revalidates the graph and never fetches, resolves a
  URL/import map, reads a filesystem, or gains the authorizer's cache,
  integrity, or network authority. A missing static edge has no fallback.
  `SpawnedBlueJsHost::spawn_for_core` preserves launcher supervision while
  handing that configuration to one trusted core. `blueice-launcher
  --out-of-process-bluejs` creates the child itself and retains it as part of
  the core generation lifecycle; cutover creates a fresh child/capability pair
  for the replacement core. Before realm replacement, core serializes exactly
  two validated primitive snapshots for ordinary JavaScript:
  `blueiceDocumentText()` (1 MiB) and `blueiceDocumentOrigin()` (4 KiB,
  canonical HTTP(S) tuple). The child repeats those limits and canonical
  spelling checks, installs no other binding, and destroys both copies on
  navigation or close. It receives no DOM object, URL, resolver, fetch/cache,
  IPC, or page-selected capability. `blueice-bluets-bluejs` owns the one fixed
  generated `core-script-document-context-v1` artifact shared by the core
  catalog and child: its byte-checked declaration and exact callback inventory
  make only `blueiceDocumentText(): string` and
  `blueiceDocumentOrigin(): string` ambient to child BlueTS. A missing, extra,
  renamed, or page-selected binding fails closed before compiler admission;
  `document`, `fetch`, URL, resolver, and object APIs remain untyped and
  unavailable. A real launcher-to-core-to-child regression observes both
  classic and ESM BlueTS report lanes in that shared realm and verifies a
  wrong callback arity becomes only the fixed source-free compilation
  category. This still is not a general DOM surface.
  `HttpOutOfProcessPageScriptSourceAuthorizer` now creates the closed graph
  itself under a fixed startup policy: canonical same-document origin or one
  canonical owner-selected origin, a URL/SHA-256 manifest, and fixed resource
  bounds. It accepts only a direct `200`, identity-encoded UTF-8 response with
  a language-approved MIME type and matching bounded `Content-Length`; it
  rejects redirects, missing manifest records, bad integrity, ambiguous/bare
  static URL forms, and over-depth/count/byte graphs. It parses manifest-
  covered JavaScript and BlueTS static edges, privately caches only verified
  bytes under deterministic URL/integrity/language keys, and derives a
  policy-complete resolver fingerprint. Core copies only the finished graph
  into the child protocol, while the child still has no fetch, URL-resolution,
  import-map, filesystem, cache, manifest, or fallback authority. The real
  core lifecycle can select exactly one compiled
  `core-page-http-fixture-v1` profile through a private launcher-to-core
  startup selector: it fixes one same-origin classic JavaScript path and its
  SHA-256 expectation, then constructs the regular HTTP authorizer inside
  core before any page executes. Neither the public launcher CLI nor that
  selector accepts URLs, manifests, resolvers, source, paths, or fetch
  settings; the per-document origin only instantiates the fixed same-origin
  policy. A subprocess regression crosses core, a real HTTP origin, and the
  supervised child to prove the finished graph handoff. This is a bounded
  integration fixture, not a general application-resource profile. The v8
  channel additionally carries source-free debugger-location operations (list
  retained programs, list a fixed bounded set of compiler-recorded safe
  points, and validate one exact tuple) and a 256-record exact-breakpoint
  configuration table. The child mints private IDs only; core verifies an
  exact live child realm, remints every public program handle/generation in a
  disjoint core namespace, and rejects mismatched tab/document/program/safe-
  point replies. Set/clear must echo the complete child-private safe-point
  tuple; core rejects duplicate or unmapped list records and revalidates each
  one before reminting a public record. Replacement and close discard both
  private and core mappings. The route deliberately does not proxy
  generic interruption, stepping, VM frames, stacks, scopes, values, bytecode,
  or source. A core-owned debugger socket selects the otherwise default-off
  document lifecycle, which defers document-order declarations for one turn
  and can arm a pending
  classic program at one exact root-code-unit point. It reports only
  `Pending`/`Paused`/`Resuming`/`Completed`; core revalidates the paused private
  tuple before reminting it publicly, and only a later core-owned advance turn
  resumes the same root frame. `ArmEntryBreakpoint`, modules, child code units,
  re-arms/loop hits, and nested interruption remain unavailable.
  Out-of-process discovery/configuration can consume at most 64 such deferred
  turns for a document, after which core advances it even if the peer keeps
  querying. DOM/event callbacks, URL or import-map resolution, broader
  per-client resource authorization, and page-selected compiler
  profiles remain open. The launcher owns an optional stable public debugger
  listener, but no debugger session or opaque handle survives a core cutover:
  each pre-cutover stream remains pinned to its old private core peer and
  closes fail-closed when that generation ends.
- [`bluets-test-interface`](TEST_INTERFACE.md) now exposes the same persistent JSON-lines ready/request/reply transport as BlueJS's test adapter. It is intentionally compile-only, accepts BlueJS's `sloppy` mode as a `raw` alias, and has stable BlueTS diagnostic codes/spans and caller-controlled compiler limits; Test262 runtime execution remains a future bridge concern rather than a hidden BlueJS dependency.
- The [BlueTS test report](TEST_REPORT.md) records the complete per-platform test-suite results, the TypeScript 5.9.3 oracle matrix and per-file line coverage for `blueice-bluets` and `blueice-bluets-bluejs` (2026-09-21).

This is deliberately not a claim of general `tsc` compatibility. Control-flow narrowing, overload resolution, decorators, enums, classes, TSX, namespace emission, parameter properties, and arbitrary JavaScript expression typing remain pending. A construct outside the implemented matrix must be added with a parser/checker/emitter test and a precise compatibility entry; it must not be advertised merely because its tokens happen to be erasable.

BlueTS is a Rust front end feeding the existing BlueJS bytecode compiler and VM. It is **not** a second JavaScript/TypeScript VM, an embedded `tsc` process, or a type-tagged replacement for BlueJS values. TypeScript's ordinary type system is compile-time only: the checker uses it to accept or reject a program, while BlueJS executes ordinary ECMAScript values. The [official TypeScript documentation](https://www.typescriptlang.org/docs/handbook/typescript-from-scratch) calls this "erased types" and states that types do not change JavaScript runtime behavior. BlueTS follows that compatibility rule for ordinary TypeScript, then adds an explicit BlueIce contract mode at foreign-data boundaries rather than silently making every local operation dynamically typed.

This phase depends on Phase 13 completing the BlueJS host, page-script pipeline and an end-to-end `<script>` fixture. It also depends on Phase 17's source-location debugger capability. It must not delay either prerequisite by pretending that a TypeScript parser can substitute for a correct ECMAScript engine or page-script host.

## Current-state correction

The current BlueJS parser accepts ECMAScript grammar, not TypeScript grammar. A colon annotation such as `const value: number = 1;` is a syntax error. The library example reads arbitrary source text and immediately calls BlueJS `parse`, so a `.ts` filename is not a TypeScript mode switch. The Phase 13 host/core wiring and real script-page test are also still open. BlueTS therefore begins after a working JavaScript path exists; it is not a way to claim current page-script support early.

References to TypeScript elsewhere in the plan refer to external automation clients or Node tooling, not page-language support. In particular, Phase 17's planned `@blueice/automation` client is TypeScript *client code* and must not be confused with executing TypeScript in a page.

## Decisions

### One front end, one runtime

Add a proposed `backend/bluets` Rust crate (`blueice-bluets`) with one shared TypeScript front end and lowering IR. Its in-process use in the long-lived BlueJS host permits a typed source program to lower directly to BlueJS's ECMAScript AST or bytecode input; it does not serialize generated JavaScript text and parse it a second time. The same crate also backs the standalone BlueTSC emitter. BlueTS has no network, filesystem, DOM, capability, or script-evaluation authority of its own. It receives source and host-resolved module records from the same page-script loader as BlueJS, consumes the same per-tab compile/memory budgets, and cannot bypass the script IPC, gatekeeper, origin, or module-resolution policies.

The execution path is deliberately narrow:

```text
TS/TS module source
  -> tokenize + parse -> bind -> type check -> shared lowered IR
  -> page path: BlueJS Program / bytecode -> BlueJS VM
  -> build path: BlueTSC ESM emitter -> .js + optional .js.map / declarations
  -> BlueTsDebugInfo + optional contract plans
```

The direct page path has no `TS -> emitted .js text -> parse .js again` round trip. BlueTSC is the deliberately separate build path that serializes the same lowered IR as JavaScript for use outside BlueIce. A test oracle may compare results with a pinned external TypeScript compiler, but Node.js and `tsc` remain test-only tools, never runtime dependencies.

### BlueTSC: standalone JavaScript emitter

`bluetsc` is the proposed BlueTSC command-line compiler. It invokes the same parser, binder, checker, resolver, lowering rules, contract planner and source-span machinery as BlueTS; it must not fork a second, subtly incompatible TypeScript implementation. It offers a build-time alternative for authors who want their project to run in ordinary JavaScript hosts, publish JavaScript packages, inspect emitted code, or move TypeScript checking out of page-navigation startup.

BlueTSC's default output is standard ECMAScript modules (`.js`) preserving ESM import/export semantics, accompanied on request by `.js.map` and `.d.ts` artifacts. Type-only imports/exports, interfaces and type aliases do not appear in emitted JavaScript. The emitter supports only an explicitly declared ECMAScript target matrix; it neither silently produces CommonJS nor claims that every TypeScript feature can target every JavaScript version. Resolution, import-map and declaration policies match BlueTS's declared project configuration, so a source graph cannot type-check one way in the browser and emit another way on the command line.

`bluetsc check` performs parse/bind/type/resolution validation without writing artifacts. `bluetsc build` type-checks first, lowers once, stages all output and publishes it atomically only if every selected entry and dependency succeeds. The default is equivalent to `noEmitOnError`: no stale or partial JavaScript output is described as a successful build. The initial implemented CLI accepts either `check|build <entry.ts>` or `check|build --config bluetsc.json`. A config has `entries`, optional project-root-confined `outDir`, `sourceMap`, `declaration`, `target`, `runtimePolicy`, and `imports`; a config invocation rejects extra CLI overrides so the recorded settings cannot drift. Its import map permits only exact file keys or trailing-slash prefixes to existing project-root-confined local source files/directories. A build always includes root-relative `bluetsc.manifest.json`; a configured map additionally produces the standard-shape `bluetsc.importmap.json` for the resulting ESM tree. This remains a deliberately small resolver, not Node/package-manager resolution.

BlueTSC's source maps map emitted JavaScript back to original TypeScript spans using the same provenance records that make BlueTS debugger locations correct. Declarations describe the checker-approved public surface, not a claim that an unimplemented BlueIce host API exists. JavaScript and declaration outputs carry a compiler/host-typing/configuration fingerprint; consuming an artifact built under a weaker contract policy must be observable and rejectable by a strict project policy.

For a `strict-runtime` project, BlueTSC lowers the same explicit boundary contracts as the direct BlueTS path. It either emits a deterministic, versioned contract helper/module with the bundle or rejects an output form unable to carry the required validators; it never strips the checks merely because the target is JavaScript. `checked` and `transpile-only` outputs are labelled in build metadata and DevTools/source-map extensions so that a deployment cannot be mistaken for strict-runtime output.

### Explicit BlueIce opt-in, not a claim of web compatibility

TypeScript is not a browser script language. BlueTS initially recognizes only these BlueIce-specific script kinds:

```html
<script type="application/x-blueice-typescript" src="app.ts"></script>
<script type="application/x-blueice-typescript-module" src="app.ts"></script>
```

The suffix alone never selects BlueTS, and `text/typescript`, TSX, or a JavaScript MIME type is not silently reinterpreted. The classic/module distinction follows the corresponding BlueJS realm and module rules; fetching, integrity, origin and policy checks remain the page loader's responsibility. These script kinds are deliberately non-portable: an author who needs ordinary-browser compatibility must precompile TypeScript to JavaScript.

The type-safety policy is core-owned configuration attached to the script request or a trusted application manifest. A fetched document cannot weaken it with an HTML data attribute. The initial product policy is `strict-runtime` for direct BlueTS pages; `checked` and `transpile-only` exist only as explicit operator/development choices and are visibly reported to DevTools and automation.

### Static semantics are required, not a cosmetic stripping pass

BlueTS's normal path implements a TypeScript-compatible parser, binder, module resolver and type checker for its declared compatibility version. It resolves source declarations, control-flow narrowing, generic instantiations, overloads, structural assignability and diagnostics before emitting executable bytecode. A parse, resolution, type, lowering, resource, or contract-planning error prevents that script/module from executing; BlueIce never falls back to treating rejected TypeScript as JavaScript or executes a partial output.

`transpile-only` is retained solely for controlled migration tests. It may erase only declared supported syntax, but it is not advertised as type-safe TypeScript support and cannot be the default direct-page mode.

The compatibility target is pinned by a `BlueTsLanguageVersion` and an explicit feature matrix. The first implementation need not promise bit-for-bit `tsc` compatibility. Each accepted syntax, checker rule, emit rule and diagnostic class must name its supported version and tests; unsupported syntax produces `UnsupportedSyntax`, not an accidental BlueJS parser error. BlueTS's runtime target is additionally bounded by the BlueJS ECMAScript feature matrix. A TypeScript construct may type-check yet be rejected as `UnsupportedRuntimeTarget` until BlueJS can faithfully execute its lowered ECMAScript semantics.

BlueTS owns a small `lib.blueice.d.ts` generated from the actual BlueIce DOM, event and network bindings. It must not claim the full web platform merely by importing a broad `lib.dom.d.ts`: declarations for APIs that BlueIce does not expose would make a successful type check dishonest. Browser URL modules and explicitly configured import maps are the initial resolution model. Node resolution, CommonJS, `node_modules`, arbitrary package-manager hooks and arbitrary remote `.d.ts` acquisition are out of scope.

### Runtime contracts protect data boundaries, not every instruction

Static checking proves properties only under the program's declared assumptions. Network JSON, URL/query data, storage, `postMessage`, extension/native IPC, foreign JavaScript module exports and user-provided values can violate those assumptions. In `strict-runtime` mode BlueTS requires an explicit, reifiable `RuntimeContract` for every type-bearing ingress and egress across those boundaries. A TypeScript assertion (`value as User`) never validates data; unchecked `any` is not an escape hatch in strict mode.

BlueTS may synthesize contract plans for a defined subset of types: primitives and literals; arrays, tuples and readonly collections; finite object records with required/optional fields; tagged unions; named recursive interfaces through a cycle-safe contract table; and generic declarations only when every runtime parameter has a supplied reifiable contract. Validation reports a bounded path, expected contract identifier, observed value category and source location without serializing sensitive input by default.

The following are not automatically reifiable: `any`, unconstrained or erased generic parameters, arbitrary conditional/mapped types, opaque/branded intersections, declaration-only ambient values, and types whose validation would require invoking an arbitrary getter, proxy, or user function. BlueTS rejects their automatic use at a strict boundary and requires a developer-supplied contract with a reviewable validator. Contracts must be pure, resource-bounded bytecode/data plans; they may not call arbitrary page code while inspecting untrusted data.

A developer may choose a registered JSON Schema as the data-plan representation for a JSON-like boundary contract. In that case BlueTS delegates compilation and validation to Phase 19's single `blueice-json-schema` engine rather than translating a schema into an ad-hoc second validator. The contract records the immutable `SchemaResourceId`, dialect, vocabulary set, registry generation and validation-plan hash in `BlueTsDebugInfo` and the BlueTSC build fingerprint. A `$schema`, `$id` or `$ref` present in page data never causes a network/file lookup or weakens the originating page's contract policy; only an operator-registered, pinned schema resource can be used. JSON Schema validates data shape at the boundary—it does not infer arbitrary TypeScript semantics, execute format/content handlers, or make a static `TypeId` into a runtime tag.

The common pattern is consequently explicit and honest:

```ts
interface User { id: string; displayName: string }

const user = await fetchJson("/api/me", User.contract);
// `user` is `User` only after the response has validated.
```

After a value passes its boundary contract, local code relies on the static checker and carries no universal runtime type tag. This concentrates dynamic cost where reality introduces untrusted values instead of checking every addition, property read, or function-local assignment. Contracts provide data-integrity checks, not a sandbox: a malicious script that already executes in a page realm is still constrained by BlueIce's origin, capability and gatekeeper policies.

### Type-aware debugger metadata is a required compiler product

Type erasure at the VM boundary does not justify discarding compiler knowledge. Each successful BlueTS compilation emits immutable `BlueTsDebugInfo` keyed by source identity, source-content hash, language version, compiler-option hash, host-typing ABI, contract-policy version and BlueJS bytecode ABI. It contains:

- original TypeScript source spans and a bytecode-safe-point map;
- module URLs, inline-script provenance and source-content hashes;
- `SymbolId` records for declarations, references, scopes and renames;
- interned `TypeId` records for declared and inferred expression types, generic substitutions and diagnostic messages;
- source-level scope membership and lowered-variable correspondence;
- contract IDs, their boundary sites, and validation-failure locations.

For a JSON-Schema-backed contract, the last group additionally links the pinned schema resource, dialect/vocabulary report and standard validation output locations. Phase 12 exposes that data as a bounded contract/schema artifact so an AI and a human diagnose the exact same failed pointer/keyword without serializing or executing the rejected value.

Phase 17's debugger maps breakpoints, stepping, stack frames and exceptions back to TypeScript locations. A scope/watch display shows the runtime BlueJS value separately from its static TypeScript type; it must never claim that a static `TypeId` is a runtime proof. Watch/evaluate expressions are parsed and checked in the paused source scope before BlueJS compilation, with the same fuel, controller-lease and audit requirements as ordinary debugger evaluation. A generic type's displayed instantiation is the checker result at that source site, not invented runtime reification.

For a BlueTS script, source maps are therefore a release prerequisite rather than a deferred DevTools nicety. Debug metadata is retained according to the script/debugger memory policy, released with its tab realm, and redacted from unprivileged automation clients just as source text and scopes are. The same metadata backs the target-aware AI MCP debugger: [Phase 12's debug-environment contract](../phase-12-mcp-server/DEBUG_ENVIRONMENT.md) exposes it only through generation-bound, paged, capability-scoped source/type/symbol/contract resources and tools.

### Performance and cache contract

Type checking is real work; BlueTS does not hide or repeat it unnecessarily. A source/module compilation cache stores bytecode, debug metadata, contract plans and dependency fingerprints under the complete key above plus source bytes. A stale dependency, declaration, import-map, option, policy, host-typing or bytecode-ABI change invalidates the dependent entry. Content-addressed output is immutable; a cache hit never reuses bytecode checked under a weaker policy.

Development mode maintains an incremental module graph and rebinds/rechecks only affected strongly connected components. Production may precompile/cache a verified graph before navigation. A type check is never performed in a VM hot loop. Contract plans are compiled once and applied only at declared crossings; validator work has explicit recursion, byte, collection-length and fuel limits. Phase 8 attributes parser, checker, metadata, cache and validator allocations to the initiating tab.

BlueTS cannot make a bytecode interpreter as fast as ahead-of-time native code. BlueJS interpreter dispatch, DOM IPC, allocation and host calls remain independent performance work. BlueTS's performance objective is instead to avoid duplicate parsing, avoid type work in hot execution paths, keep contracts boundary-local, and make cold compilation/cache costs observable in DevTools.

### Safety, analysis and resource behavior

The Phase 7 gatekeeper receives both the source-level BlueTS capability summary and the post-lowering BlueJS capability summary. Approval is based on the executable lowered behavior; source-level types, declarations and contracts add explanation but cannot hide a network call or DOM mutation introduced by a lowering transform. The summaries are linked by source span and transformed-node identity so an audit can identify their origin.

All tokenizer/parser/checker/contract operations enforce source-size, token, AST-depth, type-instantiation, union/intersection expansion, module-graph, diagnostic, compile-time and metadata budgets. Exceeding one produces a defined resource failure without executing the script, preserving the existing page/document state. BlueTS never resolves imports through arbitrary filesystem access or a checker-owned network request. It accepts only loader-supplied, origin/policy-approved module records.

## Compatibility levels

| Level | Static checker | Runtime contracts | Intended use | Promise |
| --- | --- | --- | --- | --- |
| `transpile-only` | No | Explicit contracts only | Controlled migration and compiler tests | Syntax conversion only; never advertised as type-safe. |
| `checked` | Required | Explicit opt-in contracts | Compatibility/performance measurement under operator control | Type errors prevent execution, but foreign values may remain unchecked. |
| `strict-runtime` | Required | Required at all declared foreign-data boundaries | Default direct BlueTS page mode | Static diagnostics plus validated ingress/egress for the supported contract subset. |

No level treats `as`, `!`, `any`, an unchecked cast, or a `.ts` extension as proof that a runtime value is safe. A page's active level and any unvalidated/unsupported boundary must be visible in its DevTools target metadata.

## Initial language boundary

The first executable slice targets type-bearing but eraseable syntax needed for ordinary typed page code: annotations, interfaces, type aliases, generics, union/intersection/literal types, narrowing, overload declarations, `readonly`, access modifiers, `as`, `satisfies`, non-null assertions, `declare`, `import type`/`export type`, named `export default function` declarations, `export default name` for a local runtime value, and local `export { name as publicName }` bindings. It emits only ECMAScript constructs currently supported by the selected BlueJS target.

The first slice explicitly rejects TSX/JSX, decorators, generic arrow functions, default-export expressions other than a local bare identifier and anonymous default functions, named value re-exports from another module, `enum`/`const enum`, runtime `namespace`/`module`, parameter properties, `import =`, `export =`, declaration merging with runtime emission, and custom transformers. Some are not erasable; for example, ordinary TypeScript enums emit runtime objects. They can be proposed later only with a precise lowering specification, BlueJS runtime support, debugger mappings, capability-summary coverage and differential tests. TSX additionally requires an explicit JSX factory/runtime and is a separate UI-framework decision.

## Delivery order and acceptance

### Slice 0 — JavaScript and debugger prerequisites

1. Complete Phase 13's BlueJS host/core wiring, DOM binding design and a real script-bearing page fixture.
2. Complete Phase 17's script source/safe-point debugger path and the source-map representation required by BlueTS.
3. Connect the shipped resolver-preserving `BlueJsProgramV1` script/module graph hand-off to an origin-preserving page AST/IR, with compiler/cache resource accounting and safe-point validation.
4. Generate `lib.blueice.d.ts` from implemented host bindings using the published host-typing version policy.

Acceptance: a JavaScript fixture executes through the page loader and debugger with a stable source location, while an unimplemented host API is absent from `lib.blueice.d.ts` and fails an attempted typed use.

### Slice 1 — checked TypeScript source and source-level debugging

1. Build the TypeScript tokenizer/parser, binder, URL-module resolver and checker for the initial boundary.
2. Extend direct lowering of supported typed syntax to BlueJS AST/bytecode input, without JavaScript-text round trips.
3. Reject type/resolution/lowering errors atomically and expose structured diagnostics through the script, debugger and automation APIs.
4. Emit `BlueTsDebugInfo`; prove TS source breakpoints, stepping, stack traces, scopes, symbol navigation and static-type displays map to the executing bytecode and to the Phase 12 AI MCP debug interface.

Acceptance: a local typed classic script and module execute with no generated `.js` file; a deliberate type error runs neither the entry nor dependent module; a breakpoint and exception point at original `.ts` lines; a debugger shows a static type and the separate runtime value.

### Slice 2 — BlueTSC JavaScript emission

1. Build `bluetsc check` and atomic `bluetsc build` on the shared checked/lowered IR.
2. Emit standard ESM JavaScript, source maps and the supported declaration output without reparsing or rechecking a different source representation.
3. Define reproducible output/cache fingerprints and ensure source/contract policies, host typings and resolver results cannot drift from direct BlueTS execution.
4. Make strict-runtime output retain or import its deterministic contract helper; reject targets that would silently remove required validation.

Acceptance: a checked local TS module produces deterministic ESM JavaScript and a source map pointing to original `.ts` lines; an error leaves the previous output intact and reports no successful artifact; the resulting JavaScript has the same supported observable behavior as direct BlueTS execution; a strict boundary check still fails malformed input after emission.

### Slice 3 — strict runtime contracts

1. Define the pure contract IR, its reifiable type subset, bounded validation engine and failure schema.
2. Add contracts for JSON/Fetch/XHR once Phase 17 supplies those APIs, then URL/query, storage, `postMessage`, foreign module and native/extension boundaries as each exists.
3. Make `strict-runtime` require coverage of each supported boundary; reject `any`/unreifiable types there unless a declared reviewed contract is supplied.
4. Attribute validator/cache/debug allocations to the tab and feed lowered contract behavior to the gatekeeper summary.

Acceptance: malformed JSON cannot enter a `User`-typed value; the validation error names the bounded failing path and source location; valid data reaches BlueJS without repeated local type checks; a recursive/deep/oversized input fails within the resource budget.

### Slice 4 — incremental projects and compatibility growth

1. Implement dependency-aware incremental checker/cache invalidation and trusted local declaration files. Both, plus external-oracle coverage and bounded erasable generic substitution, are complete in the standalone front end.
2. Expand the supported TypeScript feature matrix only alongside its BlueJS lowering/runtime, source mapping, contract and oracle evidence.
3. Compare the supported fixture matrix against a pinned TypeScript compiler for accepted syntax/diagnostics and add Node/BlueJS behavioral differential tests for the lowered output.
4. Expose carefully redacted project/type metadata to Phase 17 DevTools and the documented, capability-scoped Phase 12 MCP debug interface.

Acceptance: editing one module invalidates only its dependents; a cache entry checked under another policy is refused; every supported compatibility feature has parser/checker/lowering/debug/contract expectations and an independently checked fixture.

## Testing strategy

- Keep public parser, binding, checker, lowering, contract and debugger fixtures under the eventual `backend/bluets/tests/` boundary, with shared HTML page fixtures beside the Phase 13/17 integration tests.
- Test diagnostics by stable code/category, source span and semantic condition—not copied compiler-message wording. Every unsupported feature must have an explicit rejection fixture.
- Compare accepted syntax and checker behavior against the pinned TypeScript 5.9.3 reference in the CI BlueTSC compatibility-oracle job (`BLUEICE_BLUETSC_ORACLE=/absolute/path/to/tsc cargo test -p blueice-bluets --test typescript_oracle -- --ignored`); compare resulting runtime behavior against the reference JavaScript output where the selected BlueJS feature subset supports it. The reference never determines BlueTS's security policy or makes an unsupported test silently pass.
- Run BlueTSC's emitted JavaScript and direct BlueTS bytecode against the same deterministic fixtures; compare public results, exceptions, module order, source-map locations, contract failures and type-only import elision. Assert an erroneous multi-entry build publishes no partial artifact set.
- Test contracts with malformed, adversarial, recursive, cyclic, getter/proxy-like, deep, oversized and resource-exhausting inputs. Assert no arbitrary user code runs during a pure validation path.
- Test debugger round trips: source breakpoints, async/exception locations, renamed symbols, type displays, erased type-only imports, transformed spans, stale source-map rejection and privacy redaction.
- Test cache keys/invalidation, per-tab attribution, failed compilation atomicity, page/gatekeeper policy enforcement and multi-tab isolation through real processes once the host exists.

## Explicit non-goals for the first release

These limits describe the completed first release. Phase J below schedules
the requested language and package features for later compatibility work;
this section does not remove them from that backlog.

- Claiming full `tsc`/`tsserver` compatibility, all TypeScript syntax, all diagnostics, or all JavaScript package ecosystems.
- Using BlueTSC as an opaque wrapper around a bundled `tsc`, or permitting its emitter to drift from BlueTS's parser/checker/lowering rules.
- Replacing BlueJS with a TypeScript VM, preserving universal runtime type tags, or adding a type check to every local operation.
- Treating TypeScript types or contracts as a security sandbox, permission system, or substitute for Phase 7 review.
- Executing TSX/React, decorators, enums, runtime namespaces, CommonJS, Node builtins, arbitrary `node_modules`, arbitrary remote declarations, or custom compiler transformers.
- Advertising complete web-platform typings before the corresponding BlueIce host APIs and their runtime behavior exist.
- Exposing source text, static types, diagnostics, contracts or debugger scopes to an unauthenticated/unprivileged automation client.

## Phase J — requested TypeScript 5.9.3 compatibility expansion

On 2026-09-28 the user requested JSX/TSX, decorators, enums, CommonJS, and
arbitrary package resolution as development requirements toward full
TypeScript/`tsc` support. Classes and runtime namespaces are prerequisites
already named by J.1 and remain in scope. The compatibility reference is the
repository's pinned TypeScript 5.9.3 oracle. "Full" is a release claim only
after J.6 inventories and closes the remaining compiler, configuration,
declaration, emit, and CLI gaps; adding these named features alone is not
sufficient. The compiler and direct-page paths share one checked IR. BlueTSC
may emit code for a configured JavaScript host, while direct BlueTS execution
also requires the corresponding BlueJS semantics and owner-authorized host
bindings. A declaration or package name never creates such a binding.
Remote declaration acquisition is a separately authorized BlueIce extension;
it is not part of ordinary `tsc` package resolution.

The following records the J.1 lowering and authority decision for each
accepted proposal. Target-dependent transforms must use the configured
TypeScript/ECMAScript version and retain a reproducible fingerprint.

| Feature | Runtime lowering | Host authority and admission |
| --- | --- | --- |
| Classes | Preserve a native class when the BlueJS/output target supports its exact semantics; otherwise lower inheritance, fields, accessors, private members, parameter properties, and static initialization with versioned helpers and TypeScript-compatible order. | No new filesystem or network grant. Constructor, initializer, and computed-key effects enter the lowered capability summary and existing runtime budgets. |
| Enums | Emit TypeScript-compatible runtime objects and numeric reverse mappings; evaluate members in declaration order. Inline `const enum` only under the matching compiler options; keep declaration output consistent. | No new host grant; computed member expressions retain their ordinary capability effects. |
| Runtime namespaces/modules | Emit ordered namespace initialization and merge into the correct value object; ambient declarations emit no runtime object. | No new host grant. Namespace initialization executes only through the already authorized module or page. |
| JSX/TSX | Parse `.tsx` distinctly; lower according to the selected `jsx` mode to a configured classic factory or automatic runtime import, or preserve JSX only in a non-executable output mode. Include fragments, attributes, spreads, and children. | A factory/runtime package is an ordinary executable dependency requiring resolver and page-loader authorization. JSX syntax never imports React or grants DOM capabilities by itself. |
| Decorators | Keep standard and `experimentalDecorators` legacy semantics separate. Lower evaluation/application order, replacements, class/field/auto-accessor initialization, and the applicable metadata/parameter-decorator options with versioned helpers. | Decorator expressions are executable user code. Their helper and package effects enter the capability summary; metadata does not grant reflection or other host APIs. |
| CommonJS | Select ESM/CJS per configured module rules; emit `require`, `exports`, `module.exports`, `import = require`, and `export =` with correct cache, cycle, and interop behavior. Direct BlueTS requires an explicit compatible loader/realm, not an ESM text rewrite. | Resolution reads only owner-authorized dependencies. Node builtins or dynamic `require` need an explicit host capability/runtime profile; a browser page receives none from CJS syntax alone. |
| Installed packages | Resolve any package name present in the configured dependency tree using the selected TypeScript resolution mode, `package.json` conditions, declarations, `@types`, and symlink-aware canonical identities. This is a compile/resolution step, with no implicit package installation. | The owner sets canonical search roots. Imports outside those roots and source-requested package-manager hooks are refused; page execution still uses the page loader's origin and capability checks. |
| Remote declarations | Fetch only explicitly configured declaration URLs through the existing authorizer, pin the content hash, and include it in graph/cache fingerprints. `.d.ts` has no runtime lowering. | No declaration may trigger a fetch or grant a runtime global. Network admission, size/time budgets, provenance, cache ownership, and replacement/close release remain host-owned. |

The J.2 debugger, contract, and conformance decision is also per proposal:

| Feature | Debugger mapping | Contract policy and conformance gate |
| --- | --- | --- |
| Classes | Bind constructor, methods, initializers, accessors, and generated helpers to original class/member spans or mark helper-only instructions unbound. | Preserve strict checks at constructor/field host crossings; compare class order, `this`/`super`, private access, `.js`, `.d.ts`, maps, and direct behavior with pinned `tsc` and Node/BlueJS. |
| Enums | Map emitted member initialization and inlined references to their source declarations and uses. | Enum types do not validate foreign values automatically; compare numeric/string/reverse mapping, computed members, `const enum` options, declarations, and diagnostics. |
| Namespaces | Map each merged declaration and generated initializer to its own source span. | Ambient declarations grant no runtime value or contract; test merging, visibility, initialization order, and emitted/declaration artifacts. |
| JSX | Map each tag, attribute, spread, child, factory call, and generated import to its original TSX span. | Props from foreign data follow the existing strict boundary contract; test every selected JSX mode, JSX typing, factory options, emitted imports, maps, and direct execution where output is executable. |
| Decorators | Map decorator expressions, application sites, replacement values, and helper frames to original declarations; expire maps with the program generation. | No implicit validation of decorator-produced values; test standard versus legacy order, replacement, metadata, declarations, maps, and target-specific output separately. |
| CommonJS | Map wrapper, require call, export assignment, and interop helper to source; retain module identity across cycles. | Foreign module exports require a reviewed contract at strict boundaries; compare module mode, cache/cycle order, interop, diagnostics, CJS output, and source maps. |
| Installed packages | Preserve canonical source identity and original positions through package declarations and selected export conditions. | Third-party types do not prove host bindings; test TypeScript resolver decisions, denied escapes, dependency changes, `@types`, conditional exports, and cold/warm cache invalidation. |
| Remote declarations | Retain URL/hash provenance without exposing source to ungranted debugger clients. | Pinning and explicit authorization precede type use; test changed content, stale cache, denied fetch, budget exhaustion, and absence of runtime authority. |

Use TypeScript's [module-resolution reference](https://www.typescriptlang.org/docs/handbook/modules/reference.html),
[module-mode reference](https://www.typescriptlang.org/docs/handbook/modules/theory.html),
[JSX modes](https://www.typescriptlang.org/tsconfig/jsx.html), and
[standard-versus-legacy decorator explanation](https://www.typescriptlang.org/docs/handbook/release-notes/typescript-5-0.html)
as versioned design inputs. The pinned 5.9.3 executable, rather than a
floating documentation page, decides oracle expectations.

Implementation order is J.3 declarations, J.4 module/package foundation,
J.5 JSX/decorators, then J.6 compatibility closure. Each leaf needs public
parser/checker/emit tests, direct-page or explicit target-admission tests,
debugger/source-map and strict-boundary tests, and accepted/rejected oracle
fixtures. J.6 must enumerate every remaining `tsc` 5.9.3 syntax, diagnostic,
`tsconfig`, library, emit target, declaration, project-reference, and CLI gap
before any full-compatibility claim. The existing first-release gates remain
valid; Phase J completion requires a fresh workspace and platform gate. Local
Rust work uses the one shared `target` through
`scripts/test-with-disk-budget.sh`, with its target-size and free-space
limits. Reuse cached oracle dependencies and bounded temporary fixtures;
do not create a second Cargo target or duplicate package installations.

### J.3.1.1 Named class parser shell

The first class leaf is a parser representation, not an executable class
claim. `class Name { ... }` and `export class Name extends Base { ... }`
retain the name, optional single-identifier heritage, bounded original body
tokens, export bit, and source spans. Missing names/bodies and unterminated
bodies are parser errors; generics, computed heritage, and `implements` stay
explicitly unsupported in this form. The checker reports `UnsupportedSyntax`
for every retained class, so BlueTSC publishes no output and direct BlueTS
cannot execute a partially erased class. The bridge also has an explicit
class refusal if given such an unchecked module. J.3.1.2 will replace the
opaque member tokens with checked constructor and method structures.
Public parser tests prove the exact source spans and token body for two
declarations, including an exported subclass, and reject incomplete,
generic, computed, and implemented headers. A public `compile` call returns
`UnsupportedSyntax` and no artifact for a retained class; the existing
frontend rejection fixture now expects that precise fail-closed stage.
The BlueTS and bridge library suites, workspace tests with local Unix-socket
access, workspace all-target Clippy, and rustfmt pass with one disk-budgeted
Cargo target of about 13 GiB and about 716 GiB free. A final focused parser
test covers the added computed-heritage refusal after the workspace run.

### J.3.1.2.1 Class member boundaries

The class shell now partitions its original body tokens into exact member
ranges. A `constructor(...) { ... }` or simple named `method(...) { ... }`
gets a constructor/method tag and original byte span; a one-token return
annotation and signature semicolon are retained as part of the same range.
Unimplemented fields and malformed or richer member forms remain opaque.
Each shell stores indexes into the class's bounded token vector rather than
another token copy. This is a parsing aid only: the checker still refuses
every class before BlueTSC emission or direct execution. A public parser
regression verifies two member spans and that an opaque field does not hide
a following simple method. The BlueTS crate suite, workspace all-target
Clippy with warnings denied, rustfmt, and whitespace checks pass; the single
guarded Cargo target remains about 13 GiB with about 716 GiB free.

### J.3.1.2.2 Constructor syntax and body items

The existing function parameter parser is now shared with constructor
members, preserving its bounded type, optional/default, and erasure behavior
without a second grammar. A constructor shell retains parsed parameters and
either a signature-only marker or structured function-body items; its member,
parameter, and local-variable spans remain original source ranges. The
parser rejects incomplete parameter/body forms and a constructor return-type
annotation. This step does not make a class executable: the checker still
returns `UnsupportedSyntax` with no BlueTSC output, and the direct bridge
refuses the class. Public parser and compile regressions cover retained
annotations, two erasure positions, body items, overload signatures, empty
bodies, malformed shapes, and no output. The full BlueTS crate and bridge
library suites, workspace all-target Clippy, rustfmt, and whitespace checks
pass in the shared disk-budgeted target; the final body-span assertion has a
focused passing rerun.

### J.3.1.2.3.1 Simple named methods

Simple `name(parameters)` methods now reuse the shared parameter and
function-body parser. A one-token return annotation is parsed as a type
and erased from the eventual JavaScript; a semicolon retains a method
overload signature, while braces retain structured body items. Each method
keeps its original member span and name. Incomplete method headers are
parser errors; accessor syntax and richer method shapes remain opaque until
J.3.1.2.3.2. The class checker still returns `UnsupportedSyntax`, so neither
an overload signature nor an implementation reaches emission or execution.
A public parser test verifies a same-name signature/implementation pair,
parameter/return types, four erasure positions, original span, and no
artifact; another test covers malformed methods and opaque accessors. The
BlueTS crate and bridge library suites, workspace all-target Clippy,
rustfmt, and whitespace checks pass under the shared-target disk guard.

### J.3.1.2.3.2 Bounded method return types and opaque routes

The member boundary scan now walks bounded return-type tokens past union
arms and balanced record types before deciding which `{` opens the method
body. The existing type parser then retains the actual union or record
return type and its erasure span. Richer class member syntax remains opaque:
public/private modifiers, accessor prefixes, computed and private keys, and
generic method heads have explicit public parser fixtures that prove they
do not become a plain named method. Every class remains checker-rejected,
so these opaque forms cannot execute or leak into BlueTSC output. The full
BlueTS crate suite, workspace all-target Clippy with warnings denied,
rustfmt, and whitespace checks pass in the shared disk-budgeted target.

### J.3.1.2.3.3 Class method overload grouping and parser oracle

The class parser retains contiguous same-name method signatures and their
implementation as one group of indices into the original member vector. A
later implementation of the same name begins a new group. Missing
implementations and an opaque field interrupting a group remain visible to
the later checker; no method becomes executable through this parser change.

Six shared source fixtures close the parser-only oracle leaf J.3.1.2.3.3.2.
The pinned TypeScript 5.9.3 executable accepts number/string overloads with
a union implementation, a record return type, and a private method. BlueTS
retains the first two as structured method groups and routes the private
method to an opaque member pending J.3.2. The pinned compiler rejects an
orphan signature and a signature interrupted by a field with TS2391, and an
incompatible overload implementation with TS2394; each diagnostic is on the
fixture's expected line. The oracle uses `--noEmit` and verifies no output
files remain.

The public BlueTS parser test reads those same sources, checks original class
spans, method group indices, and the opaque boundary, then confirms every
checked class has one `UnsupportedSyntax` diagnostic at that span and no
BlueTSC artifact. J.3.1.2.3.3, J.3.1.2.3, and J.3.1.2 are complete; class
semantic acceptance remains gated on J.3.1.3–6.

J.3.1.3 is split into method-group validation, class constructor and
instance/static binding, body and `this` checking, inheritance and `super`,
then pinned-oracle checker closure. This order lets each checker step retain
the current fail-closed class admission rule until the full checker, emitted
JavaScript, and direct runtime are ready.

### J.3.1.3.1 Class method-group checker gate

The checker now validates retained class method groups without admitting
classes for execution. A signature group without its immediate implementation
reports its last signature's original span. Duplicate implementations report
each duplicate member's span. Explicitly typed overload signatures compare
their argument and return types to the implementation through the existing
bounded structural relation; incompatible types and exhausted expansion
budgets receive distinct diagnostics. The seven fixture sources compare
accepted groups and the TS2391, TS2393, and TS2394 failure lines against the
pinned TypeScript 5.9.3 executable under `--noEmit`.

The class `UnsupportedSyntax` refusal now occurs during binding for every
runtime policy, including `transpile-only`; a public regression verifies no
artifact escapes through that mode. The direct BlueTS bridge continues to
reject unchecked class nodes. The full BlueTS and bridge crate suites,
workspace all-target Clippy, rustfmt, and whitespace checks pass using the
same compact Cargo target and the existing pinned Test262 corpus via an
ignored symlink. J.3.1.3.2 must add class constructor and instance/static
types before valid classes can be checked as values.

### J.3.1.3.2.1 Local class type and constructor-side binding

J.3.1.3.2 is split into local dual-namespace binding, checked construction,
and instance/static member access with closed-module imports and exports.
The first step binds each local named class to a structural instance type
whose method fields retain their original source spans. The separate value
side is a constructor-side record with a `prototype` field of that instance
type. This lets the checker distinguish `Reader.prototype` from `Reader`
without pretending a class is a callable function. Construction and full
member-call validation remain the next leaves.

Illegal class/class, type-alias/class, and value/class collisions report the
second name's original span. A class/interface same-name declaration is
legal in TypeScript; either source order therefore avoids an invented
duplicate diagnostic while actual declaration merging waits for J.3.5.
No class symbol enters published debugger metadata and no class artifact is
emitted while class execution remains unsupported. Seven pinned TypeScript
5.9.3 `--noEmit` cases cover accepted instance/constructor-side assignments,
a wrong-side TS2741, TS2300/TS2451 collisions, and both interface/class
orders. The public BlueTS compile test reads the same fixture sources,
checks corresponding diagnostics and exact duplicate identifier spans, and
confirms no output. The BlueTS crate suite, workspace all-target Clippy,
rustfmt, and whitespace checks pass with the reused compact Cargo target.

### J.3.1.3.2.2 Bounded local class construction

The checker now recognizes an exact `new LocalClass(...)` expression and
infers the already-bound named instance type. A bounded scan also checks
class construction nested in a supported expression such as a function-call
argument. It selects declared constructor overload signatures when present,
otherwise the implementation signature or the implicit zero-argument
constructor. Argument count, primitive type, tuple spread, and type-expansion
budgets use the existing function-call relation; a failure reports the
original construction span. Eight pinned TypeScript 5.9.3 `--noEmit` cases
cover accepted direct/overloaded calls, wrong argument and arity, an inferred
instance-shape mismatch, and a nested invalid argument. The oracle's
diagnostic parser now locates the source coordinate before the `error TS`
marker, so parentheses in TypeScript diagnostic prose cannot hide a line.

An inherited constructor may carry its parent's parameters. Heritage and
`super` are J.3.1.3.4 work, so construction of a derived class remains
checker-deferred while every class is still refused for output; a pinned
accepted fixture prevents an invented zero-argument diagnostic. The public
BlueTS test proves exact source spans, no artifact, and a bounded scan. The
BlueTS and bridge crate suites, workspace all-target Clippy, rustfmt, and
whitespace checks pass using the same compact target. J.3.1.3.2.3 must
finish instance/static member lookup, direct class-call refusal, and
closed-module class imports/exports.

### J.3.1.3.2.3.1 Local instance member access and class-call refusal

The bound instance method shape now supports a local class instance's method
read and call. A missing instance method on the constructor side reports the
original `Class.method` or `Class.method(...)` span. A bounded expression scan
rejects direct and nested calls of a local class value without `new` at the
original call span. It checks the active scope, so a parameter that shadows
the class remains callable or constructible. The scan uses the compiler's type-expansion limit
and reports ResourceLimit when that bound is crossed. Class output remains
unconditionally refused.

Six pinned TypeScript 5.9.3 `--noEmit` fixtures cover the accepted instance
method read and call, both wrong-side accesses, direct and nested class calls,
and a shadowed callable parameter. Public BlueTS tests assert accepted versus
rejected behavior, precise spans, no artifact, and the scan budget. The BlueTS
and bridge crate suites, workspace all-target Clippy, rustfmt, and whitespace
checks pass using the reused compact Cargo target. Static methods and
closed-module class bindings remain separate J.3.1.3.2.3 leaves.

### J.3.1.3.2.3.2.1 Static method shell and side-aware groups

The bounded class parser now recognizes `static name(...)` signatures and
implementations as methods with an explicit static marker. The member shell
and method retain their original source span. Method grouping keys on both
name and side: a static overload and implementation form one group while an
instance method of the same name remains separate. The duplicate method
validator uses the same side-aware key, and the instance type builder omits
static methods. A member spelled `static()` remains an instance method.

The public parser boundary test checks these distinctions and confirms that
class compilation still produces no artifact. The pinned TypeScript 5.9.3
`--noEmit` oracle accepts the same source. The BlueTS and bridge crate suites,
workspace all-target Clippy, rustfmt, and whitespace checks pass with the
reused compact Cargo target. Static member type binding and call checks are
the next leaf.

### J.3.1.3.2.3.2.2 Constructor-side static method types and calls

Static method signatures now extend a local class's constructor-side record
beside its typed `prototype`; the instance record retains only instance
methods. A single static method is checked through ordinary member-call
validation. A multi-signature static group uses the existing bounded
function-signature selector for argument checking and return inference.
The implementation signature remains hidden when overloads exist. A missing
static method on an instance, a missing instance method on the constructor,
and bad static arguments report the original member expression or call span.
No class artifact is admitted.

Public checked-compile tests cover accepted static reads, single and
overloaded calls, return inference, wrong-side reads and calls, invalid
arguments, exact spans, and no output. Six pinned TypeScript 5.9.3 `--noEmit`
fixtures match the accepted forms and TS2576, TS2345, and TS2769 rejection
lines. The BlueTS and bridge crate suites, workspace all-target Clippy,
rustfmt, and whitespace checks pass using the reused compact Cargo target.
Closed-module class binding is the remaining J.3.1.3.2.3 leaf.

### J.3.1.3.2.3.3.1 Closed-module class instance type imports

The closed project type surface now includes an exported class's instance
definition. A local value export alias or local `export type` alias can also
expose the class type without exposing a private class by accident. `import
type` binds that definition in the consuming module, allowing instance method
reads and calls through the imported name. The value-export validator now
recognizes a class as a local runtime declaration, while class output remains
refused. Imported class instances use their original member spans for a
wrong-side static method diagnostic.

Public checked-compile tests use two-module graphs for direct and aliased
imports, accepted instance methods, a rejected static method on an instance,
and a rejected private-class import. Four pinned TypeScript 5.9.3 `--noEmit`
graphs match acceptance and TS2576/TS2459 diagnostic lines. The BlueTS and
bridge crate suites, workspace all-target Clippy, rustfmt, and whitespace
checks pass using the reused compact Cargo target. Value-imported class
constructors and static sides are the next leaf.

### J.3.1.3.2.3.3.2.1 Closed class value origin and alias binding

A bounded project export table now retains each directly exported or locally
aliased class's source name, instance shape, constructor-side shape, and
constructor signatures without copying its body tokens. Value imports bind
the class in both type and value namespaces. The imported name replaces the
source class's self references inside method types, `prototype`, and
constructor signatures. Type-only imports use the same origin to specialize
their instance method returns under local aliases; type-only export and
re-export edges retain no runtime value authority. Class output remains
refused throughout the closed graph.

Public two-module tests check direct and aliased class value imports against
constructor and instance shape assignments, a wrong-side constructor-value
assignment, exact source spans, and no output. Three pinned TypeScript 5.9.3
`--noEmit` graphs match the accepted forms and TS2741 rejection. The prior
type-only import oracle now covers an aliased self-return. The BlueTS and
bridge crate suites, workspace all-target Clippy, rustfmt, and whitespace
checks pass with the reused compact target. Imported construction and member
calls are the next leaf.

### J.3.1.3.2.3.3.2.2 Imported class construction and member calls

Value-imported classes now use the same bounded constructor and member-call
relations as local classes. Direct and locally aliased imports support
`new`, static calls, and instance calls with selected return types. Invalid
constructor or method arguments, wrong-side member calls, and class calls
without `new` report the original use span. A bounded runtime-expression
scan also reports a class imported only for types, or exported only with
`export type`, when used as a value. It respects a shadowing runtime binding
and skips erased type-assertion operands. The closed source graph still
refuses every class artifact.

Public checked-compile tests cover three accepted and eight rejected
two-module forms, exact source spans, and no output. Eleven pinned TypeScript
5.9.3 `--noEmit` graphs agree on acceptance and TS2345, TS2339, TS2576,
TS2348, TS1361, and TS1362 diagnostic lines. The BlueTS and bridge crate
suites, workspace all-target Clippy, rustfmt, and whitespace checks pass with
the reused compact Cargo target. The full workspace suite also passes when
its Unix socket tests run outside the filesystem sandbox. J.3.1.3.2's local
and closed-module type/value separation is complete; constructor and method
body semantics follow in J.3.1.3.3.

### J.3.1.3.3.1 Constructor overload and parameter validation

The class checker now validates every parsed constructor parameter annotation
and implementation default in source order. A default can read an earlier
typed parameter, while an overload signature cannot own a default. Constructor
signatures require one immediately following implementation; missing,
interrupted, duplicate, and incompatible groups report original member spans.
The compatibility relation uses the same bounded type-expansion budget as
class method overloads. Class output remains refused.

Public checked-compile coverage checks accepted overload and default forms,
rejected parameter types/defaults and group shapes, both duplicate member
spans, and no artifact. Nine pinned TypeScript 5.9.3 `--noEmit` cases agree on
acceptance and TS2390, TS2392, TS2394, TS2304, TS2322, and TS2371 lines.
The BlueTS and bridge crate suites, full workspace suite, workspace all-target
Clippy, rustfmt, and whitespace checks pass with the reused 12 GiB Cargo
target. Constructor body scope and return checking follow in J.3.1.3.3.2.

### J.3.1.3.3.2 Constructor body scopes and returns

Constructor implementations now walk their structured body in source order.
Parameter annotations and defaults seed the scope; local declarations add
checked annotations or inferred initializer types. Expression statements,
throws, calls, and returns use that scope. Braced `if`, `while`, and `try`
paths are checked recursively with local branch and catch scopes. A bare
return and a primitive return follow TypeScript constructor behavior. Returned
objects must fit the bounded class instance method shape, with diagnostics at
the original return span. Class output remains refused, and inherited `super`
is still the J.3.1.3.4 heritage task.

Public checked-compile tests cover accepted typed locals and primitive or
aliased-primitive returns; rejected locals, calls, direct object returns, and
nested object returns check original spans and no output. Seven pinned
TypeScript 5.9.3 `--noEmit` cases agree on acceptance and TS2322, TS2345,
TS2741, and TS2409 diagnostic lines. BlueTS, bridge, and full workspace
suites, workspace all-target Clippy, rustfmt, and whitespace checks pass with
the reused 12 GiB Cargo target. Method-body checking follows in J.3.1.3.3.3.

### J.3.1.3.3.3.1 Method parameters and body scopes

Instance and static methods now validate every parameter annotation, bounded
rest form, and default initializer; overload signatures reject defaults.
Constructor and method bodies share one source-ordered structured walker,
which checks typed local declarations, expression statements, calls, throws,
and nested branch/loop/catch scopes. Method return expressions receive runtime
checks here; declared return compatibility and fallthrough are the next leaf.
An invalid overload-signature default is diagnosed at its parameter span
without a redundant overload-compatibility error. Class output stays refused.

Public checked-compile coverage checks accepted instance/static typed scopes
and prior-parameter defaults, rejected locals, calls, parameter types and
defaults, exact source spans, and no artifact. Seven pinned TypeScript 5.9.3
`--noEmit` cases agree on acceptance and TS2322, TS2345, TS2304, and TS2371
lines. BlueTS, bridge, and full workspace suites, workspace all-target Clippy,
rustfmt, and whitespace checks pass with the reused 12 GiB Cargo target.

### J.3.1.3.3.3.2 Method return annotations and flow

The shared class-method IR now retains the exact source span of a return
annotation. Instance and static method signatures validate that type, and
implementation bodies compare source-ordered direct and nested returns with
the declared type. Bare returns are checked as `undefined`; `void` and types
accepting `undefined` permit fallthrough. Structured bodies that can reach
their end without a required result report the original method span. An
unresolved annotation reports its type name without a cascading return
diagnostic. Overload implementation bodies follow the same checks. Class
output remains refused.

Public checked-compile cases cover accepted all-branch and `void` returns,
invalid instance, static, bare, `void`, and overload-body returns, fallthrough,
and an unknown annotation at exact source spans with no artifact. Eight pinned
TypeScript 5.9.3 `--noEmit` cases agree on acceptance and TS2322, TS2366, and
TS2304 diagnostic lines. BlueTS, bridge, and full workspace suites, workspace
all-target Clippy, rustfmt, and whitespace checks pass in the reused 12 GiB
Cargo target. This closes J.3.1.3.3.3; bounded `this` and overload-call
inference remain in J.3.1.3.3.4.

### J.3.1.3.3.4.1 Instance `this` in class bodies

The bounded function-body parser now retains expression statements starting
with `this`. Constructor and instance method bodies bind `this` to their
class's instance type. Expression inference uses that binding for a bare
`this`, member reads, and method-call results; runtime checks validate its
method arguments and reject constructor-side methods through the same member
relation as ordinary instances. Every diagnostic retains the original call,
read, local declaration, or return span. Class output remains refused.

Public checked-compile coverage asserts an accepted constructor call and
self-return, wrong arguments, wrong-side call and read, incompatible inferred
call result, incompatible `this` return, exact spans, and no artifact. Six
pinned TypeScript 5.9.3 `--noEmit` cases agree on acceptance and TS2345,
TS2576, and TS2322 lines. BlueTS, bridge, and full workspace suites,
workspace all-target Clippy, rustfmt, and whitespace checks pass with the
reused 12 GiB Cargo target. Constructor-side `this` follows in J.3.1.3.3.4.2.

### J.3.1.3.3.4.2 Static `this` in class bodies

Static method bodies now bind `this` to the class's constructor-side record.
The existing member relation checks static reads and calls and rejects
instance-only members. Calls and reads through this bound `this` keep their
original expression span; result inference flows into local and declared
return checks. A constructor-side value is not accepted where the instance
type is required. Overload selection on static `this` follows in
J.3.1.3.3.4.4, and class output remains refused.

Public checked-compile cases cover an accepted static call, wrong arguments,
wrong-side reads and calls, an incompatible inferred local, and an invalid
constructor-side self return at exact spans with no artifact. Six pinned
TypeScript 5.9.3 `--noEmit` cases agree on acceptance and TS2345, TS2339,
TS2322, and TS2741 lines. BlueTS, bridge, and full workspace suites,
workspace all-target Clippy, rustfmt, and whitespace checks pass with the
reused 12 GiB Cargo target.

### J.3.1.3.3.4.3 Instance method overload calls

Bound class instances, including `this` in instance bodies, now select a
method overload with the bounded function-signature relation. The selected
signature supplies the inferred return type for local initializers and
declared method returns; an unmatched argument list reports the original
call span. This class path precedes the existing exact callback-overload
rule, which remains available for other structural receivers. Class output
remains refused.

Public checked-compile cases cover accepted number/string overloads on an
instance and `this`, bad arguments through both receivers, incompatible
inferred locals and returns, exact spans, and no artifact. Five pinned
TypeScript 5.9.3 `--noEmit` cases agree on acceptance and TS2769/TS2322
lines. BlueTS, bridge, and full workspace suites, workspace all-target Clippy,
rustfmt, and whitespace checks pass with the reused 12 GiB Cargo target.
Static overload selection follows in J.3.1.3.3.4.4.

### J.3.1.3.3.4.4 Static method overload calls

Bound class values and static `this` in class method bodies now use the same
bounded overload-signature selector. It checks argument lists at the original
call span, and the selected signature supplies the inferred result for local
initializers and declared method returns. Class output remains refused.

Public checked-compile cases cover accepted number/string overload calls,
bad arguments on both receivers, incompatible inferred locals and returns,
exact spans, and no artifact. Five pinned TypeScript 5.9.3 `--noEmit` cases
agree on acceptance and TS2769/TS2322 lines. This closes J.3.1.3.3;
BlueTS, bridge, and full workspace suites, workspace all-target Clippy,
rustfmt, and whitespace checks pass with the reused 12 GiB Cargo target.
Inheritance, overrides, cycles, and `super` follow in J.3.1.3.4.

### J.3.1.3.4 Named class inheritance sequence

J.3.1.3.4 is split into heritage name/value validation, bounded cycle
detection, inherited member and constructor surfaces, override compatibility,
derived constructor `super` checks, and `super` member checks. Each step uses
the shared parsed class IR and pinned TypeScript 5.9.3 diagnostics while class
output remains refused. The first step accepts local and imported class bases,
checks an unknown or known non-constructor base, and checks declaration order;
cycle diagnostics remain in J.3.1.3.4.2.

### J.3.1.3.4.1 Named heritage binding

The checker now validates each parsed named `extends` reference after local
and imported class values have been bound. A bound class constructor is an
accepted base; a missing name, known non-constructor value, or local class
declared after its derived class gets a diagnostic on the original heritage
identifier. Opaque function, `any`, and unknown value bases remain deferred
because the current constructor model cannot prove their runtime base shape.
Direct and indirect cycle diagnostics follow in J.3.1.3.4.2. Class output is
still refused.

Public checked-compile coverage proves local and imported acceptance, three
rejected forms, original spans, and no artifact. Five pinned TypeScript 5.9.3
`--noEmit` cases agree on acceptance and TS2304/TS2507/TS2449 lines. BlueTS,
bridge, and full workspace suites, workspace all-target Clippy, rustfmt, and
whitespace checks pass with the reused 12 GiB Cargo target.

### J.3.1.3.4.2 Bounded local heritage cycles

After named class binding, the checker traverses each local class's parsed
`extends` edges with the configured type-expansion limit. It reports a direct
or indirect cycle on the original class name of every cycle member; a class
that merely depends on a cycle does not get a cycle diagnostic. The existing
forward-base check still reports its distinct heritage-name error. Longer
chains fail with a resource-limit diagnostic instead of unbounded traversal.
Imported base graphs remain outside this local cycle step, and class output
remains refused.

Public checked-compile tests cover a self cycle, mutual cycle with a
dependent class, exact cycle and forward-reference spans, a one-edge scan
budget, and no artifact. Two pinned TypeScript 5.9.3 `--noEmit` cases agree
on TS2506/TS2449 lines. BlueTS, bridge, and full workspace suites, workspace
all-target Clippy, rustfmt, and whitespace checks pass with the reused 12 GiB
Cargo target.

### J.3.1.3.4.3 Inherited class surfaces

The inherited lookup work is split into instance methods, static methods,
constructor signatures, and closed-module export/import propagation. Local
and value-imported named class bases must preserve derived method shadowing
and the existing bounded property and call relations. The checker continues
to refuse class output until the emit and runtime leaves close.

### J.3.1.3.4.3.1 Inherited instance methods

Before checking class bodies and runtime expressions, the checker builds each
local class's instance method shape from its own parsed method groups and
unshadowed ancestors. It follows local `extends` edges up to the configured
type-expansion limit and can terminate at a value-imported class's already
bound instance surface. The resulting record retains original method spans
and overload groups. Derived `this` and `new` instance values reach those
methods through the existing bounded property lookup, call relation, and
selected-result inference. Class output remains refused.

Public checked-compile cases cover a multi-level local base, an imported
base, an invalid inherited call, an incompatible inferred result, exact
spans, and no artifact. Four pinned TypeScript 5.9.3 `--noEmit` cases agree
on acceptance and TS2345/TS2322 lines. BlueTS, bridge, and full workspace
suites, workspace all-target Clippy, rustfmt, and whitespace checks pass with
the reused 12 GiB Cargo target.

### J.3.1.3.4.3.2 Inherited static methods

Each local class's constructor-side record now retains its own `prototype`
and static methods, then appends unshadowed static methods from local or
value-imported named bases within the type-expansion limit. Static method
bodies bind `this` to that merged record. Class values and static `this`
reach inherited overload groups through the existing bounded call selector,
and the selected return type flows into local and declared return checks.
Class output remains refused.

Public checked-compile cases cover multi-level and imported bases, accepted
static overload calls, bad arguments, incompatible inferred results, exact
spans, and no artifact. Four pinned TypeScript 5.9.3 `--noEmit` cases agree
on acceptance and TS2769/TS2322 lines. BlueTS, bridge, and full workspace
suites, workspace all-target Clippy, rustfmt, and whitespace checks pass with
the reused 12 GiB Cargo target.

### J.3.1.3.4.3.3 Inherited constructor signatures

After class binding, a local derived class without a constructor copies the
resolved constructor overload signatures of its local or value-imported base.
The copied signatures return the derived instance type and feed the existing
bounded argument and arity selector at the original `new` call span. A
derived class that declares its own constructor retains that signature.
An unresolved base leaves constructor selection deferred behind the heritage
diagnostic and class-output refusal; checking `super` in constructor bodies
follows in J.3.1.3.4.5.

Public checked-compile cases cover a multi-level local base, an imported
base, bad arguments, missing arguments, incompatible inferred instance shape,
explicit constructor precedence, exact spans, and no artifact. Six pinned
TypeScript 5.9.3 `--noEmit` cases agree on acceptance and TS2769/TS2554/
TS2741/TS2345 lines. BlueTS, bridge, and full workspace suites, workspace
all-target Clippy, rustfmt, and whitespace checks pass with the reused 12 GiB
Cargo target.

### J.3.1.3.4.3.4 Closed-module inherited class surfaces

The closed-module propagation is split into local-base derived class exports
and derived classes whose bases are imported. The first step publishes the
already checked local inheritance shape for direct and locally aliased value
exports; the second carries that shape across imported bases and re-exports.
Both keep class output refused until the shared emit and runtime paths close.

J.3.1.3.4.3.4.2 is further split into direct value export/import propagation
for a value-imported base and the supported `export type` re-export chain.
The parser currently refuses runtime `export { value } from` syntax, so this
phase's re-export checker gate covers the type-only form it can represent.

### J.3.1.3.4.3.4.1 Local-base derived class exports

The closed-module class export table now walks locally declared classes in
source order and extends each derived export with unshadowed instance and
static members from its resolved local base. An omitted derived constructor
inherits the base overloads with the derived return type. The traversal is
limited by the configured type-expansion depth; deeper or invalid heritage
still has its checker diagnostic and class output refusal. Direct and local
aliased value exports carry the same surface, and an importing name is
specialized by the existing class import binder.

Public checked-compile cases cover both export forms, accepted construction
and method calls, rejected constructor/instance/static arguments, an
incompatible selected result, exact spans, and no artifact. Six pinned
TypeScript 5.9.3 `--noEmit` cases agree on acceptance and TS2345/TS2322
lines. BlueTS, bridge, and full workspace suites, workspace all-target
Clippy, rustfmt, and whitespace checks pass with the reused 12 GiB Cargo
target.

### J.3.1.3.4.3.4.2.1 Value-imported base class exports

The closed-module class export table now resolves value-imported base
surfaces through the existing project graph and advances derived surfaces
one module edge per pass. It merges inherited instance and static members
and omitted-constructor overloads into direct and locally aliased class
exports. Each surface carries its heritage depth, so the configured type
expansion bound also applies across module edges. Type-only imports cannot
provide a runtime base.

Public checked-compile cases cover accepted direct and aliased imports,
a four-module inheritance chain, rejected constructor/instance/static
arguments, an incompatible selected result, original diagnostic spans,
and no artifact. Seven pinned TypeScript 5.9.3 `--noEmit` cases agree on
acceptance and TS2345/TS2322 lines. BlueTS, bridge, and full workspace
tests, workspace all-target Clippy, rustfmt, and whitespace checks pass
with the reused 12 GiB Cargo target.

### J.3.1.3.4.3.4.2.2 Type-only inherited class re-export chains

The existing class surface and type-only re-export fixed points already
carry an imported-base derived class's inherited instance members through
named aliases and star re-exports. A type-only import binds the resolved
instance surface without publishing a runtime class value. The five-module
public checked-compile regression verifies accepted inherited member calls,
rejected arguments and selected results at original spans, and no output.
Three pinned TypeScript 5.9.3 `--noEmit` cases agree on acceptance and
TS2345/TS2322 lines. BlueTS, bridge, and full workspace tests, workspace
all-target Clippy, rustfmt, and whitespace checks pass with the reused
12 GiB Cargo target.

### J.3.1.3.4.4 Inherited method override sequence

Override checks are split into direct local-base methods with one signature,
deeper local and value-imported heritage with bounded lookup and optional,
rest, or differing-arity parameters, and inherited overload sets. Each step
checks instance and static sides at original member spans against pinned
TypeScript while class output remains refused.

### J.3.1.3.4.4.1 Direct local-base method overrides

A separate class override checker now compares explicitly annotated,
single-signature methods of a direct local base and derived class when their
parameters have equal required arity. It matches instance and static sides
separately, checks parameter types in either direction as TypeScript does for
class methods, and requires the derived return type to fit the inherited
return type. The existing type-expansion limit bounds the comparison. Wider
arity, optional/rest parameters, deeper or imported bases, and overload sets
remain in the following override leaves.

Public checked-compile cases cover accepted exact and narrower parameter
overrides, rejected instance/static parameter and result types at original
member spans, and no artifact. Five pinned TypeScript 5.9.3 `--noEmit`
cases agree on acceptance and TS2416/TS2417 lines. BlueTS, bridge, and full
workspace tests, workspace all-target Clippy, rustfmt, and whitespace
checks pass with the reused 12 GiB Cargo target.

J.3.1.3.4.4.2 is further split into nearest-method lookup along bounded local
heritage, inherited surfaces from value-imported bases, and optional/rest or
differing-arity parameter compatibility.

### J.3.1.3.4.4.2.1 Deeper local method overrides

The override checker now follows local named `extends` edges up to the
configured type-expansion limit and compares a derived method with the
nearest inherited method on the same instance or static side. An intervening
overload group hides older ancestors and remains for the overload-set leaf.
Existing heritage validation diagnoses cycles and over-limit chains, while
class output remains refused.

Public checked-compile cases cover accepted instance/static overrides
through a middle class, rejected parameter and return types, nearest-method
shadowing, exact original member spans, and no artifact. Six pinned
TypeScript 5.9.3 `--noEmit` cases agree on acceptance and TS2416/TS2417
lines. BlueTS, bridge, and full workspace tests, workspace all-target
Clippy, rustfmt, and whitespace checks pass with the reused 12 GiB Cargo
target.

### J.3.1.3.4.4.2.2 Value-imported method overrides

When a local override walk reaches a value-imported class, the checker now
reads the already bound instance or constructor-side record and compares its
single method signature with the derived method. This covers direct imports,
local import aliases, and class exports whose own base was imported in an
earlier module. The same bounded local traversal and method-side separation
apply; overload groups and optional/rest or differing arities remain in
their dedicated leaves.

Public checked-compile cases cover two accepted direct/transitive forms,
eight rejected parameter/result/arity forms, original member spans, and no
artifact. Ten pinned TypeScript 5.9.3 `--noEmit` cases agree on acceptance
and TS2416/TS2417 lines. BlueTS, bridge, and full workspace tests,
workspace all-target Clippy, rustfmt, and whitespace checks pass with the
reused 12 GiB Cargo target.

J.3.1.3.4.4.2.3 is split into optional/default and differing required
arities, then rest arrays/tuples and fixed/rest interactions. The first step
keeps the existing single-signature, explicitly annotated method boundary.

### J.3.1.3.4.4.2.3.1 Optional and differing-arity overrides

For explicitly annotated single-signature methods without rest parameters,
the override checker now allows the derived method to omit inherited
parameters or add omittable optional/default parameters. It rejects a
derived method that requires more positions than the base declares and
compares overlapping parameter types bivariantly regardless of optional
markers, following the pinned TypeScript method relation. Return types
remain covariant and the comparison uses the existing expansion budget.

Public checked-compile cases cover three accepted local forms, four
rejected local forms, a rejected imported-base arity, original member spans,
and no artifact. Seven local cases and the imported companion agree with
pinned TypeScript 5.9.3 `--noEmit` acceptance and TS2416/TS2417 lines.
BlueTS, bridge, and full workspace tests, workspace all-target Clippy,
rustfmt, and whitespace checks pass with the reused 12 GiB Cargo target.

J.3.1.3.4.4.2.3.2 is further split into matching-prefix array rest,
fixed/rest interactions, and tuple rest expansion. The first step uses the
already parsed array rest annotation and keeps tuple work separate from the
current array-only class parameter gate.

### J.3.1.3.4.4.2.3.2.1 Matching-prefix array rest overrides

The bounded class override comparison now handles a final array rest on
both inherited and derived methods when their fixed parameter prefixes have
the same length. It excludes rest from the required arity count, compares
array element types through the existing bounded type relation, and still
checks fixed parameters and covariant return types. Different fixed
prefixes, one-sided rest, and tuple rest remain in their following leaves.

Public checked-compile cases cover accepted instance/static overrides,
five rejected element, prefix, and result forms at original member spans,
and no artifact. Six pinned TypeScript 5.9.3 `--noEmit` cases agree on
acceptance and TS2416/TS2417 lines. BlueTS, bridge, and full workspace
tests, workspace all-target Clippy, rustfmt, and whitespace checks pass with
the reused 12 GiB Cargo target.

J.3.1.3.4.4.2.3.2.2 is split by direction: a derived array rest against
remaining fixed base parameters, a fixed derived signature against an
inherited array rest, and differing fixed prefixes when both sides have
array rest. Each comparison remains at the parsed method boundary.

### J.3.1.3.4.4.2.3.2.2.1 Derived array rest over fixed base parameters

The override comparison now aligns a derived method's fixed prefix with the
base's fixed parameters and then compares every remaining inherited fixed
parameter with the derived array rest element type. The same bounded
bivariant type relation and covariant return check apply on instance and
static sides. A base rest or a longer derived fixed prefix remains in the
following rest leaves.

Public checked-compile cases cover accepted instance/static methods, three
rejected element mismatches including a later fixed base position, exact
original member spans, and no artifact. Four pinned TypeScript 5.9.3
`--noEmit` cases agree on acceptance and TS2416/TS2417 lines. BlueTS,
bridge, and full workspace tests, workspace all-target Clippy, rustfmt, and
whitespace checks pass with the reused 12 GiB Cargo target.

### J.3.1.3.4.4.2.3.2.2.2 Fixed derived positions over base array rest

The override checker now aligns each fixed derived parameter with the
inherited fixed prefix or, once that prefix ends, the base's array rest
element type. It accepts a shorter derived signature, an optional position,
and one required position at the base rest slot when their types fit;
further required positions and type mismatches still fail under the bounded
method relation. Both instance and static sides use the original derived
member span. Differing fixed prefixes with rest on both sides remain in the
next leaf.

Public checked-compile cases cover two accepted source forms, four rejected
arity or instance/static type forms including a later position, and no
artifact. Six pinned TypeScript 5.9.3 `--noEmit` cases agree on acceptance and
TS2416/TS2417 lines. BlueTS, bridge, and full workspace tests, workspace
all-target Clippy, rustfmt, and whitespace checks pass with the reused
12 GiB Cargo target.

### J.3.1.3.4.4.2.3.2.2.3 Both-side array rests with shifted prefixes

The bounded class override relation now aligns each fixed parameter with
the other method's fixed parameter or array rest element, then compares the
two rest element types. TypeScript accepts a derived method with additional
required fixed parameters when both signatures end in array rest and those
types fit, so this shape does not use the single-rest required-arity limit.
The instance and static comparisons retain the original derived member span.
Tuple rest expansion remains in the next leaf.

Public checked-compile cases cover two accepted forms, including the longer
required prefix, and three rejected prefix, element, and static forms with
exact original spans and no artifact. Five pinned TypeScript 5.9.3 `--noEmit`
cases agree on acceptance and TS2416/TS2417 lines. BlueTS, bridge, and full
workspace tests, workspace all-target Clippy, rustfmt, and whitespace checks
pass with the reused 12 GiB Cargo target.

### J.3.1.3.4.4.2.3.2.3 Tuple rest expansion

Tuple rest support is split into fixed-length annotation admission,
fixed-length override comparison, and optional/labeled/variadic tuple
syntax. The fixed-length comparison is further divided by the direction
of the tuple rest and a both-side case. This keeps parser, validation,
and override behavior individually reviewable against pinned TypeScript.

### J.3.1.3.4.4.2.3.2.3.1 Fixed-length tuple rest annotations

Class constructor, instance, and static method parameters now accept a
parsed fixed-length tuple as a rest annotation. The shared parameter
validator still rejects a primitive rest annotation and checks each tuple
element type at the original parameter span. This admission does not yet
expand tuple elements for inherited override comparison; class output
remains refused.

Public checked-compile coverage accepts all three class parameter sites,
rejects a primitive rest and unknown tuple element at their original spans,
and confirms no artifact. Three pinned TypeScript 5.9.3 `--noEmit` cases
agree on acceptance and TS2370/TS2304 lines. BlueTS, bridge, and full
workspace tests, workspace all-target Clippy, rustfmt, and whitespace checks
pass with the reused 12 GiB Cargo target.

### J.3.1.3.4.4.2.3.2.3.2.1 Derived fixed-length tuple rest overrides

The bounded override relation now expands each required element of a
derived method's fixed-length tuple rest into a positional parameter. It
compares those positions with inherited fixed parameters or the inherited
array rest element through the existing bivariant type relation, while
retaining the covariant result check. A fixed inherited signature rejects
additional required tuple positions; an inherited array rest can accept
them when the element types agree. Unsupported inherited tuple shapes
remain for the following leaves, and class output stays refused.

Public checked-compile coverage accepts instance and static overrides,
rejects fixed-position type, fixed arity, and later array-rest element
mismatches at original member spans, and confirms no artifact. Four pinned
TypeScript 5.9.3 `--noEmit` cases agree on acceptance and TS2416/TS2417
lines. BlueTS, bridge, and full workspace tests, workspace all-target
Clippy, rustfmt, and whitespace checks pass with the reused 12 GiB target.

### J.3.1.3.4.4.2.3.2.3.2.2 Inherited fixed-length tuple rest overrides

The bounded override relation now treats each inherited tuple rest element
as a required positional parameter. A derived fixed signature compares
only its supplied positions and cannot require more than the expanded base
arity. A derived array rest is compared with every remaining inherited
tuple element. Both paths use the existing bivariant parameter relation,
covariant result check, and original derived member span. Both-side tuple
rest compatibility remains in the next leaf, and class output stays refused.

Public checked-compile coverage accepts a shorter instance signature and
a static array rest against a homogeneous tuple, rejects fixed-position
type, fixed arity, and later array element mismatches, and confirms no
artifact. Four pinned TypeScript 5.9.3 `--noEmit` cases agree on acceptance
and TS2416/TS2417 lines. The parser currently treats a parenthesized union
array annotation as a function type, so the accepted array case uses a
homogeneous tuple; that syntax limitation is outside this override leaf.
BlueTS, bridge, and full workspace tests, workspace all-target Clippy,
rustfmt, and whitespace checks pass with the reused 12 GiB target.

### J.3.1.3.4.4.2.3.2.3.2.3 Both-side fixed-length tuple rests

The bounded method override relation now expands fixed-length tuple rests
on both sides and compares aligned positions through the existing
bivariant parameter relation. It checks the derived required count against
the inherited expanded length, allowing omitted inherited positions but
rejecting extra required tuple elements. Return types remain covariant and
diagnostics retain the original derived member span. Class output stays
refused.

Public checked-compile coverage accepts matching expanded signatures with
shifted fixed prefixes on instance and static sides plus a shorter derived
tuple. It rejects an instance element mismatch, excess required element,
and static later-element mismatch, with no artifact. Four pinned TypeScript
5.9.3 `--noEmit` cases agree on TS2416/TS2417 lines. BlueTS, bridge, and
full workspace tests, workspace all-target Clippy, rustfmt, and whitespace
checks pass with the reused 12 GiB target.

The remaining tuple syntax work is split into optional, labeled, and
variadic elements. Parenthesized union arrays are separately tracked for
heterogeneous tuple-to-array override comparisons because the current
type parser interprets a leading `(` as a function type.

### J.3.1.3.4.4.2.3.2.3.3.1 Optional tuple element rollout

The existing `Type::Tuple(Vec<Type>)` cannot distinguish an omittable
element from a required one. Replace its element vector with a shared
tuple-element record containing the annotation, an optional flag, an
optional label, and a rest flag. Labels will be retained for declaration
output but ignored by structural type compatibility. Optional and rest
flags will affect minimum/maximum arity, indexed element types, and
runtime contracts. This one representation will also serve the following
labeled and variadic leaves without introducing temporary tuple variants.

First migrate existing required tuples to records with all flags clear and
prove unchanged parser, checker, emitter, and contract behavior. Keep the
parser rejecting optional tuple syntax during this migration. Then admit
optional syntax only after its general type relation, declaration output,
and contract behavior have public tests; class method override expansion
follows as a separate leaf. This sequence prevents an optional tuple from
being silently treated as a required tuple in another BlueTS use.

### J.3.1.3.4.4.2.3.2.3.3.1.2 Required tuple element metadata migration

The public tuple type now stores an element record with annotation,
optional, label, and rest metadata. Parsed type annotations and inferred
tuple literals create required, unlabeled, non-rest elements. Type
substitution, generic inference, assignability, indexed reads, readonly
tracking, call spreads, declaration text, and runtime-contract lowering
read the element annotation. The element record is re-exported so hosts can
still construct a required tuple type through the public API. Its metadata
fields are crate-visible with public read accessors; external callers cannot
construct optional or rest elements before their semantics are installed.
Optional tuple syntax remains rejected by the parser until the next leaf
closes its semantics.

Existing public BlueTS/BlueJS tests, exact-length tuple contract tests,
class tuple-rest accepted/rejected tests, and emitter regressions pass.
Full workspace tests, workspace all-target Clippy, rustfmt, and whitespace
checks pass with the reused 12 GiB Cargo target.

### J.3.1.3.4.4.2.3.2.3.3.1.3 Optional tuple types outside overrides

The tuple type parser now accepts trailing `?` elements and diagnoses a
required element after an optional one at its source span. Tuple
assignability compares the required and maximum lengths and each present
element against its expected type, allowing `undefined` at optional
positions. Indexed reads include `undefined` for an optional position.
Type labels, identities, and declarations retain the optional marker;
JavaScript emission still erases it.

Pure contracts lower an optional tuple to a bounded-length plan, validate
each present element, and accept explicit `undefined` at an optional slot.
Optional tuple spreads are rejected by the current bounded call and tuple
literal expansion paths pending a separate step. Class override comparison
skips optional tuple rest shapes until the next leaf; class output remains
refused. The parser still rejects `undefined` as a binding name, so a
shadowed `undefined` cannot be tested through admitted BlueTS source here;
simple inference consults the local scope before using the intrinsic type.
Public checked-compile cases cover accepted declarations and
indexing, wrong type and invalid element order, emitted declarations, and
no error artifact. Contract cases cover minimum and maximum lengths,
present types, and explicit `undefined`. Five pinned TypeScript 5.9.3
`--noEmit` cases agree on acceptance and TS2322/TS1257 lines. BlueTS,
bridge, and full workspace tests, workspace all-target Clippy, rustfmt, and
whitespace checks pass with the reused 12 GiB target.

### J.3.1.3.4.4.2.3.2.3.3.1.4 Optional tuple rest overrides

Class override checking now admits rest annotations with optional tuple
elements. A derived tuple contributes only its required elements to the
minimum parameter count, while every declared element of an inherited tuple
counts toward its maximum accepted position. At an optional position, compare
the element's value type, including `undefined`; the existing bivariant
method-parameter relation still applies. Variadic tuple tails remain outside
this relation until J.3.1.3.4.4.2.3.2.3.3.3.

Public checked-compile cases cover accepted fixed, shorter tuple, optional
derived, and shifted static overrides, plus instance/static element mismatches
and an excess required argument. Four pinned TypeScript 5.9.3 `--noEmit`
cases agree on acceptance and the TS2416/TS2417 diagnostic lines. The public
front end reports the rejected member span and emits no artifact.

### J.3.1.3.4.4.2.3.2.3.3.2.1 Labeled tuple syntax and type erasure

The parser recognizes `name: T` and `name?: T` tuple positions and stores
their labels in the shared tuple-element metadata. Labels do not participate
in tuple assignability or type identity; positions and optionality do. The
declaration emitter reproduces labels and places the optional marker before
the colon, while JavaScript emission erases both. TypeScript 5.9.3 also
accepts a tuple mixing labeled and unlabeled positions, so BlueTS admits it.

Public checked-compile cases cover declaration output, assignment across
different labels, a partly labeled tuple, and a wrong element type without an
artifact. Three pinned TypeScript 5.9.3 `--noEmit` cases agree on acceptance
and the TS2322 line. Labeled rest positions in class overrides are checked
in the next leaf using the existing positional relation.

### J.3.1.3.4.4.2.3.2.3.3.2.2 Labeled tuple rest overrides

The class override relation already compares tuple rest elements by declared
position and optionality, without reading the label metadata. Instance and
static overrides may rename those labels; incompatible element types or too
many required positions still fail. Four pinned TypeScript 5.9.3 `--noEmit`
cases agree on valid renamed positions and TS2416/TS2417 diagnostic lines.
Public checked-compile cases assert the rejected member spans, only the
expected mismatch diagnostic, and no output artifact. No checker change is
needed for this leaf; the regressions lock in positional semantics.

### J.3.1.3.4.4.2.3.2.3.3.3.1 Variadic tuple rollout

The shared `TupleTypeElement` already has a rest flag but the parser does not
set it. Admission will start with one array-typed rest element at the end of
a tuple, then extend to a rest with a required suffix, and finally to
concrete or constrained generic tuple spreads. A rest is a symbolic tail:
its array element type applies to arbitrarily many positions. It must not
be expanded into an unbounded vector. The existing type-expansion budget
continues to limit recursive type work; comparisons inspect each finite
prefix and suffix once plus a tail relation.

The parser will reject multiple rests and an optional element after a rest,
matching TypeScript's tuple restrictions. It will keep `...T[]` and
`...name: T[]` forms distinct only for declaration spelling; labels remain
erased from type identity. Trailing rest admission must update tuple
assignability, contextual tuple literals, indexed reads, type display,
declaration output, and pure contract validation together. Runtime contracts
will validate the finite prefix and each present tail element under their
existing collection/node limits. Class override comparison remains
fail-closed for rest-bearing tuples until a separate leaf installs its
symbolic arity and element relation. A middle rest additionally needs a
fixed suffix matched from the end, so it follows the trailing case. Concrete
and constrained generic tuple spreads need bounded specialization before
they can be admitted; unresolved spreads remain diagnostics.

TypeScript's official tuple guidance documents open-ended trailing elements
and leading/middle rest positions and the one-rest/no-optional-after-rest
restrictions: https://www.typescriptlang.org/docs/handbook/release-notes/typescript-4-2.html.
The pinned 5.9.3 oracle will determine acceptance and diagnostic lines for
each implementation leaf. This design-only leaf admits no new syntax and
changes no runtime behavior.

### J.3.1.3.4.4.2.3.2.3.3.3.2 Trailing array rest tuple semantics

Tuple parsing now admits a single trailing `...T[]` or `...name: T[]`
element and retains its rest flag and optional label. An optional prefix may
precede the rest. Another element after the rest is still rejected: an
optional one is a parse error, and a required suffix stays unsupported until
the nontrailing leaf. Non-array rest annotations remain unsupported.

Tuple assignability compares finite declared positions and at most one
symbolic repeated tail relation. A fixed tuple can fit a compatible rest
tuple, and `[...T[]]` interchanges with `T[]` where its element types fit.
Indexed reads beyond the finite prefix use the tail element type; type
identity, display, and declarations retain `...`, while JavaScript erases
the type. Pure contracts lower the finite prefix plus one tail plan and
validate every present item using the existing collection and node budgets.
The launcher still reports this contract as a tuple. Tuple spreads in calls
and tuple literals remain fail-closed, and class override comparison still
skips rest-bearing tuples until the next leaf.

Public checked-compile and pure contract cases cover named, required and
optional prefixes; empty and repeated tails; tuple/array assignments;
tail indexed reads; wrong element types; missing required positions; a
forbidden optional after the rest; and no error artifact. Five pinned
TypeScript 5.9.3 `--noEmit` cases agree on acceptance and TS2322/TS1266
lines.

### J.3.1.3.4.4.2.3.2.3.3.3.3.1 Derived trailing tuple rest overrides

The override checker now recognizes a derived rest parameter whose tuple
ends in an array-typed rest element when the inherited method has fixed
parameters or an ordinary array rest. It aligns the finite fixed positions,
uses the repeated tuple tail type for every remaining inherited position,
and compares the repeated tail once against an inherited array rest element.
Only nonoptional, nonrest tuple positions contribute to derived minimum
arity. TypeScript permits extra required derived positions when the base has
an array rest and all aligned types fit. The existing bivariant method
parameter and covariant result relations still apply.

Inherited or both-side variadic tuple rests remain fail-closed until their
separate leaves. Public checked-compile cases cover accepted fixed and array
forms, including extra required derived prefix positions, and reject a
fixed element mismatch, excess required arity against a fixed base, and a
static array-tail mismatch at original member spans without output. Four
pinned TypeScript 5.9.3 `--noEmit` cases agree on acceptance and TS2416/
TS2417 diagnostic lines.

### J.3.1.3.4.4.2.3.2.3.3.3.3.2 Inherited trailing tuple rest overrides

The override checker now recognizes an inherited rest parameter whose tuple
ends in an array-typed rest element when the derived method has fixed
parameters or an ordinary array rest. It aligns each supplied derived fixed
position with an inherited fixed or tuple position and uses the repeated
tail type beyond that prefix. An ordinary derived array rest is also compared
once with the inherited tail after finite positions are aligned. The
inherited tuple rest has no maximum arity, so additional compatible required
derived positions are accepted; the existing bivariant parameter and
covariant result checks remain in force.

Both-side tuple rest shapes remain fail-closed until the next leaf. Public
checked-compile cases cover accepted fixed and array forms, including a
zero-prefix inherited rest with one required derived position, and reject
incompatible instance and static element types at the original member spans
without output. Four pinned TypeScript 5.9.3 `--noEmit` cases agree on
acceptance and TS2416/TS2417 lines.

### J.3.1.3.4.4.2.3.2.3.3.3.3.3 Both-side trailing tuple rest overrides

The override checker now admits trailing array-typed tuple rests on both
sides. It aligns each finite fixed or tuple-prefix position with the other
side's position or repeated tail, then compares the two repeated tail types
once. Both signatures have unbounded maximum arity, so required prefixes do
not impose a fixed upper arity cap; their aligned types must still agree
under the existing bivariant method-parameter relation, and the result
remains covariant. Prefixes may be shifted by ordinary fixed parameters.

Public checked-compile cases cover accepted instance and shifted static
forms, plus rejected tail and static prefix element mismatches at original
member spans with no output. Three pinned TypeScript 5.9.3 `--noEmit` cases
agree on acceptance and TS2416/TS2417 lines. Nontrailing tuple rests remain
outside this relation until their separate leaves.

### J.3.1.3.4.4.2.3.2.3.3.3.4 Nontrailing array rest tuple semantics

Tuple parsing now admits one array-typed rest element followed by required
suffix elements. A second rest and an optional element after a rest are
parse errors. An optional prefix before a required suffix also remains a
parse error, matching the pinned TypeScript relation. Labels and `...` stay
in declarations and type displays, while JavaScript erases them.

Tuple assignment with a middle rest checks finite witness lengths from the
minimum admitted length through one beyond the combined declared shapes.
That range includes every boundary where a prefix, repeated middle, or
end-aligned suffix can change position. Each comparison consumes the shared
type-expansion budget; no runtime-length vector is built. A fixed tuple can
fit the required prefix and suffix, and two middle-rest tuples compare by
position regardless of label spelling. A merely trailing rest cannot promise
a required suffix. Existing trailing-only relations remain unchanged.

At a fixed prefix index, tuple reads retain the exact declared type. At an
index that can hold the middle or a suffix position, reads combine the
repeated element with every suffix element that can occupy that index.
The pure `RestTuple` contract now records a required suffix separately,
checks minimum length, validates the finite prefix, each present middle
item, and the suffix from the end under existing collection and node limits.
Class override comparison still skips nontrailing tuple rest shapes until
its separate leaf; tuple spreads in calls and literals remain fail-closed.

Public checked-compile and pure contract cases cover leading and middle
rests, empty and repeated middles, fixed/middle/trailing assignments, exact
and uncertain indexed reads, declaration output, missing suffixes, wrong
middle or suffix types, a second rest, and optional-before-required order.
Ten pinned TypeScript 5.9.3 `--noEmit` cases agree on acceptance and TS2322,
TS1265, and TS1257 diagnostic lines.

### J.3.1.3.4.4.2.3.2.3.3.3.5.1 Derived nontrailing tuple rest overrides

The class override gate now recognizes a final rest parameter whose tuple
contains one array-typed rest before a required suffix when the inherited
method has fixed parameters or an array rest. Other nontrailing override
directions remain separate leaves. This follows the documented TypeScript
use of nontrailing rests to express variable arguments followed by fixed
parameters.

For a fixed inherited signature, the comparison checks every arity from its
required count through its declared count. The derived tuple must accept the
minimum inherited arity. For an inherited array rest, it checks bounded
witness lengths past both fixed prefixes and the derived suffix boundary;
matching positions use the existing bivariant method parameter relation.
Tuple positions are calculated from the total witness length, so the suffix
always aligns from the end, while the middle element repeats symbolically.
Each positional comparison consumes the shared type-expansion budget.

Public checked-compile cases cover accepted leading, middle, and shifted
rests, rejected middle/suffix element types, too many required arguments,
an optional inherited argument that cannot supply the required suffix, and
a static mismatch. The original method member receives one type diagnostic
and no output is emitted. Six pinned TypeScript 5.9.3 `--noEmit` cases agree
on acceptance and TS2416/TS2417 diagnostic lines.

### J.3.1.3.4.4.2.3.2.3.3.3.5.2 Inherited nontrailing tuple rest overrides

The inherited-side gate now recognizes a final tuple rest parameter with an
array-typed middle and required suffix. A derived fixed signature selects
each arity it admits, then aligns inherited tuple positions from the total
arity, keeping the suffix anchored at the end. The fixed signature must
supply at least the inherited minimum required positions; a longer fixed
signature can match a longer middle segment when its positions fit.

A derived array rest compares finite witness lengths through the inherited
prefix, middle, and suffix boundaries. Its declared fixed prefix cannot move
past the inherited fixed prefix (ordinary parameters plus fixed tuple
elements), matching pinned TypeScript behavior for a shifted array rest.
The remaining tail is compared through the existing bivariant method
parameter relation. Each checked position consumes the type-expansion
budget, and return covariance remains unchanged.

Public checked-compile cases cover short and longer fixed overrides,
leading and shifted tuple forms, homogeneous array rests, and static
methods. Rejections cover a wrong suffix, wrong middle position, omitted
required suffix, an optional derived position, incompatible array tail, and
a shifted array prefix. Each reports the original member span with no output.
Eight pinned TypeScript 5.9.3 `--noEmit` cases agree on acceptance and
TS2416/TS2417 diagnostic lines. Both-side middle-rest comparison remains
the next separate leaf.

### J.3.1.3.4.4.2.3.2.3.3.3.5.3.1 Both-side variable tuple rest overrides

When both class method rest parameters are variable tuples and at least one
has a nontrailing array rest, the checker compares common witness lengths
from the larger minimum arity through one beyond the combined declared
shapes. Fixed method parameters, fixed tuple positions, repeated middle or
trailing positions, and suffix positions are all selected from the total
witness length. Class-method parameter bivariance applies at each position;
each comparison consumes the shared type-expansion budget. This accepts
compatible shifted prefixes and suffixes without assuming that either rest
has a fixed expansion length.

The first subleaf admits required-only fixed positions on both sides, which
include mixed middle/trailing pairs. Tuple labels remain irrelevant to type
comparison. Optional fixed positions and opposing fixed tuple rests remain
separate leaves because their arity sets differ. Public checked-compile cases
cover valid renamed labels, prefix/suffix shifts, both mixed trailing
directions, and static methods, plus incompatible suffix, middle, shift,
trailing, and static types at original member spans with no output. Six
pinned TypeScript 5.9.3 `--noEmit` cases agree on acceptance and TS2416/
TS2417 diagnostic lines.

### J.3.1.3.4.4.2.3.2.3.3.3.5.3.2 Middle and fixed tuple rest overrides

When one class method rest parameter has a nontrailing array rest and the
opposing rest parameter has a required-only fixed tuple, the fixed tuple's
total parameter count chooses a single witness length. The variable tuple
must fit that length in the derived-middle direction; the derived fixed
tuple must provide at least the inherited middle tuple's required count in
the opposite direction. Prefix, repeated middle, and suffix positions are
then compared at the chosen length with the existing bivariant method
parameter relation. Each position consumes the type-expansion budget.

This subleaf covers required-only tuple positions and ordinary fixed
parameters. Public checked-compile cases cover short and longer fixed
tuples, both inheritance directions, and static methods, plus incompatible
suffix or middle types and insufficient required arity at original member
spans with no output. Eight pinned TypeScript 5.9.3 `--noEmit` cases agree
on acceptance and TS2416/TS2417 diagnostic lines. Optional fixed positions
are handled in separate follow-up leaves.

### J.3.1.3.4.4.2.3.2.3.3.3.5.3.3.1 Optional trailing tuple prefixes

The both-side variable tuple rest relation now admits optional fixed tuple
elements before a trailing array rest when the opposite tuple has a
nontrailing rest. Its bounded common witness lengths begin at the larger
minimum required count, so the optional prefix does not create a required
argument. Existing tuple position selection retains the optional element's
`undefined` alternative and checks the repeated tail beyond the prefix.
Method parameters remain bivariant at each budgeted position.

Public checked-compile cases cover a homogeneous optional prefix in both
inheritance directions, two optional prefix elements, and a static method.
Heterogeneous optional or shifted prefix positions report one mismatch at
the original member span with no output. Five pinned TypeScript 5.9.3
`--noEmit` cases agree on acceptance and TS2416/TS2417 diagnostic lines.
Optional positions in a fixed tuple rest and ordinary method parameters
remain separate leaves.

### J.3.1.3.4.4.2.3.2.3.3.3.5.3.3.2 Optional fixed tuples against middle rests

A fixed tuple rest with optional trailing elements is a distinct override
shape from a required-only fixed tuple. When it faces a middle rest, the
checker now emits an incompatibility at the overriding member in either
direction. An inherited optional fixed tuple may terminate before the
middle rest's required suffix. A derived optional fixed tuple has a bounded
maximum and cannot match the inherited middle rest's unbounded form. The
gate keeps the required-only fixed tuple relation separate, so it does not
invent a required optional position to select a witness length.

Public checked-compile cases cover both directions, a longer fixed tuple,
and a static method with one mismatch at the original member span and no
output. Four pinned TypeScript 5.9.3 `--noEmit` cases agree on TS2416/TS2417
diagnostic lines. Optional ordinary method parameters remain the next leaf.

### J.3.1.3.4.4.2.3.2.3.3.3.5.3.3.3.1 Optional ordinary prefixes with variable tuples

For two variable tuple rest parameters with at least one nontrailing rest,
ordinary optional parameters now remain in their declared prefix positions.
The bounded witness relation starts at the larger of each side's ordinary
prefix length plus its required tuple positions. A required suffix therefore
retains the ordinary prefix slot even when that parameter's annotation is
optional. At a fixed prefix position, an optional annotation also admits
`undefined` for the existing bivariant method parameter comparison. This
matches TypeScript's accepted interchange of an optional `number` prefix
with a required `number` or `undefined` prefix before the same tuple rest.

Public checked-compile cases cover both optional/required directions, a
tuple prefix opposed by an ordinary optional prefix, mixed middle/trailing
rests, and static methods. Wrong prefix, suffix, or shifted types report a
single mismatch at the original member span with no output. Five pinned
TypeScript 5.9.3 `--noEmit` cases agree on acceptance and TS2416/TS2417
diagnostic lines. Optional ordinary prefixes against fixed or array rest
signatures and fixed tuple rests remain separate leaves.

### J.3.1.3.4.4.2.3.2.3.3.3.5.3.3.3.2 Optional prefixes with fixed or array rest counterparts

The derived-middle and inherited-middle override relations now compare an
ordinary optional parameter as its annotation or `undefined` before fixed
or array rest counterparts. The tuple's required suffix still needs a
separate position after every declared ordinary prefix slot. The arity gate
therefore counts all ordinary prefix slots plus required tuple elements,
even when a prefix annotation is optional. A short fixed method cannot
silently occupy the suffix slot with that optional prefix. Array-rest
counterparts retain their unbounded arity comparison at bounded witness
lengths, starting after the derived middle tuple's minimum length.

Public checked-compile cases cover accepted optional/`undefined` prefixes
in derived and inherited fixed/array directions. Wrong prefix types, a
fixed base with too few required positions, an optional fixed base, a
fixed derived method that omits the required suffix, and a static mismatch
report one type diagnostic at the original member span with no output.
Nine pinned TypeScript 5.9.3 `--noEmit` cases agree on acceptance and
TS2416/TS2417 diagnostic lines. Opposing fixed tuple rests with ordinary
optional prefixes remain the next leaf.

### J.3.1.3.4.4.2.3.2.3.3.3.5.3.3.3.3 Optional ordinary prefixes with fixed tuple rests

Middle tuple rest overrides now compare an ordinary optional prefix with a
fixed tuple rest counterpart using the annotation or `undefined`. The arity
gate counts all declared ordinary prefix slots when the fixed tuple has
required elements, so a required suffix cannot occupy an optional prefix's
position. With an empty fixed tuple, the gate counts only required ordinary
prefixes, allowing the optional prefix to be omitted.

Public checked-compile cases cover accepted instance and static overrides in
both directions, plus wrong prefix types and too-short empty-tuple cases. Each
rejection reports one type diagnostic at the original member span without
emitting output. Six pinned TypeScript 5.9.3 `--noEmit` cases agree on
acceptance and TS2416/TS2417 diagnostic lines. BlueTS, bridge, and full
workspace tests, all-target Clippy, rustfmt, and whitespace checks pass with
the reused 12 GiB target. Concrete and constrained generic tuple spreads are
the next leaf.

### J.3.1.3.4.4.2.3.2.3.3.3.6.1 Named tuple spread admission policy

A named tuple spread first resolves through the local or imported type
definition, completes declared generic defaults, substitutes concrete type
arguments, and expands within the existing type-expansion budget. A finite
tuple contributes its elements at the spread position, preserving optional
positions and labels for declarations. An array-typed spread contributes one
symbolic tail. A tuple containing an array tail keeps that tail and any
required suffix; neither parser nor checker constructs an unbounded element
vector. Expansion must also charge the resulting finite positions against a
bounded size, so a chain of aliases cannot evade the budget by multiplying
tuple elements.

The parser retains named spread syntax in the shared tuple element metadata;
the checker resolves it before positional assignment, indexing, contextual
tuple construction, pure contract planning, or class override comparison.
Declaration output retains the source spelling. A concrete expanded tuple
may use the existing tuple relations and runtime contract bounds. A generic
spread whose type parameter has an array or tuple constraint retains its
parameter identity during comparison: the constraint bounds possible
elements, but replacing the parameter with that constraint would accept
some invalid overrides. Equal symbolic parameters can be compared directly;
other relations require a bounded proof from their constraints. Generic
runtime contracts remain unreifiable until a concrete instantiation is
available.

An unconstrained parameter, unknown name, cyclic expansion, excessive
expansion, or shape requiring multiple variable tails produces a diagnostic
before artifact output. No such case is silently widened to `unknown` or an
array. Existing array-only tuple spreads remain admitted while this work is
staged. Local pinned TypeScript 5.9.3 `--noEmit` probes accept a concrete
`[boolean, ...Pair]`, a concrete `Prefix<[string, boolean]>`, and a matching
`Base<T extends string[]>` override. They report TS2322 for a missing
concrete position, TS2574 for an unconstrained spread, and TS2416 for
incompatible concrete and constrained-generic overrides. This decision
changes no parser or checker behavior; the following leaves implement each
surface with public-boundary regressions.

### J.3.1.3.4.4.2.3.2.3.3.3.6.2 Concrete named tuple spreads

The parser retains a named tuple rest element for checking. The checker now
expands a concrete local alias to a finite tuple before assignment, contextual
tuple literal comparison, and indexed reads. Each alias and emitted position
charges the existing type-expansion budget. Nested aliases and an array alias
resolve recursively, while cycles, unresolved names, and non-tuple or
non-array targets report diagnostics before output. The declaration emitter
retains the original named spread syntax.

The pure contract lowerer expands the same concrete shape with a separate
1,024-position bound and validates every resulting position. A spread tuple's
optional element becomes required when a later suffix element is required;
the widened element type still includes `undefined`. This keeps short tuple
assignments and short runtime values from skipping the suffix. Class method
rest parameters with named spreads are explicitly rejected until their
bounded override and call relation is implemented.

Public checked-compile cases cover nested aliases, direct spreads, optional
positions, declaration output, indexed reads, type and arity errors, invalid
and cyclic aliases, and the expansion limit. A public pure-contract case
checks valid values and rejects wrong positions and short suffixes. Five
pinned TypeScript 5.9.3 `--noEmit` cases agree on acceptance and TS2322
diagnostic lines. BlueTS, bridge, and full workspace tests, all-target Clippy,
rustfmt, and whitespace checks pass with the reused 12 GiB target. Concrete
generic specialization is next.

### J.3.1.3.4.4.2.3.2.3.3.3.6.3 Concrete generic tuple spread specialization

Generic tuple alias definitions may contain a rest type parameter constrained
to an array or tuple. The checker validates the template's shape through its
constraint but retains the parameter in the declared type. At a concrete use,
it completes default type arguments, substitutes them through nested generic
aliases, and expands a finite tuple argument into positions. An array
argument remains one symbolic tail, including when a required suffix follows
it. Each alias and produced position consumes the existing expansion budget.

Public checked-compile cases cover direct and nested alias instantiation,
finite tuple and array arguments, defaulted arguments, tuple constraints,
preserved declaration spelling, and rejected arity, element, tail, and
constraint mismatches. Unconstrained parameters and aliases that resolve to
non-tuple/non-array types still fail before output. Seven pinned TypeScript
5.9.3 `--noEmit` cases agree on acceptance and TS2322/TS2344/TS2574
diagnostic lines. BlueTS, bridge, and full workspace tests, all-target Clippy,
rustfmt, and whitespace checks pass with the reused 12 GiB target. Class
method rest parameters with these named spreads remain explicitly rejected
until the next override-comparison leaf.

### J.3.1.3.4.4.2.3.2.3.3.3.6.4 Concrete named spreads in class overrides

Class rest parameter validation and inherited-method comparison now resolve a
named alias or concrete generic instantiation to its tuple or array shape
within the type-expansion budget. Finite named spreads contribute their
expanded positions; an array alias remains one symbolic rest tail. Override
arity and positional type checks reuse the existing fixed, trailing, and
middle tuple relations after specialization. A failed or over-budget
specialization reports a diagnostic instead of skipping the override check.

Public checked-compile cases cover accepted instance overrides in both
inheritance directions, static methods, concrete generic aliases, and an
array-tail alias. Required arity, element, and static mismatches report a
type diagnostic at the derived member; a small expansion budget rejects a
large named rest. Class runtime emission remains outside this leaf, so the
accepted fixture still has only the existing class-runtime unsupported
diagnostics. Four pinned TypeScript 5.9.3 `--noEmit` cases agree on acceptance
and TS2416/TS2417 diagnostic lines. BlueTS, bridge, and full workspace tests,
all-target Clippy, rustfmt, and whitespace checks pass with the reused 12 GiB
target. Constrained symbolic generic tuple spreads are next.

### J.3.1.3.4.4.2.3.2.3.3.3.6.5.1 Generic function tuple spread constraints

Generic function checking now carries array- and tuple-constrained type
parameters into tuple spread validation and generic alias type-argument
checks. The parameter remains symbolic in the retained annotation and
declaration output; its constraint is used only to establish an admissible
tuple or array shape. Function scope restoration removes those constraints
before checking later declarations. An unconstrained spread or a type
argument whose bound cannot satisfy an alias constraint reports a diagnostic
before JavaScript output.

Public checked-compile cases cover constrained direct tuple spreads and a
generic tuple alias used in a function parameter, JavaScript type erasure,
declaration spelling, and rejected unconstrained or mismatched constraints.
Three pinned TypeScript 5.9.3 `--noEmit` cases agree on acceptance and
TS2344/TS2574 diagnostic lines. BlueTS, bridge, and full workspace tests,
all-target Clippy, rustfmt, and whitespace checks pass with the reused 12 GiB
target. Symbolic assignment and return relations follow in the next leaf.

### J.3.1.3.4.4.2.3.2.3.3.3.6.5.2 Symbolic tuple spread assignment and return comparison

Assignment, initializer and return checks now compare a tuple whose rest is a
symbolic type parameter. An unresolved spread name (already admitted by
annotation validation) stays opaque, so only an identical tail compares equal;
a tail that differs by parameter is rejected even when both share a
constraint. When the direct comparison fails, only the source side may widen a
symbolic parameter to its array or tuple constraint, so `[number, ...T]` with
`T extends [string]` is assignable to `[number, ...string[]]` while a target
that names another parameter, or requires a fixed length, is not. The target
side is never widened. Rejections use the existing type and return mismatch
diagnostics before output.

Public checked-compile cases cover identical tails through an alias, a local
initializer and a return, safe widening, distinct parameters and a fixed-length
target. Three pinned TypeScript 5.9.3 `--noEmit` cases agree on acceptance and
the TS2322 lines. The BlueTS crate tests, all-target Clippy for the crate,
rustfmt and whitespace checks pass; a full workspace run was not repeated for
this leaf. Unresolved, cyclic, unsupported and over-budget symbolic cases
close in the next leaf.

### J.3.1.3.4.4.2.3.2.3.3.3.6.5.3 Symbolic tuple spread closure

Unresolved names, cyclic aliases and non-array constraints are rejected before
output and agree with pinned TypeScript 5.9.3 (TS2304, TS2456/TS2315, TS2574).
A nested alias spread of a constrained parameter is accepted by both. Two
symbolic rests and a union constraint are accepted by `tsc` but rejected by
BlueTS before output because no bounded comparison exists for them; this is a
deliberate, documented fail-closed divergence. The generic-expansion limit
reports a resource-limit diagnostic. This leaf changed no behaviour; it pins
the boundary with public tests and a pinned-`tsc` comparison, and closes the
tuple rest/spread family. Optional tuple spread expansion and parenthesized
union array annotations are deferred (see the TODO stop-loss).

### J.3.1.3.4.4.3 Inherited method overload-set compatibility

An override is compared with the nearest earlier local ancestor declaring the
same method side. Each side exposes its overload signatures, or its single
implementation when it has none; an overloaded declaration never exposes its
implementation signature. Every inherited signature must be matched by some
overriding signature: the overriding signature may not require more
arguments than the inherited one can supply, parameters compare bivariantly,
and the result must be assignable to the inherited result unless that result
is `void`. A failure reports `TypeMismatch` at the overriding group, and an
exhausted generic-expansion budget reports a resource-limit diagnostic.

The new comparison lives in `classes/overrides/overloads.rs`, because
`overrides.rs` already exceeds the file-size guideline. Signatures with rest
parameters or missing annotations, and bases reached only through an imported
class, stay outside the compared subset and keep the earlier single-signature
behaviour; classes are still refused before output by the existing
class-runtime diagnostic. Eight pinned TypeScript 5.9.3 `--noEmit` cases
(valid overload sets, edge cases with `void` and optional extras, a single
wide override, a missing overload, a wrong result, a static side, an ancestor
past an intermediate class, and extra required parameters) agree on
acceptance and TS2416/TS2417 lines.

### J.3.1.3.4.5 Derived constructor `super` calls

`super(...)` is now retained as a structured expression statement in function
bodies (it was an opaque token), so the checker can see it. For a class with a
bound base, its constructor must contain a `super(...)` call somewhere in its
top-level or braced-block statements; the first such call may not be preceded
by a statement that reads `this`, nor contain `this` in its own arguments.
A statement containing an arrow or `function` is skipped for the `this`
ordering, since evaluation of a nested function is deferred. A top-level
`super(...)` statement is checked with the same argument selection used for
`new`: the arguments must be accepted by one of the base constructor's
signatures, so overload sets are honored and an exhausted expansion budget is a
resource-limit diagnostic. A `super(...)` call in a class with no base is
rejected. A base whose constructor signature is not yet bound (an omitted
derived constructor) is skipped, since its heritage diagnostic already
applies.

Seven pinned TypeScript 5.9.3 `--noEmit` cases (a valid matrix with nested and
repeated calls and overloads, a missing call, wrong argument count and type,
`this` before `super`, an overload mismatch, and a call in a base class) agree
on acceptance and the TS2377, TS2554, TS2345, TS17009, TS2769 and TS2335
lines. Property initializers and parameter properties do not exist yet, so the
stricter "first statement" rule they trigger is deferred to J.3.2. Classes are
still refused before output by the class-runtime diagnostic.

### J.3.1.3.4.6 `super` member reads and calls

In a derived class with a bound base, `super` is added to each constructor and
method scope: the base instance type in constructors and instance methods, and
the base constructor side in static methods. Member calls on it then use the
same lookup, argument, arity, overload-selection and result checks as `this`,
so a call returns the selected overload's result, a member that the base side
does not have is rejected, and an instance method cannot reach a static base
member (or the reverse). `super` in a class with no base, and `super(...)`
outside a constructor, are rejected before output.

Ten pinned TypeScript 5.9.3 `--noEmit` cases (a valid matrix covering
instance, static, overloaded and constructor uses, plus result, argument type,
argument count, missing member, both wrong sides, overload result, no base and
call-in-method errors) agree on acceptance and the TS2322, TS2345, TS2554,
TS2339, TS2576, TS2335 and TS2337 lines. Non-call `super` property reads and
assignments through `super` are not separately checked, and classes are still
refused before output by the class-runtime diagnostic. `classes.rs` grew to
about 1,380 lines; splitting it belongs to the modularity audit
(C3.1.3.4.7.5).

### J.3.1.3.5 Class checker matrix

`backend/bluets/tests/fixtures/typescript_oracle/class-checker-matrix.tsv`
lists every class fixture entry with pinned TypeScript 5.9.3's verdict, using
the same compiler options as the oracle tests (`main.ts`, or each `*valid.ts`
and `*error.ts` file in a directory of several entry modules). It holds 310
entries, 78 accepted and 232 rejected.

A full sweep found no disagreement. For every rejected entry BlueTSC reports at
least one checker diagnostic; for every accepted entry it reports only the
class-runtime refusal (BTS1001). `bluetsc check` never succeeds on a class and
`bluetsc build` writes no output for any accepted class, so classes remain
refused until the emit and runtime leaves are complete. The direct script and
module routes also refuse checker-accepted classes.

Ordinary tests compare BlueTSC with the record, require the record to list
every class fixture (regenerate it with the ignored oracle test and
`BLUEICE_WRITE_CLASS_MATRIX=1`), and cover refused builds. The ignored oracle
test re-derives all 310 verdicts from the pinned compiler, so the two
compilers agree transitively. Flipping one recorded verdict makes the ordinary
test fail, which confirms the comparison is discriminating.

Two limits: the matrix compares accepted versus rejected, not message text or
line numbers (the per-feature oracle tests pin lines), and it covers only
classes the checker binds today; fields, accessors and private members
(J.3.2) extend it as they land.

### J.3.1.4 Class JavaScript and declaration output

(Superseded: the switch described here was removed in J.3.1.6.) Output was gated by a temporary, default-off `CompilerOptions::class_emit`
switch (part of the build fingerprint). With it on, the checker drops the
class-runtime refusal only for a class whose every member is a parsed
constructor or method; a field, accessor or any other member still keeps the
refusal, so emitted JavaScript can never carry unerased TypeScript. Direct
lowering, the standalone CLI and every default caller leave the switch off, so
classes remain refused there. J.3.1.6 removes the switch once strict-boundary
policy, debugger mapping and stale-generation refusal are verified.

JavaScript is the source text with parameter and return annotations erased by
the parser and, new here, every constructor and method overload signature
erased, since a declaration with no body has no JavaScript form. Source maps
come from the same provenance emitter as functions, and the source-map segment
limit applies unchanged. A checker error still produces no output, so the
build stays atomic.

Declarations render an exported class, or one named by a value or default
export, as `declare class Name [extends Base]` followed by four-space members
in source order: each constructor and method overload signature, or its single
implementation signature when it has no overloads, with `static` on the
constructor side. A parameter with a default renders as optional, a rest
parameter keeps its `...`, and a method with no return annotation is refused
because BlueTS does not infer one. Two new pinned TypeScript 5.9.3 cases run
BlueTSC's and `tsc`'s JavaScript in Node with identical output and compare the
declaration byte for byte.

Limits: JavaScript keeps BlueTS's existing erased-annotation whitespace (an
erased return type joins `)` and `{`), so its text differs from `tsc`'s while
its behaviour matches. Fields, accessors, private members, parameter
properties, generic classes and `implements` do not exist yet (J.3.2).

### J.3.1.3.4.4.2.3.2.3.3.4 Optional tuple spreads

A call whose last argument spreads a lone variable typed as a tuple ending in
optional elements is checked once per possible spread length, with the variable
replaced by that fixed-length prefix in a copy of the scope; the call is valid
only if every length is. This reuses the existing function, method, constructor
and `super` call checks unchanged, so each keeps its own arity, overload and
type rules, and repeated diagnostics from the per-length passes are removed. A
spread that is not a lone variable, is not the last argument, or names a
variable another argument also uses keeps the fixed-length requirement, since
replacing the variable would change that argument's type. Tuple literals keep
the optional flag of a spread element, and a required element after an optional
one is left unmodelled.

### J.3.1.3.4.4.2.3.2.3.4 Parenthesized types

A `(` in a type now groups unless its matching `)` is followed by `=>`, which
makes it a function type. `(A | B)[]`, `((a: T) => R)[]` and tuple rests such as
`...(string | number)[]` parse, and the class-override comparison of a tuple
rest against an array rest works on them. Declaration output and type identity
keep the parentheses around an array element that is a union, intersection or
function type; without them `(string | number)[]` and `string | number[]`
produce the same text and would be treated as one type. Redundant parentheses
around a non-array type are not preserved, unlike `tsc`.

### J.3.1.3.6 Tuple literal context for returns

An expression checked against an expected type is inferred by
`infer_in_context`. When the expected type is a tuple (after alias expansion) and
the expression is a bracketed literal, each element is inferred against the
tuple element at the same position while positions are still known, so a nested
`[[1, "a"], true]` works and `[1, 2]` is reported against `[number, string]`
instead of as a widened array. A spread of a tuple contributes its elements with
their optional flags; after a spread, later positions have no fixed context and
fall back to plain inference. A rest element gives its array element type as
context. Anything that is not a plain bracketed literal is inferred as before.

The same method now serves tuple-annotated initializers (replacing a helper that
typed elements from their first token only), function returns and class method
returns. Class constructor returns keep their own primitive/instance rule.
Before this, every `return [..]` against a tuple return type was rejected,
because the literal was widened to an array. Positions other than these three
(call arguments, object properties, assignments, arrow returns) still widen and
are tracked as J.3.6.

### J.3.6.1 Tuple literal context for call arguments

`expanded_call_argument_types_for` types the arguments of a call against the
callee's candidate signatures. A bracketed literal at a position still known is
inferred against each candidate's parameter type at that position, in order (a
rest parameter supplies its element type), and takes the first whose result is
assignable to it; if none accepts it, the literal keeps its plain inferred
type. Context therefore only turns a rejection into an acceptance. After a
spread argument, later positions have no fixed parameter and use plain
inference. Function calls, function return inference, member calls (instance and
static, single and overloaded), constructors and `super` calls all use it; the
optional-spread per-length scopes are unaffected.

Rejected cases keep their existing wording, and a failing tuple literal is still
reported with its widened array type; showing the tuple type is a message-quality
improvement, not a behaviour change. Function `rest` parameters annotated with
a tuple are still refused (only class methods accept them), and object-literal
arguments have no record context yet (J.3.6.2).

### J.3.7 Function-expression bodies are unchecked

Arrow functions and function expressions are kept as tokens; only named
function and class-method bodies are structured and checked. Every error `tsc`
reports inside an arrow or function-expression body (wrong return type, a
parameter used at the wrong type) is accepted without a diagnostic. This is a
missing check, not a context gap, and needs a structured body in the parser
first.

### J.3.7.0 Annotations that survive erasure

The parser erases annotations on named functions, class methods and variable
declarations through recorded edits, but keeps the tokens of a function nested
inside an expression verbatim. An arrow with a return type, a function
expression, an object method, getter or generator, a function declared inside a
body, and a typed catch binding therefore reached the emitted JavaScript with
their TypeScript annotations, and `check` and `build` both accepted them. Only
an arrow with parameter annotations but no return type was already refused.

Once edits are applied no type position remains in the text, so a leftover
annotation can be found precisely. After the module parses cleanly, the parser
applies its edits, tokenizes the result and flags: a parameter name inside
parentheses, at the start of a parameter, followed by `:` or by an optional
`?`; a destructured parameter followed by `:`; a `)` followed by `:` and a type
that ends at `=>` or, on a `function` or method parameter list, at `{`; and a
`function` followed by type parameters. Each flag is mapped back through the
edits to its original span and reported as unsupported syntax, at most sixteen
per module. Because it runs in the parser, `check`, `build` and the direct
bridge agree.

The rules are deliberately fail-closed. A return type that starts with `(` is
skipped, so `c ? (1) : (b) => b` is not mistaken for an annotation, but
`c ? (a) : b => d` is refused as a possible return annotation. This is a
safety net, not the feature: J.3.7 replaces the refusal by structurally
parsing, erasing and checking these forms.

### J.3.7.1 Structured arrow functions

The scan over a runtime expression range stops at an arrow head at the start of
an operand: a `(`, or a lone name followed by `=>`, whose previous token does
not end a primary expression. The parameter list is parsed with the named
function's `parse_parameters`, so annotations, optional markers and defaults are
erased through the same edits; a result annotation is parsed as a type up to
`=>` and erased through the last token of the type, keeping the space before
the arrow. A braced body is parsed with `parse_function_body` into the same
structured items, returns and locals as a named function; a concise body keeps
its expression tokens and the scan continues inside it, so an arrow inside an
arrow is found by the same pass. Each arrow is stored in `Module::arrow_functions`
by the byte offset of its first token, spanning the head to the end of the body. The table is `Module::nested_functions` and also holds function expressions (J.3.7.3).

A parameter list is left unstructured when any parameter is a destructuring
pattern, when the arrow is `async` (its result is a Promise, which BlueTS does
not model), or when a result annotation appears inside a conditional's
consequent, where `c ? (a) : b => d` is more likely a conditional than an arrow
with a result type. The erasure audit of J.3.7.0 still refuses such an arrow if
it carries an annotation.

The checker checks each outermost arrow inside an expression once, through a
`FunctionDeclaration` built from it and `check_function_in_scope`, which now
takes the scope the body sees: the module values for a named function, the
enclosing scope for an arrow. A concise body becomes a single return. So the
result annotation, missing-return detection, default initializers, parameter
scope and closed-over variables use the named-function rules unchanged, and a
tuple result is read against its declared tuple. An arrow expression is typed as
a function type from its parameters and result annotation (or its concise body).

### J.3.7.2 Calls through function-typed values

A callee that is in scope with a function type (a parameter, a local, an arrow)
is treated as a single-signature function: its arguments are checked for arity
and type, the result is its result type, and it shadows a module function of the
same name. A callee in scope with any other type has an unknown result; before,
its own type was returned as the call result. The direct bridge has no arrow
node in its expression grammar and refuses every arrow, which a bridge test now
pins.

### J.3.7.3 Function expressions

`function [name] (parameters) [: result] { body }` at the start of an operand
uses the same side table as an arrow: the parameter list goes through
`parse_parameters`, a result annotation is erased through its last token so the
space before `{` stays, and the body is parsed with `parse_function_body`. The
entry is keyed by the `function` token and carries the optional name. The
checker checks it like an arrow, and additionally binds a name inside its own
body as a function type, so `function fact(n: number): number { return n ? n *
fact(n - 1) : 1; }` type-checks. Generators, `async`, generic function
expressions and destructured parameters are left as tokens and refused by the
erasure audit when annotated. Statement-level function declarations inside a
body are a separate form (J.3.7.5). Untyped function expressions are accepted
where `tsc` under `--strict` would report an implicit `any`; that difference
already exists for arrows.

### J.3.7.4 Object-literal methods and accessors

`NestedFunction` gains a `kind` (arrow, function, method, getter, setter). At a
property position, that is, a name token right after `{` or `,` whose nearest
enclosing bracket is a `{`, the parser looks for `name(` (a method), or
`get`/`set` followed by a name and `(` (an accessor), takes the parameter list
and optional result annotation through the shared helper, and requires a braced
body; otherwise it leaves the tokens alone. A `get`/`set` directly followed by
`(` is an ordinary method with that name. The entry is keyed by the member's
first token. A getter must take no parameters, and a setter exactly one and no
result annotation.

The checker checks each member like a function expression over the scope of the
object literal. Object inference reads the table by the member's first token: a
method becomes a field with a function type, a getter a field of its result type,
and a setter a field of its parameter type unless a getter for the same name
exists. Before this an object with a method fell out of inference as unknown, so
neither the method body nor calls to it were checked. `this` inside a member is
not typed. Getter-only properties are not treated as read-only.

### J.3.7.5 Nested function declarations

A `function` token at a statement position inside a function body (depth zero of
parentheses and brackets, not preceded by `async`) with a name, a `(`, a
simple parameter list and a braced body is parsed with the shared helper into a
`FunctionDeclaration` and added to the body as `FunctionBodyItem::Function`.
The body is parsed with fresh returns and locals, so a nested function's
variables and returns never reach the enclosing function. Generators, generic
declarations and destructured parameters fall through to the token path and are
refused by the erasure audit when annotated.

Before checking a body, the checker binds every nested declaration in it, at any
block depth, to its function type in the body's scope, the same treatment the
flattened `locals` already get. That gives JavaScript's hoisting: a call before
the declaration, recursion and mutual calls between siblings type-check. Each
nested body is then checked over the scope it appears in, and the enclosing
function's return analysis treats the item as a no-op. The rule ignores block
scoping (a declaration inside an `if` is visible to the whole body), which only
makes the checker more permissive than `tsc`. Named functions, class methods and
constructors all bind and check nested declarations.

The direct bridge has no lowering for a nested declaration and refuses it in its
statement, `try` and `while` paths.

### J.3.7.6 Catch binding annotations and try termination

The shape detector for a structured `try` accepted only `catch ( name ) {`, so
`catch (e: unknown)` fell to the token path, where its annotation was copied into
the emitted JavaScript until the erasure audit refused it. It now matches the
parenthesized binding as either a bare name or `name : Type`. The parser parses
the annotation, erases it through the end of the type and stores it on the
clause; `any` and `unknown` are the only annotations TypeScript allows, and any
other is a parse error. `any` types the binding as `any`; `unknown`, or no
annotation, keeps the existing strict `unknown` that holds only inside the catch
body. All three catch-scope sites (return traversal, expression traversal and
class bodies) share one helper.

A `try` used to be a no-op in the termination analysis, so a function whose last
statement was `try { return a; } catch { return b; }` was reported as able to
complete without returning. A `try` now ends every path when its `finally` does,
or when its block does and its `catch` clause, if present, does as well;
unknown syntax anywhere keeps the result opaque as before.

The direct bridge lowers a structured typed catch binding as a plain one, since
the annotation has no runtime meaning; its old test that listed the typed form
as excluded now asserts the lowering.

Known gap, not fixed here: identifier inference looks at module functions before
the local scope, so a catch binding, parameter or local named like a module
function is typed as that function. Preferring the scope was tried and broke
callback checking, because module function names also appear in the scope
map, so the two cannot be told apart that way; a fix needs to know which scope
entries are real locals.

### J.3.7.7.1 Generic nested functions

The three nested-function parsers accept `<..>` where a generic function has it:
an arrow head that starts with `<` and whose matching `>` is followed by `(`, a
function expression with `<..>` before its parameter list (after an optional
name), and a nested declaration with `<..>` after its name. The list is parsed
with the named-function routine and erased through the `>`; the type parameters
stay on the entry (`NestedFunction::type_parameters`, or the declaration's own
field), so the checker's existing generic handling applies: the parameters are in
scope for the parameter and result annotations, unknown names are reported, and
constraints are checked.

A generic function used as a value would otherwise reject a valid call, because
its parameter type is the name `T`. Its function type therefore substitutes each
type parameter with its constraint, or `unknown`, in the parameter and result
types. That is permissive and sound for arguments but loses precision: the
result of `identity(1)` is `unknown`, not `number`, where `tsc` infers `T`.
Inference of type arguments for a value call is a separate feature.

The old refusal of a generic arrow ran over raw tokens before any parsing and
missed a result annotation. It is removed, and the erasure audit now refuses a
`<..>` still present in front of a parameter list at the start of an operand, so
a shape that is not structured (`async <T>(a) => a`, a destructured parameter
list) stays refused instead of reaching the output.

### J.3.7.7.3.1 Promise, async functions and await

Named `async function` declarations were already parsed and carry an
`async_function` flag, but `Promise` did not exist as a type, so any annotation
using it was reported as unknown, and `await` had no typing and no context
check. A module's type table now gets a built-in generic interface `Promise<T>`
when nothing local or ambient defines it, after ambient declarations are bound,
so a host that supplies its own `Promise` keeps it. Its `then` takes a callback
of `(value: T) => any`, `catch` a callback of `(reason: any) => any` and
`finally` a callback of `() => any`, each returning `any`. `T` occurs only in the
callback parameter of `then`, which makes a promise covariant in `T` under the
existing function-type comparison, and no method returns a promise, so the
definition is not self-referential. The price is that chaining loses precision.

For an `async` function the checker requires the annotation to be `Promise<T>`
(the error for anything else is TS1064's counterpart) and then checks the body
against `T`: returns, the implicit-`undefined` rule and the can-complete-without-
returning analysis all use `T`, so `Promise<void>` allows falling off the end. A
function with no annotation is unchanged.

`await x` is typed as the `T` of a `Promise<T>` operand, and as the operand's
type for anything else. The checker records whether the function being checked
is `async` (unset at module level) and refuses an `await` in a function that is
not, and at the top level: JavaScript would reject both. Tokens that lie inside
a structured nested function are judged in that function's own context, not the
enclosing one, and an expression that also holds an `async` token is not judged
because it may contain an unstructured async form. That last case is a known
hole: `[async () => 1, await f()]` in a sync function is not refused.

The result type of a call is the declared `Promise<T>`, so `run().then(..)`
type-checks against the built-in methods. `Promise` values from other sources
(`Promise.resolve`, `new Promise`) are not modeled and have unknown type.

### J.3.7.7.3.2 Async nested functions

`NestedFunction` gains an `async_function` flag, and the four parsers now take an
`async` that immediately precedes the function head as part of it: an arrow (with
parentheses or a lone name, with or without type parameters), a function
expression, an object member written `async name(..)` (a method named `async` has
`(` right after it), and a nested declaration (recognized where the statement
loop meets the `async` token, so no stray opaque item is left in the body). The
entry's span and key start at the `async` token, which keeps the exact-match
lookup that types an arrow expression working. Erasure is unchanged: `async`
stays and the annotations go.

The checker copies the flag into the `FunctionDeclaration` it builds, so the
`Promise<T>` return rules and the await context of J.3.7.7.3.1 apply. A function
value's type keeps a declared `Promise<T>` result and wraps an unannotated async
function's body result in `Promise`, for expression-bodied arrows, hoisted
declarations and object methods alike. For an object method the property name is
the token after `async`.

The await context check used to skip any expression holding an `async` token,
because that token could begin an unstructured async form. It now counts only an
`async` token that does not start a structured nested function; those are judged
in their own context, so `[async () => 1, await f()]` in a sync function is now
refused. An async form the parser still leaves as tokens (an async generator, an
async arrow with a destructured parameter) keeps the old skip.

### J.3.7.7.2 Destructured parameters

`Parameter` gains an optional `pattern` (`BindingPattern`: a flat object pattern
of `key`, bound `name` and default, or a flat array pattern of named, defaulted
or skipped elements). A destructured parameter's `name` is the source text of its
pattern, so the existing declaration printer, which writes `name: Type`, produces
`{ a, b }: Props` with no special case, and the scope, which would otherwise gain
a bogus entry, binds the pattern's names instead.

`parse_binding_pattern` parses the supported subset from tokens and does not
touch parser state, so the pre-scan that decides whether an arrow, function
expression or method can be structured (`simple_parameter_list`) and
`parse_parameters` share it. A pattern outside the subset (nested, rest,
computed or string key) makes the pre-scan refuse to structure the form, which
leaves it to the erasure audit; in a named function declaration, where
`parse_parameters` runs directly, it is an unsupported-syntax error.

The checker types the bindings from the annotation, expanding aliases. An object
pattern reads each key through the ordinary property lookup, so an optional
property is `T | undefined` unless the binding has a default; a key the record
does not have is an error. An array pattern reads the tuple element at its
index (an optional one adds `undefined`, a rest element gives its array's element
type, an index past the end is an error) or an array's element type. A default
must be assignable to the value type and removes `undefined`. Without an
annotation the names are `unknown`. Class methods and constructors bind pattern
names as `unknown`, since their scope is built without error reporting.

The direct bridge lowers parameters by name and has no pattern support, so it
refuses a destructured parameter rather than lowering the pattern text as a
name; a bridge test pins this.

### J.3.1.5 Direct lowering of classes

A checked class lowers to BlueJS's own class node, `Stmt::ClassDecl`. Each
constructor and method member with a body becomes a `ClassElement::Method` whose
function is built by the same parameter and body lowering as a function
declaration (`lower_function_value`, split out of `lower_function`): the
constructor is a non-static method with the key `constructor`, as BlueJS's own
parser represents it, methods keep their `static` flag, overload signatures have
no body and are skipped, and `extends` is the base class name as an identifier.
The class has no decorators and no source text, so `toString` reports a native
function, as for every synthesized function. A script refuses an exported class;
the module bridge lowers it and adds an export entry under its name.

The bridge's expression grammar had no `this` or `super`. It now lowers `this`
to `Expr::This` and `super` to `Expr::Super`, and accepts `super` as a call
target, so `super(..)` is an ordinary `Call` on that node and `super.m(..)` a
`Member` call, which is how BlueJS represents them and validates their
placement. A destructured parameter, a nested declaration or an object-literal
form inside a method is refused by the same paths as in a function.

(Superseded by J.3.1.6, which removed the switch.) The checker admitted a class only under the `class_emit` staging switch, so the
direct route reaches this lowering only when the caller's options set it, and
`CompilerOptions::default()` still refuses classes. J.3.1.6 verifies the
strict-boundary, debugger and stale-generation behaviour and removes the switch.

### J.3.1.6a Class debugger mapping, stale generations and strict boundaries

Three things J.3.1.6 asks for, verified on the direct route with classes
admitted.

Debugger mapping. The safe-point map pairs each root statement with the code
units it produced: a function declaration owns one child closure, found through
`root_function_child_indices`. A class owns several (its constructor and each
method), and the map gave those no source location, so a frame paused inside a
method had no original position. BlueJS now records, for every root statement
other than a function declaration, the range of direct child closures compiling
it created (`Bytecode::root_statement_child_ranges`, filled by counting the
children before and after the statement), and the bridge maps every safe point of
every closure in that range to the whole original class declaration. Attachment
still refuses a program whose statement ranges disagree with its lowering, and
an existing attachment is matched against the new ranges as well.

Stale generations. A frame paused in a method is refused after navigation, along
with the map and the debug registry entry, exactly as for a function; a test
pauses in a Base method, checks the mapped span is the original class, then
navigates and checks all three are refused. A nested pause is only supported in
a method the root code calls directly (BlueJS refuses it elsewhere), so the test
pauses in the one the script calls.

Strict boundaries. An emitted strict module admits only the owner-selected
string functions and refuses every other runtime declaration, so a class in such
a module is refused with an invalid-contract diagnostic even when classes are
admitted; a test pins it.

### J.3.1.6b Classes are admitted by default

The `class_emit` staging switch is gone. The checker admits a class whose every
member is a parsed constructor or method, in every entry point: `compile`, the
BlueTSC CLI, declaration output and the direct bridge. A class with any other
member, that is a field, accessor, private member or anything else the parser
keeps as opaque tokens, is still refused as unsupported syntax, with a message
that names what is missing, because emitting it would copy TypeScript into the
output; J.3.2 adds those members.

A class and an interface with the same name are also refused. TypeScript merges
the two declarations, so `interface Reader { extra: number } class Reader {}`
gives `Reader` the `extra` property. BlueTS does not model the merge, so admitting
the pair would type only the class and quietly drop the interface's members.
The refusal is unsupported syntax, not a duplicate-declaration error, because the
pair is legal.

The class matrix changes from "BlueTSC never succeeds on a class" to "BlueTSC
succeeds on exactly what pinned TypeScript accepts and BlueTS supports". A
separate hand-maintained deferred list names the accepted entries BlueTS cannot
erase yet (one, a private member); each must fail with only the unsupported
syntax code, and the list must be a subset of the accepted rows so it cannot hide
a real disagreement.

Tests that used a class's refusal as their evidence were updated: an accepted
class now expects no diagnostics and an artifact, a checker error expects none,
and tests that listed a class among refused shapes now use a class with a field.

### J.3.2.1 Public class fields

A member of the form `[public] [static] [readonly] name[?|!][: T] [= init];` is
now a `ClassField` with a structured annotation and initializer. Erasure removes
`public`, `readonly`, the `?`/`!` marker and the annotation and keeps `static`,
so the output is a native class field. That is exactly what tsc emits for
ES2022 (`tag?: string;` becomes `tag;`), and initialization order, including
base-then-derived field order, is the engine's own. An ES2020 target would need
constructor assignments instead, so a field is refused there until J.3.3 rather
than emitted with the wrong semantics.

Typing. The instance record gains the instance fields and the constructor side
gains the static ones, so `this.x`, `obj.x`, `Class.x`, inheritance through the
existing record merge and the existing readonly/property checks all apply
unchanged. A field's type is its annotation, or the widened type of a number,
string or boolean literal initializer (literal when `readonly`). Inferring from
any other initializer needs expression inference at bind time, before the
checker has a scope, so it is refused as unsupported syntax and an annotation is
the fix; an unannotated field with no initializer is TypeScript's implicit-`any`
error.

`strictPropertyInitialization` is modelled conservatively. An assignment at the
top level of a constructor body counts. An assignment only in an `if`, `while`
or `try` may or may not cover every path, and proving that is definite-assignment
analysis, so it is refused as unsupported rather than guessed; no assignment at
all is TS2564.

Readonly. TypeScript lets a constructor assign its own class's readonly fields.
The checker carries the set of those fields for the constructor being checked
and consults it only for a plain `=` on a bare `this`, and clears it inside any
nested function, since a closure is not the constructor.

Overrides. A derived field must be assignable to the base property; a field
cannot replace a base method; a field redeclared over a member of an imported
base is refused, because only that base's type surface is known and a method
cannot be told from a function-typed field there.

The direct bridge lowers a field to a BlueJS class field, so the class-wide
debugger mapping from J.3.1.6a covers the initializer closures as well.

### J.3.2.2 Member visibility

`private`, `protected` and `public` are parsed before a member name, in the only
order TypeScript allows (`accessibility static readonly`); a modifier word counts
only when a name follows, so `private: number` and `public()` are still members.
The keyword is erased with the gap after it and never reaches the output.

Types. A restricted member is recorded in the class's type record only under a
marker name, `private Box@main.ts secret`, that carries the declaring class and
module. It is never under `secret`. Everything TypeScript does for such a class
then follows from ordinary record assignability. A public structural target
(`{ secret: number }`) is not satisfied by a class whose `secret` is private, an
object literal or an unrelated class with an identical private member lacks the
marker, and a subclass inherits it, so it is assignable to its base. A subclass
that redeclares a protected member as public has both the base marker and the
plain name, which is what TypeScript allows.

Access. While a class body is checked, `with_class_access` exposes the markers
that body may use under their plain names and restores the records afterwards:
a private member for the declaring class only, a protected member for the
declaring class and its subclasses, reached through instances of the class being
checked or a subclass (`other.value` on a `Base` is refused, as TypeScript's
TS2446), and for static members through any class constructor. `super` is bound
to a synthetic record that adds the protected methods, so `super.method()` works
and `super.secret` does not. Outside every class body nothing is exposed.

Only a bare `a.b` was ever checked for a missing property, so a private member
would have been readable as `b.secret + 1` or `new Box().secret`. A scan of every
checked runtime expression now visits each `.name` and `?.name` whose name is
restricted in some class, finds the receiver expression's extent, infers its type
and reports a marker it cannot use. A receiver whose type cannot be settled is
refused as unsupported instead of assumed accessible; `any` and bracket access
are permitted, as in TypeScript. The general missing-property check is not
widened, since that would change every existing program; it remains a known gap.

Rules between classes: a base's private member cannot be redeclared, protected
may be kept or made public, public must stay public, all overloads of one
member agree, and a protected member redeclared over an imported base is
unsupported because its signature cannot be compared. A private constructor
allows `new` only inside its class and forbids extending; a protected one also
allows it inside a subclass; an omitted constructor inherits its base's.

Declaration output follows tsc: a private member is `private name;` with no type
(`private constructor();`, overloads once), protected members keep their types,
and `public` is omitted.

### J.3.2.3 Constructor parameter properties

`constructor(public x: number)` declares a property `x` and assigns it from the
argument. The parser reads `[accessibility] [readonly]` before a parameter only
while parsing a class constructor's parameter list; the same words in any other
parameter list stay a parse error, which is how a parameter property on a method
or function is refused. Each recognized parameter is recorded in
`ClassConstructor::parameter_properties`, together with the insertion point for
its assignment, and the modifiers are erased.

TypeScript's ES2022 output is reproduced: the field declarations `x; y;` come
first in the class body, before declared fields, because they are class fields
initialized to `undefined` before the constructor body runs; the assignments
`this.x = x;` come at the start of a base constructor body or straight after the
derived class's `super(...)` statement. The emitter inserts both as zero-width
edits on the line they land in, so no later line, and no source-map line, moves.
A derived constructor with no top-level `super(...)` statement has nowhere fixed
to put them and is refused as unsupported (TypeScript accepts a `super` in each
branch of an `if`).

For checking, the properties are synthesized as `ClassField`s
(`ClassDeclaration::parameter_property_fields`): typing, duplicates, visibility,
nominal assignability, overrides, and assignment of a `readonly` property in the
constructor all come from the field rules of J.3.2.1 and J.3.2.2. A literal
default gives the widened type (not the literal, unlike a `readonly` field), an
optional property is `T | undefined`, and an unannotated non-literal default
needs an annotation. Declaration output lists the properties first, in parameter
order.

### J.3.2.4 Accessors

`get name(): T { .. }` and `set name(value: T) { .. }` are recognized when `get`
or `set` is followed by a name and `(`; a member merely named `get` (`get = 1`,
`get()`) is still a field or method. The accessibility keyword and the
annotations are erased and everything else is emitted as written, so the output
keeps the original accessors. Grammar errors (a getter with a parameter, a setter
with none, two, an optional, defaulted or rest one, or a result annotation) are
parse errors, as TypeScript reports them.

A getter and its setter are one property in the type record. It reads as the
getter's annotation, or the setter's parameter annotation when the getter has
none, and is read-only when there is no setter, so assignments through it, in a
constructor and through an alias, are checked by the existing readonly rules. A
setter-only property reads as its parameter type, which TypeScript also allows.
Accessor bodies are checked by the method machinery through a method-shaped view
(`ClassDeclaration::accessor_methods`): a getter must return its annotated type
and cannot fall off the end, and a setter's result is `void`, so returning a
value is an error.

TypeScript 5.9 allows a getter and setter of unrelated types, which needs a read
type and a write type on one property. BlueTS records one type, so a pair whose
types differ is refused as unsupported rather than typed as one of them. For the
same reason of inference, a getter with no annotation and no annotated setter is
refused: its type would come from its body. An accessor pair must agree on
accessibility (TS2808) and on placement (an instance and a static accessor of one
name are separate members).

Inheritance. A member cannot change kind when redeclared: a property over an
accessor or method, an accessor over a property or method, a method over a
property or accessor. The nearest declaration is found in the chain of local base
classes; for a base outside the module only its type record is known, so an
accessor redeclaring one of its members is unsupported. An accessor over an
accessor must be assignable to the base property type.

Declaration output follows tsc: each accessor is `get name(): T;` or
`set name(param: T);` in source order, a private one is `private get name();` and
`private set name(value);`, with the setter's parameter renamed as tsc does.

### J.3.2.5 ECMAScript private names

The lexer reads `#` as punctuation, so the parser first joins a `#` and the name
written immediately after it into one identifier token `#name` whose span covers
both. `this.#x` is then a normal member access and `#x` a normal member name for
every member parser and for every check that looks at `a.b`, and the emitted text
is still the original source.

A private-name member is `private` by construction: the parser gives it private
visibility (an accessibility keyword on it is an error) and everything from
J.3.2.2 applies. It exists in the class's type record only under a marker that
names its declaring class, becomes visible under its plain name only while that
class's body is checked, is invisible to subclasses and to the outside, and makes
the class nominal. The differences from the `private` modifier are the two
places TypeScript treats them differently: a subclass may declare its own `#x`
(the names are per class, so nothing is overridden and the override and
kind-conflict rules skip a `#` name), and `o?.#x` is an error.

`#x in object` is a brand check. It is valid only inside a class body that
declares `#x` itself; the scan of member accesses reports a `#name` that is not
after a `.` and not before `in`, and one that names nothing the enclosing class
declares. The bridge lowers it to `PrivateIn`, whose name BlueJS keeps without
the `#`. The checker does not compute its type, so it reads as unknown.

ES2022 emits private names natively, and an ES2020 target would need TypeScript's
WeakMap lowering, so it is refused until J.3.3. Declaration output is one
`#private;` at the top of each class that declares any private name, and none of
the names.

### J.3.2.6 Static blocks and static initialization order

`static { .. }` is recognized where `static` is followed by `{`, so a member merely
named `static` is unaffected. Its body is parsed as a function body, so annotations
inside it are erased like any other, and checked with the class body machinery:
`this` is the constructor side (static members and private static names in, instance
members out), `super` is the base's static side, `return` is an error, and `await`
is an error because the block is checked as a non-async function.

Initialization order is the engine's, as in TypeScript's ES2022 output: static fields
and blocks run in source order when the class is defined, then instance field
initializers run at each construction, with a derived class's statics after its
base's. The checker enforces the one static ordering rule TypeScript does: an
initializer or block may not read a static field declared after it, by `this` or by
the class name. Both forms are refused as unsupported instead of modelled when the
read sits in a nested function or arrow, because whether such a function runs before
the field is initialized depends on its callers.

### J.3.3.1 `useDefineForClassFields` and downlevel class fields

TypeScript has two meanings for a class field. With `useDefineForClassFields` the
field is defined (native ES2022 class field syntax, or `Object.defineProperty`
below ES2022), which creates the property even when there is no initializer.
Without it the field is assigned in the constructor, so a field with no initializer
does not exist and a setter on the prototype chain runs. The default follows the
target: define for ES2022, assign for ES2020. BlueTS takes the same option and the
same default, and records the resolved choice in the fingerprint and the manifest.

ES2022 with define semantics keeps the source text and only adds text for
parameter properties. Every other combination lowers fields by moving text:

- Instance fields go to the start of the constructor, after the parameter
  properties and, in a derived class, after the top-level `super(...)`. A class with
  no constructor gets one, with `super(...arguments)` when derived. A derived
  constructor with no top-level `super` has nowhere to put them and is refused.
- Static fields become a static block in place (ES2022, assign) or statements after
  the class (ES2020). A static block below ES2022 becomes
  `(function () { .. }).call(Class);`.
- `this` in a static initializer means the class. Rather than rewrite `this` tokens,
  which would also rewrite the `this` of a method or function nested in the
  initializer, the initializer that mentions `this` is evaluated as a function called
  on the class. `super` has no such form, so it is refused.
- The text that moves is the original source with the erasing edits recorded for
  it applied, cut out with `apply_edits` over just that range, and inserted on the
  line it lands on so no later line moves.

The checker gains the one diagnostic that depends on the option: under define
semantics a derived class that redeclares a base instance property with no
initializer and no constructor assignment overwrites the base value with
`undefined`, which TypeScript reports as TS2612. It does not apply to static
fields, private names, or a property assigned in the constructor.

The direct bridge reads the same option. For define semantics it keeps BlueJS
fields (which define). For assign semantics it emits the constructor assignments,
static blocks, and a synthesized constructor as AST, in the same positions as the
emitter.

The proof is behavioural: four programs whose output depends on which properties
exist, their order, and initialization order run through BlueTSC and pinned
TypeScript for ES2022, ES2022 with assign semantics, ES2020, ES2020 with define
semantics, and ES2020 with assign semantics, and must print the same thing under
Node.

### J.3.3.2 Private names below ES2022

A private name cannot be lowered to a property, so it is lowered to state the
object does not carry, which is what makes it private: a `WeakMap` from instance to
value for each field, one `WeakSet` of instances per class (the brand) for its
private methods and accessors, the method and accessor bodies as plain functions
called with the instance, and for a static member the class itself as the brand
with a `{ value }` holder for a field. Each read, write and brand check becomes a
call of one of three helpers that check the brand and throw a `TypeError` as the
specification does. The helpers are written here from those semantics, carry a
version (`bluets-class-helper-v1`) that is part of the build fingerprint and a
leading comment in the output, and are defined once per module (only those used) ahead
of the first class that needs them, as `var`s so they exist by the time any class
runs.

The rewrite works on tokens, not on an expression tree, so it does only what it can do
without changing meaning. A private access is `RECV.#x` with `RECV` either `this` or
one plain identifier, which may therefore be repeated (the helper takes it twice for a
read-modify-write and a method call) without evaluating anything twice. Assignment,
compound assignment (`+=`, `-=` and so on, with the right side parenthesized) and
`++`/`--` are supported only as whole statements, since replacing them inside a larger
expression needs the value they produce; logical assignment, an increment used as a
value, and any other receiver are refused, as unsupported below ES2022, instead of
guessed. `#x in o` needs a plain `o`. Accesses are rewritten last to first so an
assignment's right side is already rewritten when the assignment's text is built, and
moved method and initializer text is rendered with every recorded edit applied.

Statements that must run in the constructor go there in this order: the brand
(`_C_instances.add(this)`), parameter properties, then fields in source order,
public and private interleaved as declared. After the class come the `WeakMap`s and
`WeakSet`, then the method and accessor functions, then the static members in source
order. The lowering declares `_C_x`-style variables and refuses a source that already
uses one, or one of the helper names.

### J.3.4.1 and J.3.4.2 Enums

`enum` declarations are parsed into `EnumDeclaration` with their members and
their initializers' original tokens. Everything about a member's value comes from
one evaluator (`enum_eval.rs`) that the checker and the emitter both call, in
source order over the module so a later declaration of a name sees the earlier
one. A member's value is constant when its initializer is a constant expression
in TypeScript's sense and otherwise computed. JavaScript's number semantics are
reproduced where they differ from Rust's: bitwise and shift operators go through
`ToInt32`/`ToUint32` with the shift count masked to 5 bits, `**` has JavaScript's
special cases, and numbers print as `Number.prototype.toString` prints them
(`1e+21`, `-1`, `0.5`, `Infinity`). The parser presents `>>` and `>>>` as separate
`>` tokens one byte apart (for generic closers), which the evaluator puts back
together.

Emit is TypeScript's own shape: a `var` and an immediately called function that
fills the object, called with the object itself or a new one so declarations of a
name merge. The replacement text keeps the declaration's line count, spread over
its parts, so no later source line moves.

For typing, an enum binds four names: the type `E` (the union of the member types,
or `number` with a computed member), one type `E.A` per constant member, the
object type `typeof E` with readonly members, and the value `E`. A member type is
the literal type spelled `E.A`, so it is distinct from the number `0`, and its
definition records the member's constant so the relation rules can compare it:
`(E.A, number)` is assignable by the member's value, `(E.A, string)` for a string
member, and `E.A` to `E.B` is not. The assignability rules that are specific to
enums sit in front of the general ones: a numeric enum accepts any `number` and
a number literal only when some member has that value, and a string enum accepts
only its own members. Because inference widens a bare number literal to `number`,
a literal read where an enum is expected (a declaration, a return, an argument,
an assignment) keeps its value as a literal type; an arithmetic result is a plain
number, which TypeScript also accepts.

`E[n]` is typed by a special case in the indexing path: a number-like index on an
enum object with a numeric member is the member's name, and everything else that
is not a member name is an error.

Not modelled, and refused or recorded instead: `const enum` (its own leaf), an enum
imported from another module, an enum in a declaration module, an enum declared
inside a body, and a computed initializer that names a sibling member bare.

### J.3.4.3 const enum, cross-module enums and options

A `const enum` has no runtime object of its own. Its declaration is erased and each
use, `E.A` or `E["A"]`, is replaced by the member's constant followed by a comment
naming the member, as TypeScript does. The emitter reads the module's tokens again
(including the substitutions inside template literals) and replaces each
occurrence that is not itself a property of something else; an occurrence inside
erased text, such as a type annotation, is dropped with it. A negative or
non-finite value is parenthesized, which TypeScript does not do and which makes
`E.N ** 2` a syntax error in its own output.

The checker adds the restrictions that follow from the object not existing: a bare
`E`, a dynamic or numeric index, and a non-constant initializer are errors, and a
local variable or parameter of the same name is left alone. Two options choose
what "emitted on its own" means. `preserveConstEnums` also emits the object, for a
consumer that reads it at run time. `isolatedModules` (and transpile-only, which has
no types) stops inlining, since another module's values are not assumed known, and
emits the enum as an ordinary one, and an ambient const enum, whose values only a
type check can supply, cannot then be used.

An exported const enum always keeps its object. TypeScript removes the importing
module's import instead, but that needs a specifier-level edit of the import
statement, which has no recorded source span; keeping the object makes every
importer a valid ES module with the import left as written, and the importer still
inlines.

Enums cross modules through the same evaluated members the exporting module uses: an
export map (the module's enums by exported name, including `export { E }`) is built
once per project, an import binds the same four names as a local enum, and an
`import type` binds the types only. Checking this found that a value import of a name
the module does not export was accepted as an untyped value, so the checker now
compares a value import with the module's exported value names (variables,
functions, classes, enums, `export { .. }`, default); value re-exports from another
module are not supported, so that set is closed.

## Checklist

- [x] Decide that BlueTS is a BlueJS front end, not a second VM or a `tsc` runtime process
- [x] Decide direct-AST/IR lowering, no emitted-JavaScript reparse path
- [x] Decide that BlueTSC is a shared-front-end standalone compiler emitting standard JavaScript rather than a second TypeScript implementation
- [x] Decide atomic no-emit-on-error builds, ESM-first output, source-map/declaration artifacts and strict-contract-preserving JS emission
- [x] Decide explicit non-standard BlueIce script kinds and a core-owned policy
- [x] Decide that full static semantics and TypeScript debugger metadata are required for supported direct pages
- [x] Decide `strict-runtime` boundary contracts as the default direct-page safety policy
- [x] Define the initial reifiable-contract boundary and explicit non-reifiable failures
- [x] Define cache keys, resource accounting, debugger metadata and gatekeeper visibility requirements
- [ ] Complete Phase 13 page-script host and Phase 17 source-map prerequisites
- [x] Expose one exact classic-root instruction step through debugger v26 on the in-process page route, retaining the BlueJS continuation and reporting only opaque safe points; source-level mapping remains Phase 13/17 work
- [x] Carry the exact classic-root step through page-host v27 and the supervised child route, including retained BlueTS classic metadata and a public source-free debugger transition; nested-frame and source-level stepping remain Phase 13/17 work
- [x] Add page-host v31 core-only child-wide actual-usage snapshots with checked live-realm aggregation and owner-envelope validation; keep conservative reservations and RSS/fleet limits distinct.
- [x] Expose a bounded classic-root BlueTS source-span step through debugger v30 and metadata manifest v4 with independent owner/client grants, same-stream receipts, core reminting, a distinct limit stop reason, and real launcher debugger-socket acceptance; modules and nested-frame stepping remain open.
- [x] Add the page-host v28 private exact BlueTS safe-point-to-original-byte-span lookup under a live child metadata handle, with no nearest-position fallback or public debugger grant; public source-level breakpoint/stack routing and capability policy remain open.
- [x] Bind the private v28 lookup to core-reminted program and metadata handles, require an exact compiler source ID, and reject mismatched child tuples/ranges before the public debugger adapter can use it; session receipts and owner/client grants remain open.
- [x] Expose debugger v27 `OpaqueSafePointSpan` only with independent owner/client grants, same-stream opaque metadata/source receipts, exact live safe-point validation, and a bounded source-text-free original BlueTS byte range; source-level breakpoint/stack execution and cache/hibernation invalidation remain open.
- [x] Add page-host v29 and debugger v28's separately default-denied bounded BlueTS source-byte-position binding: retain unbound lowering spans beside the live metadata, require exact program/metadata/source receipts and owner/client grants, remint and revalidate bound safe points in core, and return explicit unbound results without source text or automatic execution. General non-root/module interruption and stack/scope mapping remain open.
- [x] Create the standalone `blueice-bluets` crate and pin the initial `blue-ts-0.1` compatibility matrix
- [x] Publish the versioned BlueJS AST/IR hand-off, bytecode-safe-point-map and host-typing compatibility contract
- [x] Implement the first public BlueJS structured-program hand-off for bounded host-neutral classic-script and resolver-preserving ESM-module-graph subsets, without emitted-source reparsing
- [x] Generate and test `lib.blueice.d.ts` from actual host bindings using the published host-typing strategy
- [x] Implement the independent initial parser/binder/closed-module resolver/checker/type-erasure ESM emitter with atomic compile failures
- [x] Implement `bluetsc check`/staged `build`, ESM/column-provenance-source-map/declaration emission and reproducible artifact fingerprints for the initial matrix
- [x] Implement VM-independent `BlueTsDebugInfo` (source hashes, symbols, static types and spans); direct and isolated child page admission retain it only against an exact live BlueJS generation. Compiler/MCP provenance now uses a labeled SHA-256 digest, so the source-text-free compiler identity is collision-resistant; it remains a query-only metadata record, not source-read authority. Page-host v10 can enumerate a separately minted opaque metadata handle for an exact live BlueTS program, describe it with a bounded fingerprint/count summary, list only compiler-minted source-record IDs parent-bound to that handle, and—only under a distinct source-provenance request—describe one prior ID with canonical module identity plus the labeled SHA-256 digest. Debugger v9 defaults to a denied negotiated manifest; an owner must separately enable `OpaqueInventory`, dependent `OpaqueSummary`, dependent `OpaqueSourceInventory`, and/or `OpaqueSourceProvenance`, and a client must request the exact canonical set for the live realm. The public summary is handle/generation-bound and carries only language/options fingerprints plus source/type/symbol/contract counts; source inventory carries only numeric source IDs under that same handle; provenance is source-text-free metadata only. No debugger surface has source text, spans, names, type/symbol/contract records, bytecode, or values. Broader static-record disclosure, diagnostics, bytecode source mapping, and controlled debugger read exposure remain pending.
- [x] Expose source-free, generation-bound BlueTSC diagnostics through compiler IPC v9 and MCP with optional original-source zero-based UTF-16 coordinates, derived only from valid byte spans in the immutable authorized graph; unavailable or invalid positions are omitted. This does not grant source reads, arbitrary offset mapping, build, or output writes.
- [x] Validate core diagnostic evidence again at the MCP boundary before publishing check generations or diagnostic pages: exact project/generation, ordered byte ranges, plausible optional coordinates, and usable one-shot continuation shape are required; a new check revokes old local receipts even if its reply is lost or malformed.
- [ ] Expose TypeScript diagnostics, symbols, types, contracts, lowering provenance and BlueTSC check/build through Phase 12's negotiated MCP debug interface. A core startup owner can now seal fixed closed projects before opening the query-only compiler listener and explicitly attach MCP's bounded v2 client; exact static source-hash provenance plus reifiable-local contract read/validation and exact-generation paged opaque-ID discovery are shipped. Phase 12 authorization, catalog distribution, lowering/bytecode provenance, broader capability/session negotiation, and output-write elevation remain absent.
- [x] Implement the pure runtime-contract IR and bounded JSON-like validator; the only installed page host result boundaries now have an explicit inventory and pre-admission primitive-string validation, while broader host-boundary discovery, JSON Schema delegation and page enforcement remain pending
- [x] Implement host-neutral dependency-aware incremental parser/checker cache invalidation; cache reuse is refused across compiler-policy changes and failed compilations preserve the last successful entry
- [x] Support root-confined, type-only local `.d.ts` modules without runtime emission or package/remote declaration acquisition
- [x] Bound the pure contract validator's depth, collection, node-fuel and string-byte work with caller-visible limits
- [x] Add an opt-in, TypeScript-5.9.3-pinned external-oracle job for the implemented initial matrix
- [ ] Add real-process page, debugger, contract, resource, policy and multi-tab regression coverage

### J.3.4.4 Enum declaration output

An exported enum, or a local one named in `export { .. }`, prints as
`[export ]declare [const ]enum E { .. }`, one member per line: a constant member
as `name = value` (numbers as JavaScript prints them, so `1e+21`, `Infinity`,
`NaN`; strings double-quoted; a non-identifier name quoted), a computed or
uninitialized ambient member by name alone, merged declarations printed
separately, an empty body as `{` newline `}`. The shared evaluator in
`enum_eval.rs` supplies the values, so the declaration and the JavaScript can
never disagree. The declaration does not read the const-enum options; both
options are in the fingerprint, so an incremental session recompiles when one
changes. A local enum can now be the target of `export { Local }`.

### J.3.4.5 Direct runtime for enums

The bridge lowers a runtime enum to one root statement,
`var E = (function (E) { ...; return E; })(E || {});`, the same shape as the
emit path, so merged declarations share one object and imports and exports read
it. The evaluator (now public, `blueice_bluets::evaluate_enums`) supplies the
constants; a computed member keeps its own initializer. A `const enum` is read
through its object, which is observably the same as inlining it; an ambient
`enum` is erased; an ambient `const enum` is refused because its uses would need
inlining. Script routes refuse an exported enum like any export.

### J.3.5.2 Checking namespaces

The checker's maps are flat and keyed by name, so a namespace is checked by
running a second `ModuleChecker` over the body: its module is the body's
declaration list, its maps start as copies of the enclosing scope's with the
names the body declares removed (so a declaration of the same name is not a
redeclaration), and the earlier blocks' exports are added by their bare names.
The body then binds and checks exactly as a module does, including classes,
enums, overloads and nested namespaces, which recurse through the same path.

What the body exports is published into the enclosing checker under keys
qualified from the root: `N.f` for a value, `N.I` for a type, `N.E.A` and
`typeof N.E` for an enum's member and object, `N.Inner.z` for an inner
namespace's value. Every type in a published entry is renamed to its qualified
key, so the entry means the same anywhere; types the body declares but does not
export are published too, since an exported declaration may mention them, and
the registry records them as hidden so a reference by name is refused. Inside a
namespace the entries are also kept under keys relative to it. A namespace is
published when it is bound, with declared types, and again after its body is
checked, because a variable's type is inferred as its declaration is checked.
The namespace's own value `N` is an object type with its exported values, unless
`N` is already a function, class or enum that it merges into.

References are matched by a second parse pass. The first pass learns every
namespace and what it exports; the second merges each chain `N.a.b` whose head
is a namespace in scope and whose every next name is an exported member of the
previous namespace into one identifier token spelled `N.a.b`, spans intact, so
emission is unchanged. Merging stops where the member is not a namespace
member, so a class merged with a namespace still reads its static member
`K.s` as three tokens. The checker then finds `N.a.b` in its maps with no
change to its expression handling.

### J.3.5.3 Emitting namespaces

Emission copies source text and applies recorded edits, so a namespace is
rewritten in place. The header (`export namespace A.B {`) is replaced by the
opening of one function per segment and the closing brace by the matching
closings, each keeping the line breaks it replaced; the body's own text does not
move. Inside, an exported variable's `export const x` is replaced up to the name
by `N.x`, which turns the declaration into an assignment; an exported function or
class loses `export` and is followed, on the same line, by `N.f = f;`, which is
where TypeScript puts it; an enum is rebuilt by the enum emitter with the
namespace's parameter as its home; and an inner namespace recurses with the
current parameter as its parent.

TypeScript reads an exported variable through the object, so a reference to it
inside the body must change. The emitter finds references in the lexed tokens of
the body, skipping text that is erased, replaced or an inner namespace's, a
property after `.`, an object key, a class member's name and a label, expanding a
shorthand property, and descending into template substitutions. An exported
function, class, enum or namespace that another block declared is a reference
too, since it is not a local of this block; one the block declares is local and
left alone.

That is only sound if nothing shadows the name. The emitter therefore collects
what the body binds locally, from the parsed structure (parameters, locals,
catch bindings, nested function expressions, class members) and from the tokens
(declarations in loops, blocks and patterns), and refuses a body in which such a
binding has the name of an exported variable it could see. An exported variable
with several declarators in one statement is refused as well, because the
statement would assign only the first through the object.

### J.3.5.4 Declarations and namespaces across modules

Declaration text for a namespace reuses the module-level printer: the exported
members are printed as if each were a module export, then each first line loses
`export declare` (or `export`, in the explicit form), and an inner namespace
recurses. A member that is not exported is printed when a printed member names
it, found by repeating until no new name appears, and that is exactly when
TypeScript switches to the explicit form.

An importer needs what the exporting module's checker computed, such as variable
types it inferred, so modules are checked in dependency order and each records
`NamespaceExport`: the published entries of its exported namespaces keyed by the
name it declared them under, the registry of members and hidden types, and a copy
of every type of the exporting module that those entries mention, under a key
marked `@module`. Importing renames the namespace's name to the importer's local
one in every key and every type, so `import { Geo as G }` works and a type that
refers to another module's interface still means the same interface.

The references have to be single tokens in the importer too, and the parse of a
module happens before its imports' namespaces are known, so a module that imports
a namespace is parsed again once its dependencies have been parsed, with the
names each one exports.

### J.3.5.5 Namespaces in the direct bridge, and class-interface merging

The bridge lowers from the declaration's own tokens, so a namespace body is
lowered after its declarations have been rewritten: the same analysis the emitter
uses names the identifiers that must be read through the namespace object, and
`rewrite_declaration` writes each as `N . x` in every token vector the lowerer will
see (variable initializers, function and method bodies, field initializers,
statements, enum member values), in the same positions the emitter's text scan
would. The rewrite also writes a qualified name BlueTS merged into one token back
out as names and dots, since the lowerer parses ordinary expression syntax. The
namespace itself becomes the statements TypeScript's emit would run, built as
BlueJS AST: a variable declaration, and a call of a function over the namespace
object; the safe-point map attaches the closures nested under that statement to
the statement's own span.

An interface with the name of a class in one declaration list adds its fields to
the class's instance type: the parser records them on the class
(`merged_interface_fields`) so every consumer of the instance type, including the
inherited surface a derived class sees and an exporting module's class surface,
agrees, and the checker does not define a separate type for the interface. A field
the class declares itself must have the interface's type. The interface is erased
as ever and printed beside the class in a declaration file.

### J.3.6.2 and J.3.6.3 Literal context in properties and assignments

One function reads an expression where a type is expected: it expands named
types, then reads a bracketed literal against a tuple (each element against the
element at its position), a braced literal against a record (each property
against the field of the same name), a bracketed literal against an array (each
element against the item type) and anything against a union by trying its
members in turn. Everything that already asked for context (a variable
initializer, a call argument, a return, a member assignment) now gets all of
these for free, and call arguments try the context for a braced literal as they
did for a bracketed one.

Assigning to a variable had no check at all. A variable declared with an
annotation is remembered (a top-level one when it is bound, a parameter or local
while its function is checked, restored after, with an unannotated name of the
same spelling removed so a shadowing local is not judged by the outer type), and
`name = value` and `array[i] = value` are read against its declared type. An
unannotated variable is never checked: its type is only what the checker
inferred, which an assignment legitimately widens in TypeScript's flow analysis.

### J.3.7.7.4 Generator functions

A generator's checking context has three types: what it may yield, what it may
return and what `yield` evaluates to (what a caller passes to `next`). They come
from the return annotation, which must be `Generator<T, R, N>`, `Iterator`,
`IterableIterator` or `Iterable`; the library types are parsed from a small
declaration text of their own (so `default` type arguments, method signatures and
the `IteratorResult` union need no special model), bound after a module's own
declarations so a local or host `Generator` wins. `yield` is not in the set of
tokens that start an expression statement in a function body, so it was added; the
checker then reads each `yield` in a statement's tokens that is outside a nested
function, requires it to start an expression, takes its operand to the end of the
statement and reads it against the yield type (or, for `yield*`, takes the items of
the operand's iterable type). A `return` in a generator is checked against its return
type through the same path an async function's `Promise<T>` takes, so a generator
whose return type is not `void`, `undefined` or `any` must return on every path.

The emitter changes nothing: the annotations are erased by the ordinary edits and
`function*`, `yield` and `yield*` are in the copied text, which ES2020 and ES2022
run. The bridge's expression parser handles `yield` at assignment precedence, and
its function lowering sets BlueJS's `generator` flag.

### J.4.1 Module system selection and CommonJS

`CompilerOptions` gained `module_kind` (`Esm` by default, or `CommonJs`) and
`es_module_interop`; both are in the project fingerprint, the `bluetsc` CLI
(`--module esnext|commonjs`, `--es-module-interop`), its config file (`module`,
`esModuleInterop`) and the build manifest (`module`, `esModuleInterop`), so an
artifact is never reused under a different module system.

**Syntax and checking.** `import x = require("m")` parses to an import with one
`export=` binding (`equals_require`), and `export = name;` to a value export with
the one binding `export=` (`export_assignment`); `export = a.b`, `import a = b.c`
and a non-string `require` argument stay explicit refusals. In an ECMAScript module
both are errors that name `--module commonjs`; in CommonJS an `export =` module
cannot also have other exports, a default import of an `export =` module is an
error without `esModuleInterop` (and binds the whole export with it), and
top-level `await` is refused. A cycle between modules is a valid graph in CommonJS
(`require` returns the partial exports of a module still loading) and keeps being
refused for ECMAScript modules.

**Emit.** `emitter/commonjs.rs` rewrites ES syntax to TypeScript's CommonJS shape
by text edits that keep every line: `"use strict"` and the `__esModule` marker,
the `exports.a = exports.b = void 0` key chain, `const m_1 = require("./m.js")`
per import (a default import under interop goes through `__importDefault`, a
namespace import through `__importStar`), `exports.f = f` for hoisted functions,
`exports.C = C` after a class, `exports.E = E = {}` inside an enum or namespace
IIFE, `exports.x = ..` for an exported variable, and `module.exports = x` for
`export =`. A use of an imported name reads a member of its required module (a
call as `(0, m_1.f)(..)`, so `this` is not the module) and a use of an exported
variable reads `exports.x`, through the same reference scan namespaces use
(`Replacement`); the shadow refusal is the shared `refuse_shadowing_of`. Declaration
output prints `export = x;`. The pinned `tsc --module commonjs` and BlueTSC are run
under Node on six programs (live bindings, cycles, `export =`/`import = require`,
interop default and namespace imports, `this` of an imported call) and must print
the same thing (`tests/commonjs_oracle.rs`, ignored test).

**Direct runtime route.** The direct bridge executes ECMAScript scripts and modules
only; with `module_kind: CommonJs` all three `compile_direct_*` functions refuse,
naming the supported route: `--module commonjs` output run in a realm with a
host-provided CommonJS loader. The page gets no `require`, `module` or `exports`
from CommonJS syntax alone, since no loader is provisioned for it.

Recorded gaps: a module imported by a bare or extensionless specifier (that is
J.4.2); the type of a value imported from another module is still `unknown` in the
checker (also for ESM), so a declaration file prints `unknown` for a variable
inferred from one; reassigning a local listed in `export { local as x }` is not
tracked after the statement; `export default <expression>` and re-exports remain
out of the matrix; an exported variable with several declarators is refused.

### J.4.2 and J.4.3 Installed packages: resolution, canonical roots, fingerprints

`package_resolution.rs` resolves a bare specifier the way TypeScript 5.9.3 does,
over an injectable `PackageFs` (the real disk is `OsPackageFs`; the unit tests use an
in-memory tree with symlinks). The owner's configuration is `moduleResolution`
(`node10`, `node16`/`nodenext`, `bundler`), optional `packageRoots` and
`customConditions` in `bluetsc.json` only; with no `moduleResolution` a bare
specifier is refused exactly as before, and the single-entry flag mode has no way
to turn the feature on.

**Lookup.** For each ancestor of the importing file that is inside an authorized
root, nearest first, `node_modules/<name>` and then `node_modules/@types/<mangled>`
(`@scope/pkg` is `@types/scope__pkg`). `node10` reads `typings`, `types`, `main`,
then `index`; a subpath is a file, then a directory with its own manifest fields,
then `index`. `node16` and `bundler` use `exports` when the package has one and then
use nothing else (no `main`, no loose files): conditions are `types`, `node` (node16
only), `import` or `require` by the importing module system, and the owner's
`customConditions`; the entry is the exact key or the longest-prefix `*` pattern;
arrays and condition objects are an ordered list of candidate files, the first that
exists winning, as TypeScript does, with `null` ending the list as a block; `#imports`
resolve through the nearest manifest. A `.js`/`.jsx` target stands for its `.ts`,
`.tsx` or `.d.ts`; a JavaScript-only package is the explicit `JavaScriptOnly` error.
`exports` is read with an order-preserving JSON value, since a `serde_json::Value`
sorts keys and would pick the wrong condition.

**Roots and symlinks (J.4.3).** The roots are canonical directories (the project root
plus the config's `packageRoots`). The search stops at them; a package directory is
canonicalized before anything inside it is read; a manifest field that would climb out
of its package is skipped without being probed; a found file is canonicalized and must
be inside a root. A package, `node_modules` directory or file reached through a symlink
that leaves the roots is `OutsideRoots`, an error rather than a reason to try a farther
candidate, so a link cannot redirect a lookup to something the owner did not
authorize, and a link that stays inside is followed to its real path (pnpm layouts).
Nothing is installed or fetched: a missing package is `NotFound` and says so.

**Module identity and emit.** A file under a root is the module id `<path from the
project root>`; one under a `packageRoots` entry is `@external/<n>/<path>`, and one reached
by a symlink to a directory without `node_modules` in its path is `@package/<path>`.
`is_external_library_module` (a `node_modules` segment or either prefix) marks a
module as an installed package's: it is checked and its types are used but, as with
TypeScript's external-library files, it is neither emitted nor copied as a
declaration, and the importing program keeps the package's own specifier, which the
host that runs the output resolves. A value import of such a declaration module is
allowed (a project or host-supplied `.d.ts` still refuses it as type-only). The direct
bridge links no packages: a runtime import resolving to one is an explicit refusal
naming the `bluetsc build` route.

**Fingerprints and invalidation.** The resolver records every `package.json` it read
(with its SHA-256), every candidate it probed and found absent, and each directory's
canonical target. `fingerprint()` hashes that with the TypeScript resolution version, the
strategy, conditions and roots; `revalidate()` re-checks each observation, so a nearer
package appearing, a manifest edit, a repointed symlink, or a better-ranked file appearing
makes it `false` and the host must discard what depended on it. The configuration part is
in `resolver_fingerprint` (so in the project fingerprint) before compilation, and the
manifest's `packageResolution` records the version, strategy, conditions, number of
external roots, the resolved packages (name, version, whether from `@types`) and the
full observation fingerprint after it. Tests: 30 in-memory resolver tests, 5 disk tests
(real symlinks, escapes, traversal-shaped specifiers, invalidation), and 10 `bluetsc`
tests, including a CommonJS project that imports an installed package and a
`export =` module and runs under Node.

Recorded gaps: the symlink tests are Unix-only (the code uses only `std::fs::canonicalize`
and `Path::starts_with`, so Windows verbatim prefixes compare consistently, but it has
not been run there); a case-insensitive file system relies on `canonicalize` returning
one spelling; `.d.mts`/`.d.cts`/`.mts`/`.cts` entries, `typesVersions`, the package's
self-name import, `paths`/`baseUrl` and automatic `@types` inclusion are not resolved;
real-world declaration files that use syntax BlueTS's declaration parser does not
accept are refused at parse time, not skipped.

### J.4.4 Remote declaration sources

`remote_declarations.rs` and the `bluetsc` config `remoteDeclarations` (each
`{ specifier, url, sha256 }`) plus `declarationCache` (default `.bluetsc/declarations`,
inside the project root). The owner pins the SHA-256 of exactly the bytes allowed.

**Nothing in a program fetches.** `check` and `build` never open a socket: they read
only the content-addressed cache (`<sha256>.d.ts`), re-hash each entry on every read
(a tampered file is `CorruptCache`, never trusted or replaced) and, when an entry is
missing, say to run `bluetsc fetch-declarations`. That command is the only network
step; it takes only `--config`, requests only the owner's listed URLs, and the
`ureq` fetcher allows `https` only, follows no redirect, has a 30-second total
timeout and stops reading one byte past the size limit. Source text cannot trigger a
fetch: a source importing `https://...` is refused as a bare specifier by resolution,
and a pinned declaration's `/// <reference>` lines and `import("https://..")` types are
text, never requests (tested offline). The URL list is validated when the config is
read (https, a host, no credentials, fragment, spaces or control characters; a bare
specifier; a 64-hex pin; unique specifiers).

**Bounds.** 1 MiB per source, 16 sources, 8 MiB of cache by default (`RemoteLimits`); a
fetched body is checked against the size, the pin and UTF-8 before it is written, and is
written to a temporary file and renamed, so a mismatched or partial download never
appears in the cache.

**Identity and authority.** The module id is `@remote/<sha256>.d.ts`, resolved only
for an exact configured specifier; the pin is in the id, the manifest
(`remoteDeclarations`) and, with the specifiers and URLs, in `resolver_fingerprint`, so a
different pin is a different project fingerprint. A remote declaration cannot import
anything (its resolutions are refused as not self-contained), is checked and used for
types but not emitted or copied (`is_external_library_module`), and adds no ambient
scope: a `declare function` in it does not become a callable global (checked with
`require_declared_global_calls`), and the importing program keeps the plain specifier for
whatever runtime the owner provides. The direct bridge refuses a runtime import of it
(`links no installed packages`), so a page gets no host API from a declaration.

Recorded gaps: an unknown identifier is still not diagnosed (a declared `const` that was
not imported is not shown to be absent), the real network path is exercised only for its
failure and validation behaviour (no HTTPS server in tests), and the cache has no
eviction (a full cache is an error the owner resolves).

### J.5.1 JSX: parsing and checking

**Lexing.** In a `.tsx` module the lexer turns a whole outermost element into one
`JsxElement` token (`syntax.rs`, `jsx.rs`), so the statement parser and the checker's
token-slice inference see one operand. A `<` starts an element after a token that can
precede an expression (`(`, `,`, `=`, `=>`, `?`, `&&`, `return`, ...), or, after `:`, `;`,
`{`, `}` or at the start, when the text scans as one; `<T,>` and `<T extends U>` are
generic arrows, as in TypeScript, and `<=`/`<<` are operators. An embedded `{ expression }`
is lexed by the same lexer (so strings, templates, braces and nested elements inside it
are right), and its tokens follow the element token as the arguments of a synthetic
`( ... , ... )` group: arrow functions, erased annotations and (later) name rewriting
inside JSX expressions are therefore reached by every pass that walks tokens, with
absolute offsets. The element's structure (names, attributes, spreads, children, text,
fragments, attribute values that are elements) is a tree of source ranges
(`jsx::parse_element`), rebuilt from the token's source by whoever needs it. The scan is
linear in the element's size (each embedded expression is lexed once, carrying its tokens
in the tree), nesting is bounded at 128, and a malformed element (mismatched or
unterminated tag, a bare `>` or `}` in text, an empty attribute value) is a parse
diagnostic. JSX text trimming and entity decoding are TypeScript 5.9.3's, with the entity
table generated from the pinned compiler.

**Checking** (`checker/module/jsx.rs`). With no `jsx` option any element is an error, as
TS17004. An intrinsic tag takes its props from `JSX.IntrinsicElements` (unknown tag or no
interface is an error); a function component's first parameter is its props and its
return must be assignable to `JSX.Element | null`; a class component's props are the
member `JSX.ElementAttributesProperty` names (and the class must satisfy
`JSX.ElementClass`); attributes are checked for value type (in the prop's context, so
literal unions and arrows work), unknown names (hyphenated names are never checked),
missing required props, `{...spread}` of an object type, and children against
`JSX.ElementChildrenAttribute`'s member (one child is its type, several an array; having
children when the props declare none is an error). `JSX.IntrinsicAttributes` (`key`) applies to
value-based elements only, as in `tsc`. The `JSX` namespace is looked up as
`<factory root>.JSX` first and then globally, so an imported factory namespace
(`import * as React`) works; that required two general fixes found on the way:
`import * as ns` now binds the module's types and namespaces as `ns.T` and `ns.N.T` (it
bound neither), and a function type with a `void` result accepts a function returning
anything. Classic mode requires the factory (and fragment factory) root to be in scope,
from `jsxFactory`/`jsxFragmentFactory` or an `@jsx`/`@jsxFrag` pragma (read from the file's
leading comments). Every embedded expression is checked as an ordinary expression.

Evidence: a 53-fixture accepted/rejected matrix recorded from the pinned compiler
(`jsx-checker-matrix.tsv`; regenerate with `BLUEICE_WRITE_JSX_MATRIX=1`) on which BlueTSC's
verdict agrees for every entry, plus `tests/jsx.rs` (option off, lexing corners, bounds,
pragmas, expressions) and the `jsx.rs` unit tests. Fixtures are global scripts, because a
module-scoped `declare namespace JSX` is not found by `tsc` either.

Recorded gaps: type arguments on a tag (`<Foo<string> />`); generic components and props
that are unions or intersections are resolved but not compared (`Props::Open`); the
automatic runtime's `JSX` namespace comes from `<jsxImportSource>/jsx-runtime` in
TypeScript and is not read from a package yet, so the global or factory namespace is
used; `JSX.ElementType`, `LibraryManagedAttributes`, `IntrinsicClassAttributes`,
`defaultProps`, duplicate-attribute and `children`-specified-twice diagnostics,
namespaced-attribute typing; a parameter named `type` in a function declaration does not
parse (found while writing fixtures; unrelated to JSX).

### J.5.2 JSX emit and direct execution

`CompilerOptions` gained `jsx` (`preserve`, `react-native`, `react`, `react-jsx`,
`react-jsxdev`), `jsx_factory`, `jsx_fragment_factory` and `jsx_import_source`: all in the
project fingerprint, the `bluetsc` flags (`--jsx`, `--jsx-factory`,
`--jsx-fragment-factory`, `--jsx-import-source`), the config (`jsx`, `jsxFactory`,
`jsxFragmentFactory`, `jsxImportSource`) and the manifest. `@jsx`, `@jsxFrag`,
`@jsxImportSource` and `@jsxRuntime` pragmas in a file's leading comments override them for
that file, as in TypeScript.

**Emit** (`emitter/jsx.rs`), TypeScript 5.9.3's JSX transform read from the pinned compiler:
- *classic* (`react`): `factory(tag, props | null, ...children)`, the fragment factory as the
  tag of a fragment, an intrinsic tag as a string and a value tag as an expression;
- *automatic* (`react-jsx`, `react-jsxdev`): `jsx`/`jsxs`/`jsxDEV` imported from
  `<jsxImportSource>/jsx-runtime` (or `/jsx-dev-runtime`), children in the props (one child
  as is, several as an array and `jsxs`), `key` as the third argument, `createElement`
  imported from the source itself when a `key` follows a non-literal spread, and for
  `jsxDEV` the `void 0`, static-children flag, `{ fileName, lineNumber, columnNumber }` and
  `this` arguments with a `_jsxFileName` constant;
- *preserve* and *react-native* keep the JSX text (the output is `.jsx` for `preserve`, `.js`
  for `react-native`, and imports of a `.tsx` module are written to match).
Attribute names are bare when they match `[A-Z_]\w*` and quoted otherwise, string attributes
keep their quote style and are entity-decoded, text children follow the whitespace and
entity rules of `jsx::text_value`, `{...x}` is a spread, and an expression with a top-level
comma is parenthesized.

Only the JSX syntax is rewritten. Each embedded expression is left in place in the source
(an `Item::Keep`) and the gaps between them are replaced by edits that keep their line
breaks, so every emitted line is its source line, and every other pass (erased annotations,
CommonJS name rewriting, namespaces) reaches the expressions through the flattened token
group. The tag and the factory root are rewritten by the same reference map the CommonJS
lowering uses (`react_1.default.createElement`, `(0, factory_1.default)(...)`). The runtime
import is inserted after the directive prologue as an `import` in a module and as
`const jsx_runtime_1 = require(..)` (with `(0, jsx_runtime_1.jsx)(..)` calls) in CommonJS. A
module that already uses `_jsx`, `_jsxs`, `_jsxDEV`, `_Fragment`, `_createElement` or
`_jsxFileName` is refused rather than risk a collision. A moved `key` is written out as text,
not kept in place (it comes after the props in the output).

**Parity.** `tests/jsx_oracle.rs` builds 11 programs with BlueTSC and the pinned `tsc` (classic
and automatic, dev, ESM and CommonJS, the options and each pragma, an imported factory) and
requires identical output under Node with a recording stand-in for the runtime; the programs
cover text trimming, entities, spreads, `key` placement, fragments, member tags, namespaced
and hyphenated attributes, nested elements in attributes and expressions, and the dev
line/column positions.

**Direct execution.** `compile_direct_*` lowers a classic-mode element to a BlueJS call of the
program's own factory, built as AST from the same tree and rules (`jsx_direct.rs`); the
factory is an ordinary identifier of the program (JSX syntax imports nothing and grants no
host API), and a classic program whose factory is not in scope is refused at check time. The
automatic runtime (it imports a runtime module the bridge does not link), `preserve` and a
missing `jsx` option are refused naming the supported route. The same programs run under
BlueJS and Node (via the pinned `tsc` output) and complete with the same value
(`bluets-bluejs/tests/jsx_direct.rs`).

Recorded gaps: source-map and debugger mappings for JSX are the coarse ones every edit
gets (the replaced gap maps to its source span; no per-attribute mapping), so a breakpoint
inside an element tag is not bound; a JSX element under `strict-runtime` follows that
policy's existing refusal of everything but its one string-boundary shape; the direct bridge
lowers embedded expressions with the existing expression subset (an arrow function inside a
JSX expression is as limited as an arrow anywhere else there); namespace-exported names used
as tags inside another namespace body are not rewritten.

### J.5.3 Standard decorators

`@expression` decorators on classes and on methods, accessors, fields and `accessor`
(auto-accessor) members parse into `Decorator`s (the expression's tokens, `a`, `a.b`,
`a(..)`, `a.b(..)` or `(expr)`), with `ClassField::accessor` for `accessor x`. The
expression is an ordinary expression: its annotations are erased and it is checked.
Decorators on anything else (a function, variable, parameter, constructor, static block, an
overload signature, an interface member, or a member form the class parser does not
structure) are errors, matching `tsc`. A decorator with no parameter is `TS1329`-style
refused (`tsc`'s arity rule, probed: zero parameters rejected, one or more accepted); one
requiring more than two, a non-callable decorator and a decorator whose return type cannot
be what the target accepts (a function or nothing for a class, method, getter, setter and
field; an object for an `accessor`) are refused. BlueTS has no `lib.decorators.d.ts`, so the
context argument is untyped.

**Emit** (`emitter/decorators.rs`) is TypeScript 5.9.3's ES2022 shape, from the pinned compiler: the
decorated class becomes `let C = (() => { let _classDecorators = [..]; ... var C = class {
static { _classThis = this; } static { <metadata, decorator arrays, __esDecorate calls in
TypeScript's order: static non-fields, instance non-fields, static fields, instance fields,
then the class; Symbol.metadata> } <members> ... }; return C = _classThis; })();`. Evaluation
order is source order (class decorators, then each member's), application order is TypeScript's
(last decorator of an element first; members before the class), `Symbol.metadata` is
inherited through the `extends` clause, fields run the initializers decorators returned and
queue the extra initializers `addInitializer` collected, and an undecorated `accessor` is a
private backing field with a getter and setter. The helpers (`__bluetsEsDecorate`,
`__bluetsRunInitializers`) are the specified algorithms of the proposal, written here,
versioned (`bluets-decorator-helper-v1`, in the fingerprint and the manifest). Only the
class syntax is rewritten: a decorator expression is moved into its array by a *relocation*
(`apply_edits` expands `\0M<start>,<end>\0` markers to the text of that range with the edits
inside it applied, so CommonJS name rewriting and erased annotations follow it), a field's
initializer stays where it is and is wrapped in place, and every source line stays a line. A
dotted decorator (`@a.b`) is bound to its receiver as TypeScript does. CommonJS output
carries `exports.C = C`.

Evidence: a 31-fixture accepted/rejected matrix recorded from the pinned compiler
(`decorators-checker-matrix.tsv`), and `tests/decorators_oracle.rs`: 20 programs (all member
kinds, class replacement and `addInitializer`, evaluation/application order, `Symbol.metadata`
inheritance, the `access` objects, extra-initializer injection with and without fields and
constructors, undecorated auto-accessors, `export` forms, and seven programs whose class
definition throws, compared by error name and message) printed identically by BlueTSC and the
pinned `tsc` output, in ES module and CommonJS output. Two general checker defects found on the
way are fixed: a function whose return type is a function type was checked against the
return type's parameters at its call sites, and `import * as ns` now binds the module's
types and namespaces.

**Direct execution.** BlueJS implements the standard decorators itself (Phase 13), so the
bridge passes the decorator expressions (and `accessor`) through to the BlueJS class AST and
needs no helper; programs are compared with Node under `tsc`'s output
(`bluets-bluejs/tests/decorators_direct.rs`). The bridge's expression subset has no arrow
functions, so decorators there are function declarations.

Recorded gaps: the target must be ES2022 with class fields defined (other targets are refused
with that message; `tsc` lowers the fields too); a decorated private method or accessor,
computed or literal member names, a decorated class or auto-accessor in a namespace, `super` in a
static member of a decorated class and a decorated class expression are refused; the
decorator context and `Symbol.metadata` have no types; debugger mappings for the generated
helper frames are the coarse ones every edit gets.

### J.5.4 Legacy decorators and decorator metadata

`experimental_decorators` (`--experimental-decorators`, config `experimentalDecorators`) selects
TypeScript's legacy decorators and `emit_decorator_metadata` (`--emit-decorator-metadata`,
`emitDecoratorMetadata`, which requires the former, as in `tsc`) adds metadata; both are in the
fingerprint, the manifest and the options, and the helper text is versioned
(`bluets-legacy-decorator-helper-v1`). This is a separate mode from J.5.3, with separate
parsing of one new construct (parameter decorators `m(@d a: number)`, which are an error
without the flag), a separate checker and a separate emitter (`emitter/legacy_decorators.rs`),
and the standard helpers are never emitted in it.

**Semantics (the pinned compiler's emit, not the proposal's).** Nothing is wrapped: the class
is defined as written (a class with class or constructor-parameter decorators as
`let C = class C { .. };`) and decorators are applied by calls after it: one
`__decorate([..], C.prototype | C, "name", null | void 0)` per decorated element, every
decorated instance member in source order, then every static member, then
`C = __decorate([class decorators, __param(i, d) of the constructor], C)`. An element's list is its
decorators, its `__param(i, d)` entries and, with metadata, `design:type`,
`design:paramtypes` and `design:returntype`. Decorators run last to first through the helpers,
which are the specified behavior of `Reflect.decorate` and its fallback; a method or
accessor decorator may return a descriptor and a class decorator a replacement class.
A getter/setter pair is decorated once (decorators on both are an error, `TS1207`) and its metadata
describes the pair (the getter's type, the setter's parameters). Because the members are not
rewritten, every field semantic and target the class lowering supports works unchanged,
verified on ES2022 and ES2020.

**Checking.** TypeScript resolves the decorator call, so the arity is checked against the
arguments each target receives (probed: class 1, property 2, method or accessor 2 or 3,
parameter 3; optional and rest parameters count), a property or parameter decorator must return
nothing, and the other kinds may not return a primitive. Placement errors, a decorator on a
constructor, an overload, an `accessor` member or a decorated private name, are errors.

**Metadata** serializes the types BlueTS resolves, following `serializeTypeNode`: `number`,
`string`, `boolean` and their literals, `Array` for arrays and tuples, `Function`,
`Object` for interfaces, records, `any`, `unknown`, `object` and mixed unions, `void 0` for
`void`/`undefined`/`null`/`never`, `null` and `undefined` dropped from a union, aliases followed,
numeric and string enums, `Symbol`, `BigInt`, `Promise`, and a class declared in the module by
name. A type it cannot resolve (an imported class, which TypeScript guards with a `typeof`
check and a hoisted temporary) and a method without a return type annotation are refused rather
than guessed.

Evidence: a 19-fixture checker matrix recorded from the pinned compiler
(`legacy-decorators-checker-matrix.tsv`) on which BlueTSC agrees for every entry, and
`tests/legacy_decorators_oracle.rs`: 18 program/module/target combinations (order of
evaluation and application for class, member and parameter decorators, replacement of the class,
methods, accessors and fields, inheritance, exports, and 5 programs with metadata over a recording
`Reflect.metadata`), each printed identically by BlueTSC and the pinned `tsc` output under Node
for ES2022 and ES2020 and ES module and CommonJS output.

**Direct execution.** BlueJS implements the standard decorators only, so a program compiled with
`experimentalDecorators` that has decorators is refused by the direct bridge, naming
`bluetsc build --experimental-decorators` as the route.

Recorded gaps: metadata for an imported or unresolved class type, an unannotated method return
type, `design:*` for decorated getters/setters whose pair has no annotations, and decorated
classes inside namespaces or `export default @d class` are refused; return-type compatibility of
a replacement (a method decorator's descriptor, a class decorator's class) with the decorated
element is not checked beyond primitives; `emitDecoratorMetadata` types that need `typeof`
guards are the only metadata form not byte-compatible with TypeScript.

### J.6 Compatibility closure (state recorded 2026-10-02)

**J.6.1 — inventory.** `COMPATIBILITY_INVENTORY.md` is the inventory: what is compared with the pinned
`tsc` and how (section 1), what the subset is (2), every open gap by area with a stable ID (3: type system G-T1–T10,
emit G-E1–E5, declarations G-D1–D2, modules and projects G-M1–M3, diagnostics and CLI G-C1–C3, the direct runtime
profile) and the 162 refusal sites the compiler itself knows (4, generated by `tools/inventory_refusals.py`). The
inventory is complete as a list; closing the gaps in it is not done: they are required for a full-parity
claim and several (G-T1–T4, G-T7, G-M2, G-C1) are each a large piece of `tsc` itself. **Therefore the full-parity
claim is not advertised**, and the README/PLAN wording stays "a checked subset, differentially tested".

**J.6.2 — differential suites.** All 19 differential suites of the two crates (every `#[ignore]` test: the accepted/rejected
matrices, the emit-and-run oracles, declarations, the option-combination oracle added here, the direct-runtime
parity tests) pass on macOS 26 / Apple silicon with the pinned `typescript@5.9.3` and Node 26, none skipped, and CI
(`typescript-oracle` job) now runs every ignored test of both crates, not only `typescript_oracle`, on Ubuntu 24.04 and macOS 15.
Windows is not in that matrix (G-M3), so "all applicable platforms" is met for the platforms the harness supports and not for
Windows. A real defect found by the new suite is fixed: CommonJS name rewriting ran after the lowerings that move source text, so
a static initializer moved after the class (ES2020) or a decorator expression moved into its array kept an imported name unrewritten
(`Base is not defined` at run time); the rewriting now runs first and the relocation of moved text keeps edits inside it.

**J.6.3 — workspace gates.** On the same disk-budgeted target directory (`cargo clippy --workspace --all-targets -- -D warnings`
clean, `cargo fmt --check` clean, `cargo test --workspace --no-fail-fast`: 360 suites, one failure, the launcher test
`scope_relations::launcher_relates_nested_parent_roots_but_not_child_local_slots`, whose launcher process exited at start-up
under the load of 33 parallel processes; it passes alone and its whole suite passes 33 of 33, so it is recorded as a load flake,
not fixed). Coverage of `blueice-bluets` and `blueice-bluets-bluejs`, the crates J.4 to J.6 changed:
`cargo llvm-cov --fail-under-lines 90` passes with 92.23% lines, 94.83% functions and 92.17% regions
(the 2026-10-02 run needed `LLVM_COV`/`LLVM_PROFDATA` set to the pinned 1.95.0 toolchain's tools because
cargo-llvm-cov looked in the Homebrew toolchain). The workspace-wide aggregate was not re-measured: the other crates are unchanged.


### K.1.1 Lexical names, hoisting and initialization order

The checker now builds one lexical scope tree before resolving uses. Module, function,
parameter, block, loop, catch, class, static-block, namespace and enum scopes share value/type
lookup; declaration names and property keys are distinguished from references. `var` and
function declarations hoist to their declaration boundary. `let`, `const` and class references
before initialization report `BTS3005` with TS2448/TS2449 in the message. An execution identity
allows a deferred function to capture a later outer binding, while immediate initializers and
parameter defaults retain their declaration order. A default parameter cannot see body locals.

The parser records original spans for named types in annotations and erased assertions, plus
`typeof` value queries. Query aliases carry source positions internally so shadowed parameters
and outer variables keep distinct types; declaration text retains the source spelling. Namespace
exports are shared across merged bodies in both directions, private members remain local to one
body, and private nested namespaces retain separate identities. A namespace is a qualifier for
its member types, not a standalone type; a type-only body supplies no value for `typeof`. A
namespace merged into a class retains that class's initialization point. Import value/type meanings
come from the sealed project graph and existing owner declarations. Source-offset indexes keep type
lookup and erased-token exclusion bounded by the parsed source instead of scanning all scopes
or edits for each identifier. Contextual keywords such as `any` and `of` also participate in
value lookup; the for-of separator, labels and property names retain their syntactic roles.
Named-type validation lives in `checker/module/binding/names.rs`; new scope sources are split by
construction and expression traversal.

Evidence is `unknown_name_checker_matrix.rs`: 108 pinned TypeScript 5.9.3 verdicts replayed through
both public CLI commands, including rejection without output, plus execution and exact `.d.ts`
comparison under Node. `tests/names.rs` checks original identifier spans, shadowed query types,
parameter/body separation, contextual keywords used as values, owner call-policy enforcement and
the explicit unchecked `TranspileOnly` route. Existing diagnostic tests now expect the identifier
token for unknown types and the initialization-order code for a forward class base; formerly undeclared assertion
examples receive an explicit ambient declaration while preserving their expression assertions.

The K.0 gate passed on 2026-10-05: `cargo fmt --all -- --check`, all-target Clippy for
`blueice-bluets` and `blueice-bluets-bluejs` with `-D warnings`, and
`BLUEICE_BLUETSC_ORACLE=<tsc 5.9.3> FORCE_COLOR=0 cargo test -p blueice-bluets
-p blueice-bluets-bluejs -j 4 --no-fail-fast -- --include-ignored --test-threads=4`.
The two-crate run passed 993 tests, including all 102 ignored oracle tests in 20 suite files,
with no failures, ignored tests or filtered tests. No accepted entry in the existing matrices
became a new refusal. The refusal inventory was regenerated from the final source.

Recorded boundaries: the finite ECMAScript/console list recognizes static names only; K.1.4 still
owns a versioned standard library and its types. It adds no DOM name and no runtime or host grant,
and `require_declared_global_calls` still requires the owner's declarations. Imported value
inference, immutable assignment enforcement, broader query/operator typing and TypeScript
numeric diagnostic parity remain K.1.3, K.1.2, K.4 and K.3 respectively. Full parity stays off.


### K.1.2 Immutable bindings and member writes

Each lexical binding now retains mutability independently of its inferred type. Writes resolve
against that binding's scope identity, so a writable parameter, `let`, `var` or catch binding can
shadow an outer `const` or import. Constant/import/function/class/enum/namespace rebinding reports
`BTS3006`, with the corresponding TS2588/TS2632-family code in the message and the original written
identifier as the diagnostic span. An imported object's mutable contents remain writable; a
namespace import's immediate exported properties remain immutable. Namespace exports share their
mutability across reopened bodies, including nested namespaces. Qualified tokens merged by the
namespace parser are expanded back to the original lexer tokens for target diagnostics. Raw
lexer tokens join an adjacent private marker and identifier into `#name`, matching the parser's
runtime token representation and preserving the complete private identifier span.

Assignment, every compound assignment, prefix/postfix updates, recursive array/object/rest/default
destructuring targets, implicit for-of/for-in writes and writes in nested functions, class fields,
methods, constructors and static blocks use the same target scan. Pattern declaration names are
excluded from writes and reads; computed keys and default RHS expressions remain reads. Loop scopes
survive object literals in their source expression and end with the loop, including an unbraced body.
The contextual word `of` remains a valid declared name as well as a for-of separator.

Readonly destructuring and iteration look through declared records, class surfaces, private/protected
markers, nested members, literal computed keys, arrays, tuples and aliases. Restricted receiver
fields retain their declared types during mutation lookup; ordinary accessibility checking remains
separate. ECMAScript private names select their lexical declaring class, so equal spellings in a base
and derived class do not share mutability. A string key with the same spelling is an ordinary
property and does not select a private symbol. Bounded initializer
references recover a receiver's declared type when its local declaration was inferred. Only the
immediate constructor of the declaring class may initialize its own readonly instance fields, using
plain/compound assignment, updates or destructuring; inherited fields, nested closures and other
receivers keep the readonly restriction. Existing direct and nested property mutation checks retain
their conservative refusals. Property helpers are shared in `checker/properties.rs`; target grammar
and lexical mutation checks live in separate scope modules, each below 650 lines.

Target scans cap their token traversal at `max(max_type_expansions * 32, 64)`, retained mutation
leaves at `max(max_type_expansions * 16, 256)`, and pattern/receiver/alias recursion at 128. Type
lookup uses the compiler's shared expansion budget. Exceeding a bound reports `ResourceLimit` and
prevents output rather than silently accepting an unchecked write. Owner-supplied ambient constants
retain their binding kind. `TranspileOnly` keeps its explicit unchecked behavior; no source loading,
network or runtime authority changes.

Evidence: `immutable_checker_matrix.rs` records 173 pinned TypeScript 5.9.3 verdicts (75 accepted,
98 rejected), replayed through `check` and `build`; the runtime program compares Node output and
exact `.d.ts` text. `tests/immutables.rs` adds seven public-boundary tests for diagnostic positions,
shadowing, namespace exports, readonly targets, constructor permission, ambient ownership,
transpile policy and exhausted budgets. The initial 130-case failing replay was committed before
implementation; additional receiver cases were recorded and shown to fail before their fixes.

The final K.1.2 gate passed on 2026-10-05: `cargo fmt --all -- --check`, all-target
Clippy for `blueice-bluets` and `blueice-bluets-bluejs` with `-D warnings`, and
`BLUEICE_BLUETSC_ORACLE=<tsc 5.9.3> FORCE_COLOR=0 cargo test -p blueice-bluets
-p blueice-bluets-bluejs -j 4 --no-fail-fast -- --include-ignored --test-threads=4`.
The 51 test targets passed 1004 tests, including all 104 ignored oracle tests in 21 suite files,
with no failures, ignored tests or filtered tests. Existing accepted entries remain accepted.
The unbraced-loop regression from K.1.1 is covered by its unchanged name matrix. The refusal
inventory was regenerated from the final source; imported value typing and the versioned
standard library remain K.1.3 and K.1.4. Full parity stays off.

The six sources listed in K.1.R.1 are still at their audited sizes. After this verified feature
commit and push, split those sources and run a separate K.0 gate before K.1.3 adds responsibilities.


### K.1.R.1 Split near-limit compiler and direct-runtime sources

After the verified K.1.2 feature commit and push, the six audited production sources were split
by responsibility. Import/export and module-system validation now live in `binding/modules.rs`;
class body checking and type/surface helpers live in `classes/bodies.rs` and `classes/types.rs`;
inherited signature and rest-parameter helpers live in `overrides/signatures.rs`. The CLI keeps
execution and artifact publication in `bluetsc.rs`, with arguments and owner configuration in
`bluetsc/config.rs`. The direct bridge separates calls/construction and writes in `expression/`,
and class/decorator lowering from namespace/enum lowering in `lowering/`.

The split moves complete items, preserves source headers, and changes only module declarations,
imports and the visibility needed for the original callers. No parser/checker/emitter/runtime
behavior or authority changes, and existing public-boundary and pinned-oracle coverage remains
the validation boundary.

| Original source | Before | After | New child modules |
| --- | ---: | ---: | --- |
| `checker/module/binding.rs` | 1205 | 759 | `modules.rs` (474) |
| `checker/module/binding/classes.rs` | 1520 | 963 | `bodies.rs` (364), `types.rs` (241) |
| `checker/module/binding/classes/overrides.rs` | 1487 | 998 | `signatures.rs` (509) |
| `bin/bluetsc.rs` | 1693 | 1021 | `bluetsc/config.rs` (687) |
| `bluets-bluejs/src/expression.rs` | 1234 | 874 | `calls.rs` (249), `writes.rs` (139) |
| `bluets-bluejs/src/lowering.rs` | 1340 | 698 | `classes.rs` (292), `namespaces.rs` (385) |


The K.1.R.1 gate passed on 2026-10-05: `cargo fmt --all -- --check`, all-target
Clippy for `blueice-bluets` and `blueice-bluets-bluejs` with `-D warnings`, and
`BLUEICE_BLUETSC_ORACLE=<tsc 5.9.3> FORCE_COLOR=0 cargo test -p blueice-bluets
-p blueice-bluets-bluejs -j 4 --no-fail-fast -- --include-ignored --test-threads=4`.
The 51 test targets passed 1004 tests, including all 104 ignored oracle tests in 21 suite files,
with no failures, ignored tests or filtered tests. The refusal inventory was regenerated
for the moved source locations. A fresh production-source audit found no Rust source
in either crate at or above 1200 lines. K.1.3 proceeds after this separate commit and push.


### K.1.3 Imported value types — test-first evidence

The pinned TypeScript 5.9.3 compiler recorded 62 `imported-type-*` verdicts
(31 accepted, 31 rejected), including named/default/namespace imports, CommonJS
named imports and `import = require`/`export =`, interop, private exporter types,
generic and overloaded functions, class/enum bindings, dependency chains and
annotated/unannotated cycles. Sources use the existing authorized `.ts` resolution
path; emit comparisons enable TypeScript's relative-extension rewrite.

The initial 54-entry public CLI replay disagreed on 26 typing verdicts before
implementation, including accepting rejected argument/result/property types and
unannotated circular inference. The committed replay exercises both `check` and
`build` with no output on rejection. Four `import-decl-*` programs compare exact
inferred `.d.ts` text, and a linked program compares Node output and declarations.
Public compiler tests require inferred import/variable symbol metadata, exporter
private-type identity, dependency-surface reuse/invalidation and the explicit
unchecked transpile policy. These are failing tests for the implementation that follows;
K.1.3 remains unchecked until its full K.0 gate passes.


### K.1.3 Checked value surfaces and declarations

Dependency-ordered checking publishes each exported value together with its function
signatures, constructor bindings and the module-owned type definitions it references.
The existing namespace surface retains private type identities rather than resolving
them against an importer's equally named local type. An initial binding pass supplies
declared types to cycles; inferred variables from an unfinished module retain a pending
marker and circular inference requires an annotation with a TS7022-family message.
Successful incremental results retain these value surfaces alongside namespace exports.
The dependency/check/publication coordinator is now `checker/checking.rs`.

Named, default and namespace imports consume the checked surfaces, including function
overloads and generic signatures retained through variable alias chains. Namespace objects
expose readonly ESM export slots; a CommonJS `import = require` of a module retains writable
`let` slots and readonly `const` slots. `export =` objects also expose typed named members.
Qualified constructors use the class construction checker, and known imported values without
a construct signature are rejected. Default class/enum identifiers and named default functions
retain their public names. The closed project builder accepts static ESM cycles as well as
CommonJS cycles; source/edge/depth budgets and canonical owner resolution remain in force.
A hoisted-function ESM cycle is compared under Node, while unannotated circular value inference
is rejected before output.

Inferred variable symbols now retain their checked types for importers and debugger metadata.
Declaration emission consumes that metadata; `emitter/inferred_declarations.rs` retains required
imports, function `typeof` names, class/enum names, public external type queries, record formatting,
recursive callable/tuple type naming and fresh versus explicitly annotated literal declarations.
An external private type that cannot
be named is refused with a TS4023-family message, with no published artifacts. This adds no loader,
network or runtime grant; owner ambient declarations and `TranspileOnly` keep their existing policy.

Evidence: 81 pinned TypeScript 5.9.3 verdicts (39 accepted, 42 rejected), replayed
through `check` and `build`; 15 declaration cases (13 exact accepted comparisons and two TS4023
rejections); two linked Node programs including the ESM cycle; four public-boundary tests for
symbol types, private type identities, incremental reuse/invalidation and unchecked transpilation.
Three additional declaration cases reproduced the emission of internal `Shape@dep.ts` names
inside callable/tuple types and the acceptance of an unnameable private tuple type before the fix.
The declaration replay now includes 15 cases (13 accepted, two TS4023 rejections). The process
adapter's old cycle-rejection expectation was replaced with acceptance of annotated cycles and
a TS7022-family type error, without artifacts, for circular inferred initializers; the exact
requests were replayed through the built adapter before rerunning its tests.
Additional cases were recorded and shown to fail before their fixes.

The final K.1.3 gate passed on 2026-10-06: `cargo fmt --all -- --check`, all-target Clippy
for both crates with `-D warnings`, and `BLUEICE_BLUETSC_ORACLE=<tsc 5.9.3> FORCE_COLOR=0
cargo test -p blueice-bluets -p blueice-bluets-bluejs -j 4 --no-fail-fast --
--include-ignored --test-threads=4`. All 53 targets passed 1014 tests, including all
107 ignored oracle tests in 22 suite files, with no failures, ignored tests or filtered tests.
The refusal inventory was regenerated, and all production Rust sources in both crates
remain below 1200 lines. The workspace and coverage milestone gate remains M6 after K.1.5.
Standard-library typing, inferred function/getter returns and wider inference rules remain
K.1.4/K.1.5/K.4; value re-export syntax and imported-type query parsing remain inventory gaps.
Full parity stays off.


### K.1.4 Standard-library oracle before implementation

Recorded 78 `lib-*` verdicts from TypeScript 5.9.3 with the same ES2020/ES2022
`--target` and `--lib`, strict checking and no DOM library: 39 accepted and 39 rejected.
The offline replay checks both `check` and `build`, including the absence of output
for a rejected input. The cases cover Array/ReadonlyArray, primitive and boxed methods,
Object/Function, Promise, Map/Set/WeakMap, Math/JSON/Symbol, the Error family, iteration,
Date/RegExp and target-selected `at`/`hasOwn`. A Node program compares the selected runtime
calls, and three declaration programs compare methods, instances and collections.

The failing replay on 2026-10-06 confirms missing named library types, unchecked method
arguments and results, unselected target members, and missing manifest identity. Public
compiler tests require static library types to stay out of the source graph and artifacts,
local/owner declaration replacement, the existing page binding policy, and target selection.
These tests precede the implementation; K.1.4 remains unchecked until its full K.0 gate passes.


### K.1.4 Original standard declarations

The embedded `blue-ts-ecma-lib-v1` set selects original base and ES2022 additions,
parses them with BlueTS and caches their immutable ASTs by target. Its identity includes
the version, target, source names and exact content digest in the project fingerprint
and CLI manifest. No library source joins the owner's module graph or emitted output.
Local and ambient owner names take precedence, and page profiles retain their requirement
for owner/local runtime call bindings. Name recognition now comes from the selected
declarations, with the existing console compatibility and language-level undefined.

The checker uses those interfaces for primitive/array members, preserves library array
element compatibility and Iterable generic compatibility, and checks keyword-named
methods such as Map.get and Generator.return. Parsed constructor descriptors feed the
existing new checker without admitting declare-class or construct-signature syntax.
The factory const spelling of unique symbol is compared separately from aliases,
function results, annotations, let and a local Symbol shadow; callable factories retain
their static registry members. Existing decorator programs' Object operations have
parsed signatures too. Omitted shapes and precision are listed in STANDARD_LIBRARY.md.

Focused validation passed 123 pinned TypeScript 5.9.3 target/lib verdicts (61 accepted,
62 rejected), replayed through check and build; five exact declaration comparisons;
one Node program; CLI identity/output isolation and five public compiler boundaries.
The extra Symbol factory case reproduced acceptance of Symbol(true) before its fix;
the pinned declaration probe established the const-factory freshness rule before the
emitter change. The runtime fixture uses a fixed log signature within the existing
declaration subset rather than a refused rest-method signature.

The final K.1.4 gate passed on 2026-10-06: format/check, both crates' all-target Clippy
with `-D warnings`, and the complete two-crate test command from K.0 with the pinned
oracle and `--include-ignored`. All 55 targets passed 1026 tests, including all
110 ignored oracle tests in 23 suite files, with no failures, skipped or filtered tests.
The refusal inventory was regenerated. All production sources remain below 1200 lines.
The workspace/coverage M6 gate follows K.1.5. The minimum library's recorded omissions
remain visible in STANDARD_LIBRARY.md and the compatibility inventory; full parity stays off.

The full replay exposed two regressions before completion: implicit readonly
library constants polluted the pre-existing opaque-receiver safety check, and
Array.map callbacks were required to consume all three supplied arguments.
The readonly fix distinguishes lexical library bindings from source/owner
shadows and preserves explicit library flows and alias initializers. Six
additional pinned target/lib cases establish zero-to-three callback parameters
as accepted, and a fourth required parameter or wrong parameter type as rejected;
three accepted cases failed against BlueTSC before the compatibility fix. A fifth
public boundary covers implicit constants, explicit flows, aliases, local parameter
and namespace shadows, and owner declarations. The complete final replay passed after both regression fixes.

Library method signatures also exposed the old receiver scan treating an entire
binary-expression prefix as the receiver of its final method. Twelve pinned
cases cover left/right operands, nested arguments, generic call results and
unary operators: six accepted and six rejected. The pre-fix replay showed both
false rejections and missed invalid arguments/results. A separate postfix-range
module now shares receiver boundaries between validation and inference, retains
keyword-named members and non-null assertions, and checks each nested call.
The full recorded class matrix and all 98 library cases pass the offline check/build
replay; the final K.0 gate passed on the same frozen source snapshot.

The complete regression corpus required selected Array.sort/filter and JSON
replacer/spacing declarations, including stringify's pinned `string` result,
and enum-member property boxing without changing nominal assignment. Eighteen
additional pinned cases cover these forms. Seven iterator-loop cases cover the
yield projection, unguarded access, reassignment, shadowing and following statements.
The adapter uses parsed boolean discriminator fields, lexical binding identities,
and write invalidation; it does not claim general control-flow narrowing.
The original generator-runtime fixture now checks without modification. The library
matrix totals 123 cases (61 accepted and 62 rejected); the final K.0 gate passed
on a frozen Linux snapshot with Rust 1.95, Node 26.7 and TypeScript 5.9.3.


### K.1.5 Return-inference oracle before implementation

The pinned TypeScript 5.9.3 compiler records 83 `infer-return-*` verdicts
(51 accepted, 32 rejected), using strict ES2022 with the same ES2022 library.
The fixtures cover primitive and union returns, fall-through, void/undefined/null,
freshness and widening, local/branch scopes, forward and generic calls, recursion,
async wrapping and awaiting, generator yield/return/next types, accessors and methods,
arrow/function-expression bodies, namespaces and imported checked signatures.
Fourteen declaration programs compare exact `.d.ts` output; one linked Node
program compares runtime output. Three public compiler tests cover emitted signatures,
importers and recursive inference.

Before implementation the oracle replay and matrix coverage tests pass, while the
check/build replay, declaration comparison, Node program and all three public tests
fail. The public declaration still prints `unknown`; an importer accepts an invalid
string assignment, and recursive return inference lacks its annotation diagnostic.
This failing replay is committed before the implementation. K.1.5 remains unchecked
until the complete K.0 gate passes; workspace and coverage remain the M6 milestone.


### K.1.5 Inferred return signatures

A separate return-inference module collects expressions in structured lexical
bodies, preserving branch scopes and excluding nested functions' returns.
It joins multiple returns, adds implicit undefined on a completing value-return
path, distinguishes bare/no-value returns, and widens a single fresh literal.
Annotated literal values retain their type. Async results flatten Promise values;
generators retain yield, return and contextual next types. Return inference shares
the configured expansion budget and records a diagnostic instead of recursing
without a bound.

The additional pinned recursion probe distinguishes direct tail self-calls
(`never`, or a widened base return) from mutual or embedded cycles that require
an annotation. The original public recursive fixture was corrected to the
record-expression cycle after that probe; its previous bare self-call is accepted
by pinned TypeScript and remains in the matrix as `recursive-tail-valid`.

Inferred signatures are retained for imports, including generic parameters.
Class access uses the existing private/protected visibility view; default and
destructured parameters contribute to the same lexical inference. Return and
default-parameter types are recorded separately from the original AST. Only
declaration output applies those annotations, preserving JavaScript emission
and runtime boundary policy.

The expanded matrix records 121 verdicts (77 accepted, 44 rejected), twenty-three
exact declaration comparisons and one Node program. Two additional declaration
programs use a pinned TypeScript type-equality consumer to compare unions whose
member printing order differs with literal interning; their types agree, while
that text-order difference remains explicit. Four public tests cover
callers/declarations, imports, recursion, the expansion limit and preservation
of the source AST. All 121 check/build verdicts, declaration comparisons and
public/runtime tests pass in the focused replay.

Additional pinned completion probes cover nonreturning calls, infinite loops,
getter bodies returning a value or completing without one, declared unknown
values and setter parameters inferred from a getter. Completion analysis is
kept in its own module. Record parameter formatting and nested inferred literal
spelling were compared against pinned declaration output before their fixes.
The first full replay exposed three stale assertions/discovery rules for the
now-supported getters and methods. Their obsolete refusal expectations and
class-matrix deferred entry were removed; the focused regression replay passes.
A final public-interface probe reproduced a method-default-parameter panic
and an inferred declaration that doubled string escapes. Four further pinned
fixtures cover those regressions: method defaults now contribute parameter
types to checked class signatures and declaration copies, while inferred string
literals use the existing JavaScript string decoder before canonical quoting.
The final K.1.5 gate passed on 2026-10-06 on a frozen Linux aarch64 snapshot
with Rust 1.95, Node 26.7 and TypeScript 5.9.3. Format/check, both crates'
all-target Clippy with `-D warnings`, and all 57 test targets passed: 1036 tests,
including all 114 ignored oracle tests across 24 suite files, with no failures,
skipped or filtered tests. The refusal inventory was regenerated. The largest
production source is 1171 lines; no production source reaches the 1200-line
review threshold or needs another queued split. The completed K.1.R.1 split is
retained in the refactoring queue. K.1.5 is checked; the M6 workspace/coverage
milestone is next. Broader literal freshness, contextual inference, control flow
and declaration precision remain in K.4/K.8; full TypeScript parity stays off.


### M6 workspace replay: imported-type metadata expectation

The first M6 CI run [37373793823](https://github.com/ephoton0210/blueice/actions/runs/37373793823)
on `930bbb9a1` found an obsolete engine IPC assertion: the imported, annotated
`answer` binding was still expected to display `unknown`. K.1.3 retains its
checked `number` type, so the generation-bound, source-free metadata query now
correctly returns `number`. The assertion and its comment are updated without
changing protocol or runtime authority. All 26 compiler IPC boundary tests
pass in the Linux disk-budget runner. Format/check and engine all-target
Clippy with `-D warnings` also pass. The M6 gate remains unchecked pending
the complete CI replay on the corrected revision.


### M6 workspace replay: native package-resolution paths

CI run [37375485614](https://github.com/ephoton0210/blueice/actions/runs/37375485614)
on `8167eb82a` passes the Linux workspace tests and the workspace 90% line
coverage step. Its Windows 11 arm64 job exposes thirteen package-resolution
tests comparing native backslash path text with hard-coded forward slashes.
The resolver returns the correct files; the shared test helper now retains
`PathBuf`, and all 26 path expectations compare native path components.
Production resolution, canonical-root confinement and fingerprints are unchanged.
All 33 package-resolution tests, format/check and both crates' all-target
Clippy with `-D warnings` pass in the Linux runner. The complete M6 replay
on the corrected revision remains required before checking the milestone.


### M6 workspace replay: original LF and CRLF spans

The Windows Server 2022 job of CI run [37379358433](https://github.com/ephoton0210/blueice/actions/runs/37379358433)
on `290ce28ce` passes native package-path tests and reaches the frontend suite.
Its fall-through-method test expects LF text even though a Windows checkout
embeds CRLF fixtures. The diagnostic's original byte range is correct.
The test now compiles both LF and CRLF variants and compares the exact source
slice with the expected text in that variant's newline format. No diagnostic
range, parser or checker behavior is changed.

All 123 frontend tests pass with the ordinary LF fixture snapshot and again
with every TypeScript-oracle fixture converted to CRLF in the isolated Linux
copy. That copy is restored before format/check and both crates' all-target
Clippy with `-D warnings`, which also pass. The M6 gate remains unchecked
until the complete CI run on the corrected revision passes.
