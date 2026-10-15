# Original diagnostic source positions

[← Browser integration TODO](BROWSER_INTEGRATION_TODO.md)

M1q is in progress. The TODO records verified failing page/VM fixtures,
successful foundation tests and all remaining acceptance gates. Section H
remains open until original-source reporting works across the complete path.

## Immutable code ownership

An owner constructs `DiagnosticSourceMap` for one exact
`BlueJsSourceIdentity`. Binding verifies the live generation, source identity,
code-unit ordinals and instruction boundaries before publishing anything.
Half-open ranges are sorted and non-overlapping; gaps remain unavailable.
All code units share one immutable `Rc` map. Compiled closures keep their own
association even after another program runs. No JavaScript value, live object
root or ambient mutable filename is retained by the map.

Locations contain a bounded copied filename and one-based line/UTF-16 column.
The scanner handles CRLF as one newline, as well as CR, LF, LS and PS; an
interior UTF-8 byte has no coordinate. HTML leaves error-information extraction
[implementation-defined](https://html.spec.whatwg.org/multipage/webappapis.html#report-an-exception).
This boundary uses certified compiler/parser provenance and explicit absence.

The current map limits are 65,536 ranges, an 8 KiB filename and 8 MiB of owned
map payload. Vector/string capacities participate in measurement. The page
installer charges root code and immutable map payload together against its
existing installed-program byte limit. A failed builder, invalid table or
excess combined charge removes the candidate program before execution.

## Parser and compiler statement provenance

The original prototype mapped classic JavaScript root statements. A real
fixture proved that this incorrectly located a nested block at its outer `if`
and left a function's throw unlocated. The expanded implementation captures
parser statement subtrees, including normalization, and pairs their structure
and function source ranges with the executable AST preorder. Temporary pointer
identities stay inside compilation; emitted owners contain AST ordinals only.

The diagnostic compilation mode records the active AST owner of each emitted
instruction. The page owner combines those exact owners with parser spans,
coalesces adjacent equal locations, then discards temporary instruction-owner
metadata when binding the immutable source map. Ordinary host-neutral parsing
and compilation omit this extra collection. Existing debugger AST identities
and the structured-program ABI retain their order and meaning.

The interpreter restores a caller's source context after nested execution.
An explicit `reportError` uses its active call site; an uncaught language throw
uses its captured instruction origin. The existing terminal debugger fallback
is separate: a mapped caller cannot manufacture a location for an unmapped
`eval` or `Function` body. Error events preserve their existing cancellation,
handler arguments, recursion and original-value behavior.

## Expression and binding-pattern provenance

`parse_with_diagnostic_node_spans` retains the public statement/expression AST
prefix and appends binding-pattern slots followed by assignment-pattern slots
in source-only namespaces. The
legacy statement-span API and debugger inventory keep their original shape.
Compiler instruction owners select expression and binding-pattern locations;
temporary overlays preserve proven original ordinals through named and unnamed
arrow lowering. Aliases contain temporary AST addresses only during compilation.

Template placeholder ranges come from the lexer, including nested and tagged
templates and braces inside strings/comments. Subparsers rebase their captured
ranges into the enclosing source. No token-text search supplies a location.
Cumulative cloned AST and span-storage work is capped at 8 MiB before snapshot
cloning; shared `SourceText` buffers are not cloned or charged again. A public
control accepts wide expression lists while refusing excessive capture work
without treating valid grammar as a syntax error.

Nine initial public failures report an outer statement or no source. The
expanded twenty-three-scenario matrix passes, including parameter defaults,
destructuring, arrow lowering, operator/constructor intermediates, templates,
CRLF and UTF-16 columns. Thirty-five diagnostic groups pass, with source-only
pattern inventory and unchanged ordinary bytecode controls. The old real
Core/host still report the statement's inline column 25 where the RHS call
begins at column 36. Matching new Core/host now pass all three official MCP
console cases with the exact RHS line 5, column 36, through generator/finally/
async-stack/`await using` forwarding. Five additional real-page contexts verify
operand, default/destructuring parameter, named-arrow and template coordinates.

The final disjoint selection passes 1,611 BlueJS cases across thirty-eight
targets, including 1,205 library cases and thirty-five diagnostic groups.
Bridge, Launcher and real page targets add 491 passes and one existing ignored;
three MCP passes bring the total to 2,105 over forty-two targets. Formatting,
matched Core/host/Gatekeeper builds and focused Clippy pass. Twenty final Rust
input hashes, old/new runtime hashes, test hashes and receipts are in
`target/expression-origins-source-20261015.json` and
`target/expression-origins-validation-20261015.json`.
The subsequent requested cleanup removes old compiled executables and caches;
source hashes, executable hashes, receipts and logs remain available. Changed
integration-target Clippy and a complete diagnostic-target replay also pass
after an equivalent test-configuration initialization fix.
Remaining expression dispatch, decorator application and other generated-code
provenance still require their implementation and acceptance.

### Class and decorator lowering

Constructor and accessor naming clones retain the original parameter/body
owners through temporary inventory overlays. Static-block copies retain their
statement subtrees; static and instance field lowering aliases only the copied
initializer expression, including nested functions, arrows and classes. The
generated receiver, stored key, private-brand operations, default constructors
and auto-accessor functions acquire no invented source owner. The shared
overlay helper restores the parent inventory on success and compilation error,
before any temporary clone address can be reused. No AST address survives in
compiled bytecode or becomes a JavaScript root.

The dedicated decorator grammar captures identifiers and intermediate member
references as well as the final expression. Receiver-preserving decorator
evaluation installs the corresponding expression owner, including the inner
member of a parenthesized decorator, without changing receiver semantics.
Decorator application failures in generated operations remain a separate
precision requirement.

The first eighteen public `ErrorEvent` scenarios reproduce fifteen missing or
coarse locations before production changes; all eighteen pass after the fix.
The expanded matrix covers thirty-two source scenarios and three unknown-source
controls for generated accessor/field operations and dynamic `eval`. The
shared public-boundary helper also compares ordinary syntax, code-unit counts
and instruction bytes against diagnostic compilation. Real-page, queued MCP
and cross-script major-GC acceptance are recorded in the milestone evidence.

Four later-script cases verify actual major collection before invoking retained
constructors, getters and field/static-block closures. The final selection
passes 1,627 BlueJS cases across thirty-three targets, including 1,205 library
cases and all thirty-eight diagnostic groups. Bridge, Launcher and real pages
add 491 passes and one existing ignored; three official MCP console passes
bring the disjoint total to 2,121 across thirty-seven targets. Six additional
real-page source contexts pass, and MCP preserves queued getter/field throws
at line/column 5:78 and 6:74. Formatting and focused library/integration-target
Clippy pass. After Cargo's test build relinks the child host, both cross-process
targets are replayed without rebuilding; all twenty cases pass with identical
final runtime hashes before and after. Eight Rust input hashes, initial/final
runtime hashes, test hashes and receipts are in
`target/class-origins-source-20261015.json` and
`target/class-origins-validation-20261015.json`. The complete diagnostic target
is replayed after a test-only change moves the major-GC assertion before
closure invocation; the other production and test inputs stay unchanged.
Broader source-producer,
precision, integrated and final conformance requirements remain open.

### References and assignment patterns

Assignment patterns append a separate source-only inventory after the unchanged
public AST prefix and existing binding-pattern slots. Their parser capture
records nested arrays/objects, targets, shorthand identifiers and expression
`for-in/of` heads from lexical ranges. Template subparsers rebase the same
forest; compiler clone overlays include these nodes. Legacy statement tables
and public debugger ordinals keep their shape and authority.

Reference lookup and write instructions select their actual target owner,
including prefix updates, parenthesized calls/tags, optional-chain recursion,
logical stores and prepared destructuring targets. Evaluation of a prepared
member, acquisition of an iterator/property value and the eventual target
store retain their respective owners without changing their instruction order.
The shared capture ledger charges assignment snapshots before cloning and
retains the existing 8 MiB limit. A 3,000-target pattern is admitted; a valid
40,000-target pattern is refused without becoming a syntax error.

Twenty-seven of thirty initial public source scenarios fail before the fix.
The expanded thirty-seven-scenario selection and unknown getter/Proxy controls
pass within all forty-two diagnostic groups. Public compatibility checks verify
the source-only suffix, legacy cardinality, ordinary syntax, code-unit counts
and instruction bytes. The old matched Core/child reports a queued optional
callee at 2:51 instead of 3:27 through official MCP; the final host acceptance
also verifies a queued nested-assignment location at 6:27 and six additional
real-page contexts. The final selection passes 1,571 BlueJS cases over forty-one
targets, including 1,205 library cases and forty-two diagnostic groups. Host
acceptance adds 491 passes and one existing ignored; four official MCP console
passes bring the disjoint total to 2,066 over forty-five targets. Formatting and
focused library/integration Clippy pass. All artifacts are compiled before
fixing runtime/test hashes for acceptance; those hashes remain unchanged.

An initial parallel browser run exposes an existing race between an older
frame event and a newer `GetRepresentation` reply. The helper retains all three
distinct unsolicited frames/pixels and matches each returned counter snapshot
to its observed frame generation, including the exact final count. Only that
test input changes; the complete seventeen-case rerun and its Clippy pass.
The original failure and isolated pass remain in the evidence but are excluded
from the final disjoint total. Eleven final Rust input hashes, old/new runtime
hashes, test hashes and receipts are retained in
`target/reference-origins-source-20261015.json` and
`target/reference-origins-validation-20261015.json`. Remaining precision,
source-producer, integrated and final conformance requirements stay open.

## Emitted BlueTS, modules and debugger execution

The emitted source producer binds the compiler artifact to the authorized
original module ID, original content hash and exact `sourcesContent`. The
generated JavaScript keeps its own compiled source identity; diagnostic
filenames come from the verified original source. A source-map URL or an
unbound debugger map cannot supply this association. Copied original identity
strings participate in the existing 16 MiB retained-map budget.

`parse_module_with_diagnostic_node_spans` captures executable export items,
nested statements, expressions and patterns using the same bounded inventory
as scripts. Declarative import/export headers add no debugger nodes. Diagnostic
compilation preserves the ordinary AST inventory, instruction bytes and child
code units. Static module request rewriting does not change these source slots.

The owner converts parser byte boundaries to the emitter's zero-based CR/LF
and UTF-16 coordinates, then performs one exact batch lookup. The decoder
validates the complete mapping stream, bounds input bytes, segments and query
count, and preserves query order and duplicates. Only an exact mapping segment
supplies provenance; intervening positions and explicit unmapped segments stay
unknown. The decoder and owner scanner both match the emitter's lone-CR and
CRLF rules. Original coordinates convert through exact UTF-8 boundaries to
one-based ECMAScript CR/LF/LS/PS and UTF-16 positions; an interior surrogate
position cannot create a location.

Normal and debugger-controlled emitted classic/module programs use this
builder. JavaScript module installation and deferred classic execution use the
original JavaScript builder. All paths install immutable diagnostic maps in
the code admission transaction. A substituted original, malformed complete
stream or combined code/map budget failure discards the candidate; graph
attachment also discards previously admitted modules before publishing any
debugger identities. Normal emitted execution releases the temporary compiler
map after installation; debugger execution retains its bounded artifact for
the existing generated-source controls.

The expanded matrix also fixes BlueTS LS/PS trivia and line-comment termination:
the lexer advances comments on complete UTF-8 characters and preserves token
byte spans. A standalone regression first fails because a Unicode line break
leaves the following function inside the comment. The final BlueTS selection
passes 276 cases, including the six public source-map lookup groups; ten
existing oracle cases require pinned TypeScript 5.9.3 and remain ignored.

The current-source selection passes 1,450 BlueJS cases across twenty-one targets,
including all forty-three diagnostic groups and eight module-shape fixtures.
Bridge, launcher and real pages add 496 passes; five official MCP console cases
bring the disjoint total to 2,227 across thirty target selections. One existing
launcher ignore and the ten oracle ignores are retained. Six new real-page
contexts verify original emitted classic/module and JavaScript-module positions.
MCP verifies emitted classic/module/async throws at original `.ts` line/column
2:2 alongside an explicitly unavailable dynamic `Function` origin.

Formatting, focused seven-library/integration Clippy and launcher test Clippy
pass. The real TLA fixture declares an explicit export under the existing
checked TypeScript module policy; its bounded observer keeps exact coordinate
assertions. All host tests replay after the fixture correction, with runtime
and test hashes fixed before and after execution. Unchanged BlueJS/BlueTS
inputs retain their passing receipts. Sixteen Rust inputs, two Cargo inputs,
executable hashes and per-target receipts are recorded in
`target/emitted-origins-source-20261015.json` and
`target/emitted-origins-validation-20261015.json`.

### Initial direct BlueTS source bindings

Direct page-realm script and module installers now use source-only records
derived from checked BlueTS declarations and their final lowered AST. A shared
BlueJS visitor names the exact diagnostic ordinals, including the existing
pattern suffixes, without creating live debugger IDs. Temporary AST addresses
are discarded when the bridge converts certified byte offsets to coordinates.
The common bounded range builder consumes compiler instruction owners for both
the launcher and the direct bridge.

Each direct artifact retains its original source identity, coordinate vector
and a private bounded AST digest. The digest streams the AST's debug structure
and actual numeric bits without retaining another AST or original source text.
Source substitution or AST mutation invalidates the evidence before execution.
The page installer charges code and source ranges together; metadata refusal
discards the candidate and graph attachment discards earlier candidates.
Ordinary bridge bytecode, function `SourceText` behavior and debugger ordinals
retain their original representation. Standalone registry/debugger attachment
retains its existing caller-owned AST editing behavior.

The initial bindings cover exact statement spans in ordinary functions and
control-flow bodies, expression roots, certified identifier/member/call
prefixes, defaults and standard defined-field class members. Constructor
parameter-property prologues stay unmapped while original body statements
keep their own positions. A separate fix adds the missing top-level lowering
record for a default-export identifier snapshot; its ordinary code is unchanged.

The initial producer left deeper expression/pattern nodes, namespace/enum/JSX
lowering and assignment-field class paths without their own evidence. The
expression follow-up below addresses part of that gap. Full producer/lifecycle/
resource and nonstandard emission-profile acceptance remain open; M1q stays
at 3/10 final gates.

The initial direct selection passes 1,205 BlueJS library cases, forty-four
diagnostic groups and all 173 bridge cases. Launcher, real pages and official
MCP console add 336 passes, for 1,758 across six disjoint target selections,
zero final failures and one existing ignored. Five new real-page contexts and
three deferred debugger contexts preserve direct `.ts` locations. MCP verifies
direct `reportError`/console call sites at 1:1 and a Blob-text Promise callback
throw at 1:44. Source/AST changes and a low code/map byte budget are refused
without leaving a program or byte charge. Twelve Rust inputs, three Cargo
inputs and runtime/test hashes are recorded in
`target/direct-origins-source-20261015.json` and
`target/direct-origins-validation-20261015.json`.

Formatting, seven-library/integration Clippy and bridge/launcher test Clippy
pass. The final test-only correction replaces the old typed-console
unavailable-source assertion with exact coordinates; all five MCP console
cases and MCP Clippy replay successfully. Other runtime/test hashes remain
fixed. Successful BlueJS harnesses from the coupled invocation retain their
own receipts; its failed direct-call fixture is excluded from the final count.

### Direct expression token evidence

An optional collector now accompanies the existing direct-expression parser.
It records each constructed expression's original token bounds and ordered
executable children. Grammar tiers returning an unchanged expression share
their existing record; intermediate binary/member nodes retain separate
records before the next operator consumes them. Constructor callees are
recorded before arguments, so `new C(C)` keeps the two identifiers distinct.
Object shorthand values and computed keys have their own records, while
noncomputed property names remain data rather than identifier evaluations.

The source producer replays the checked original token slice through that
same parser, including type-assertion/type-argument erasure. It binds the
result only when the entire expression equals the final lowered AST. Token
order and child paths determine the join; repeated structural matches never
select a different occurrence. This replay does not parse emitted JavaScript
or replace executable code. The existing artifact digest and source identity
still bind installation to its immutable final AST.

Capture allocations are conservatively charged before growth against an
8 MiB limit. Inconsistent evidence or capture refusal stops artifact creation.
This first expression increment kept template, class-expression and JSX atoms
opaque below their roots. Its public template regression confirmed that an
unmapped descendant did not borrow its parent's location; the next increment
below now supplies that descendant's own evidence. Namespace/enum rewrites that
do not exactly match a replay retain only their earlier proven bindings.

Twenty-two public failures verify operands, arguments, member chains, computed
keys, arrays, object values, conditional/logical branches, assignment/update
references, default parameters and erased assertions. Eighteen parser shapes
verify duplicate child ordering and unchanged AST/instruction/constant output;
a wide expression refuses oversized metadata. All 177 bridge cases pass,
including dependency expressions before/after await. Remaining patterns,
opaque descendants, namespace/enum/JSX/assignment-field lowering and the full
integrated acceptance gates remain open.

Four additional real-page contexts and three additional deferred debugger
contexts preserve operand, argument and computed-key coordinates. Official
MCP also verifies three queued direct errors at original 2:2, 3:2 and 4:2,
alongside the existing JavaScript reference/assignment failures. The final
disjoint selection passes 1,762 cases across six targets with zero failures
and one existing ignored. Formatting, seven-library, changed integration and
bridge/launcher test Clippy pass. Eleven Rust inputs, three Cargo inputs and
runtime/test hashes remain fixed; receipts are retained in
`target/direct-expression-source-20261016.json` and
`target/direct-expression-validation-20261016.json`. M1q stays at 3/10 final
acceptance gates.

### Embedded template and class-expression evidence

The existing template lowering parser now passes its optional capture through
the parser for each substitution. The lexer still rebases tokens to original
byte offsets, and the grammar, escape decoding and ordinary output are
unchanged. Siblings share both pending roots and the allocation charge, so
many individually small substitutions cannot bypass the 8 MiB capture limit.
The origin tree names each substitution in execution order, including templates
inside arrays/calls and repeated expressions.

After exact expression replay succeeds, a class-expression root can also name
its checked original class declaration by the recorded byte offset. Defined-
field classes use the existing member/body evidence traversal for methods,
accessors, constructors, defaults, static blocks and field initializer
expressions. Original statements keep their positions around constructor
parameter-property prologues. Nested class expressions recurse through the
same proof. A successful parent traversal must not clear a refusal raised by
its child; a public oversized-class fixture verifies this atomic failure.

Fifteen public template/class failure scenarios pass, plus ten source-coordinate
contexts covering UTF-16 and CR/CRLF/LS/PS. Module dependencies retain template
and class-expression origins before and after await. Five parser shapes retain
ordinary ASTs, instruction bytes and constants. Shared-budget and nested-refusal
controls pass. Dynamic function bodies reached through a mapped template or
class keep unavailable origins. This increment kept assignment-field class
descendants unavailable; the class-path follow-up below supplies their own
evidence. Remaining class/computed-key,
pattern/namespace/enum/JSX/assignment-field evidence and integrated lifecycle/
resource/full-workspace/Test262 acceptance stay open.

The final selection passes all thirty-five bridge library/integration targets
(253 cases, fifteen existing external-oracle cases ignored), forty-four BlueJS
diagnostic groups, 312 launcher cases, nineteen real-page cases and five MCP
console cases. This is 633 passes across thirty-nine disjoint targets, zero
final failures and sixteen existing ignored. Five additional real-page contexts
and two deferred debugger contexts pass. Queued template/class MCP errors
retain original 5:2 and 6:2. Formatting and related library/integration/test
Clippy pass with warnings denied.

The final page fixture needed an explicit export for its top-level await to
pass the existing BlueTS external-module check. After that test-only correction,
the complete page/MCP replay and browser-target Clippy pass. Other source,
runtime and test identities are unchanged. Nine Rust inputs, three Cargo inputs,
three runtime and thirty-eight test executables are recorded in
`target/direct-nested-source-20261016.json` and
`target/direct-nested-validation-20261016.json`; the initial failed receipt and
source identities are retained in the matching `direct-nested-initial-*`
manifests. Final host replay keeps every recorded identity fixed. M1q remains
at 3/10 final acceptance gates.

### Direct classic JSX source evidence

Classic JSX lowering now passes its original embedded token groups through the
same optional source collector as ordinary expressions and templates. Written
component names are lexed at their actual source byte offset; generated factory
and fragment names have no copied element position. A lowered record can carry
ordered child records while its own position is explicitly unavailable.
Factory invocations, props objects and generated literals retain those gaps;
precedence tiers preserve that status when returning the same lowered node.
Only exact full-expression replay equality permits the final AST join.

This preserves attribute/child expression positions through nested elements,
element-valued attributes, fragments, spreads and templates. Repeated identical
expressions keep independent source offsets. A real factory function or spread
getter can still report its own body position beneath the generated scaffolding;
dynamic function bodies cannot borrow a mapped embedded call's position. All
records share the original capture allocation charge before growth, including
synthetic wrappers and many small embedded children.

Source-coordinate regressions also exposed JSX trivia that stopped only at LF
and skipped only ASCII whitespace. The scanner now handles CR/LF/LS/PS comment
termination and LS/PS tag whitespace, preserving original byte offsets. Block
comments scan bytes safely through multibyte text. Twelve scanner cases verify
name ranges across all four newline forms, line comments and Unicode block
comments.

Fourteen public failure forms, four unavailable-source controls and two known
callback bodies pass. Six parser shapes preserve ordinary ASTs, instruction
bytes and constants; ten coordinate contexts preserve UTF-16 and original
line terminators. Dependency origins work before/after await, and queued JSX
callbacks keep their source through later unmapped code and disappear on
navigation. The 50,000-child capture test exercises the parser boundary
independently of full-compiler admission. All 192 bridge and 254 BlueTS library
cases pass.

Direct JSX coverage uses the explicit classic-mode bridge API. The launcher's
existing page compiler profile uses `jsx: None`; its Core/MCP regression
selection verifies the shared reporting path with existing JavaScript/BlueTS
inputs. Remaining helper, class/computed-key, pattern/namespace/enum and
assignment-field precision, integrated lifecycle/resource acceptance and full
workspace/all-target/Test262 gates stay open.

Final acceptance passes 937 cases across forty-seven disjoint targets with zero
failures and twenty-two existing ignored: 263 bridge, 294 BlueTS, forty-four
BlueJS diagnostic groups, 312 launcher, nineteen real-page and five MCP console
cases. Formatting, seven-library and changed integration Clippy, and complete
BlueTS/bridge/launcher test Clippy pass with warnings denied. Seven Rust inputs,
four Cargo inputs, four runtime executables (including the tested compiler CLI)
and forty-six test executables remain fixed. Receipts are retained in
`target/direct-jsx-source-20261016.json` and
`target/direct-jsx-validation-20261016.json`. M1q remains at 3/10 final gates.

### Class lowering paths and decorator application

The actual class lowerer can now emit a temporary path recipe alongside its
unchanged AST. It records original member indices, pending computed-key
assignments consumed by later keys, moved instance/static field initializers,
anonymous-class NamedEvaluation wrappers and the constructor's actual prefix
length after computed-binding hoisting. A bounded replay must equal the final
class before these paths bind any nodes. Class-expression names supplied by
the enclosing proven expression are normalized at that boundary. The recipe
is discarded after conversion to the ordinary coordinate inventory.

This covers both field modes, generated constructors, original constructor
statements around prologues, trailing pending-key blocks and erased fields
that still evaluate their key. Source expressions inside synthetic assignments
keep their own positions; assignment helpers and inferred-name scaffolding
do not acquire a nearby declaration's location. Path vectors have a separate
conservative 8 MiB charge before growth, including members without executable
expression slots.

Fifty-two direct class scenarios pass in both declaration/expression forms and
both field modes, following thirty-four reproduced missing origins. Six shape
controls preserve ordinary ASTs, instruction bytes and constants. Twenty source
coordinate contexts, eight awaited/non-awaited dependency cases, queued
retention/navigation and a hoisted constructor-prefix case pass. A synthetic
read-only field write remains explicitly unavailable.

Decorator application exposed an additional native validation gap. The compiler
now attributes `PushDecorator` to the actual expression. Its existing private
heap-owned array record retains copied filename/line/column scalars with the
function and receiver. The scalars use normal heap accounting and tracing,
introduce no JavaScript object roots or debugger handles, and are not passed
to user callbacks. Native callable/result validation selects the failing entry's
source and establishes a fresh propagation epoch. Callback/getter throws keep
their own known or unavailable origin; caught noise cannot replace a later
validation error's location. Ten JavaScript cases pass under normal and
aggressive GC, including multiple decorators and invalid accessor results.

The native/Proxy follow-up below extends these validation cases to dispatch and
accessor-result property reads. Generated helper and namespace/enum/pattern
precision, remaining dispatch/application cases, integrated lifecycle/resource
acceptance and full workspace/all-target/Test262 gates remain open.

The final selection passes 2,000 cases across forty-three disjoint targets,
zero failures and sixteen existing ignored: 1,205 BlueJS library, 142 decorator,
forty-five diagnostic groups, 272 bridge, 312 launcher, nineteen real-page and
five MCP console cases. All bridge targets and the full related decorator
targets pass, including allocation/instruction refusal regressions. Three
additional real-page contexts and two deferred debugger contexts pass. MCP
preserves a queued direct key at original 7:2 and typed decorator validation
at 2:2. Formatting and related library/integration/test Clippy pass with warnings
denied. Thirteen Rust inputs, three Cargo inputs, three runtime and thirty-eight
host test executable hashes stay fixed; receipts are retained in
`target/direct-class-lowering-source-20261016.json` and
`target/direct-class-lowering-validation-20261016.json`. M1q remains at 3/10
final acceptance gates.

### Native and Proxy decorator dispatch

Twelve native/Proxy failure shapes initially reported the class location or
unavailable coordinates instead of the evaluated decorator expression, under
both normal and aggressive GC. Native calls had no nested interpreter to
publish a throwing source. A successful Proxy getter that caught an unrelated
throw also left its propagation epoch behind, obscuring the subsequent native
validation failure.

An active diagnostic frame can now represent either certified bytecode or a
copied native location, including an explicitly unavailable one. Decorator
dispatch and accessor-result reads temporarily install the saved expression's
location. A catchable failure without a fresh callee throw publishes that
location; nested interpreted code masks the scope and keeps its own known or
unavailable source. Every result path restores the displaced frame. The scope
adds no persistent records, JavaScript object roots or debugger handles.

Successful calls move the caller's pending throw metadata aside and restore it
without copying its filename. This includes a successful tail-call transition.
Caught callback noise therefore cannot disguise a later native failure, while
an escaping callback error retains its own provenance. Actual debugger throw
sites continue to come from interpreter instructions independently of these
source-only locations.

Twenty-three decorator scenarios pass with normal/aggressive GC: constructor-only
and bound natives, revoked/forwarding Proxies, invalid apply traps, caught
getter noise, stacked entries, accessor-result invariants and original callback
throws. Three ordinary callback cases also verify successful-call restoration.
Two dynamic native/Proxy decorator controls keep unavailable coordinates.
All forty-seven diagnostic groups pass. Direct BlueTS adds six script cases
across both field modes and eight native/Proxy dependency cases before/after
await. Its fixtures obtain the existing Proxy through `globalThis` and use
supported class getters rather than requiring new direct expression forms.

The follow-up passes 2,302 cases over sixty-nine disjoint targets, zero failures
and sixteen existing ignored: 1,694 BlueJS, 272 bridge, 312 launcher, nineteen
real-page and five MCP console cases. Two additional real-page and deferred
debugger contexts preserve 2:2; MCP verifies two more typed scripts at 2:2.
Formatting and related library/integration/test Clippy pass with warnings
denied. Nine Rust inputs, three Cargo inputs, three runtime and sixty-nine test
executable hashes are recorded. The source receipt separately records a
host-only fixture name correction before host builds; completed BlueJS inputs
and executable hashes remain unchanged. Evidence is retained in
`target/native-decorator-source-20261016.json` and
`target/native-decorator-validation-20261016.json`. Remaining dispatch and
generated-code precision, integrated lifecycle/resource and full workspace/
all-target/Test262 gates remain open; M1q stays at 3/10 final acceptance gates.

### Namespace and enum lowering

The production namespace lowerer optionally records its actual IIFE wrapper
indices, original body declaration indices, exported initializer values and
nested namespace paths. Its reference-rewrite context accompanies those
temporary paths. A shared 8 MiB budget charges vector slots and reference-map
capacities, including tree-node slack and displaced traversal copies, across
the entire nested recipe; the ordinary lowering path retains
none of it. Complete root namespace groups must replay with identical
structure, including merge-dependent declarations and dotted wrappers, before
the recipe joins any final AST nodes. Only original runtime statements and
expressions receive positions. Generated object setup, export assignments and
wrapper calls remain gaps.

Enum replay uses the same evaluated declarations as production, including
merged enum history. Only an initializer that remains a runtime expression
binds through the generated numeric forward/reverse-write structure. Folded
values, generated names and writes remain unmapped. Structural comparison is
streamed within the existing fingerprint budget and preserves signed zero;
folded numeric values additionally require identical bits, including NaN
payloads. Direct bytecode admission also compares numeric constant bits: normal
NaN enums are admitted, while changed payloads or zero signs are refused.

Rewritten qualified-name tokens can share all their source bounds. Expression
capture now records consumed token cursors, so a new member is distinct from an
unchanged grammar tier even when their byte ranges coincide. Namespace reads
rewritten to parameter members preserve the original read's coordinates;
inserted receiver identifiers are masked using the recorded rewrite context.
Temporary tokens, paths, addresses and reference maps are discarded before
retaining the existing source-only ordinal/coordinate certificate.

Forty-two missing positions were reproduced across twenty-one shapes and both
field modes. They now pass, alongside twelve ordinary AST/bytecode controls,
eight qualified-name chains, a 25,000-declaration path refusal and an oversized
reference-map refusal with fewer than thirty-two executable slots, ten UTF-16/
newline contexts and sixteen dependency cases before/after await. Queued
namespace closures preserve their source across later code and actual major
GC, and expire on navigation. Generated export/enum write failures and dynamic
code remain unavailable; public instruction-map inspection verifies generated
receivers remain gaps. NaN/infinity/negative-zero admission and atomic mutated
constant refusals pass. All 213 bridge library cases pass in final validation.

Final acceptance passes 667 cases across thirty-nine disjoint targets, zero
failures and sixteen existing ignored: 284 bridge cases across all thirty-five
targets, forty-seven diagnostic groups, 312 launcher, nineteen real-page and
five MCP console cases. Three new page contexts (including an awaited namespace
module) and two deferred debugger contexts preserve original 2:2; MCP verifies
namespace and enum errors in two additional typed scripts. Formatting and
related library/integration/test Clippy pass with warnings denied. Twelve Rust
inputs, three Cargo inputs, three runtime and thirty-nine test executable hashes
remain fixed. Receipts are `target/namespace-origins-source-20261016.json` and
`target/namespace-origins-validation-20261016.json`. The initial green cohort
before reference-budget refinement is archived separately. Remaining dispatch/
pattern/generated precision, integrated lifecycle/resource and full workspace/
all-target/Test262 acceptance stay open; M1q remains at 3/10 final gates.

### Direct binding patterns and anonymous exports

The bridge replays each checked variable pattern with the production pattern
lowerer before binding the final pattern tree. Root positions require the
checked declaration keyword and exact original pattern spelling; child names
require their checked key/rename/rest prefixes. Default values use the existing
bounded expression-origin traversal. Written variable, ordinary/rest parameter
and catch bindings receive their own source-only slots. Erased `this`
parameters contribute no runtime binding; constructor properties keep the
written parameter name. Generated export bindings remain explicitly unmapped.
The existing direct grammar still refuses destructured parameters and nested
patterns; source capture does not change executable syntax or emitted bytes.

Anonymous default-function statements and anonymous default-class variable
wrappers now traverse the written function/class parameters and bodies rather
than discard their evidence. The wrapper's private binding receives no borrowed
location. All retained metadata remains the same private coordinate inventory
and AST certificate; checked tokens, replayed patterns and addresses are
temporary. The existing shared inventory, expression and fingerprint budgets
bound this work without adding runtime roots or debugger authority.

Forty-four unavailable positions were reproduced across twenty-two supported
shapes and both field modes, including null patterns, defaults, rest, class/
namespace bodies, native Proxy reads and iterator next/close failures. They
now pass. Eight public instruction-owner checks verify renamed/rest/parameter/
catch positions, while generated export binding instructions remain gaps.
Four unchanged AST/bytecode controls and four contextual-keyword shorthand
controls preserve the direct grammar's existing accepted bindings. Twenty
UTF-16/newline contexts, twenty-four dependency/default-export contexts before/
after await, callback/unknown-source
controls and major-GC/navigation retention pass. All 221 bridge library cases
pass in final validation.

### Ready async-module parents

The awaited-dependency cases exposed a failure in parent settlement: a ready
parent's language error escaped through the dependency's Promise job even
though its module record already owned the error. Page-owned bounded
checkpoints reported that job error and the entry's terminal module failure,
duplicating the same source. Escaping early also skipped ready siblings and
could leave an indirect entry suspended without its evaluation error.

Parent settlement now propagates a recorded catchable failure through module
waiters/static ancestors and continues its ready siblings. Both ordinary and
debugger evaluation record every catchable error, including string limits, so
these remain language completions with their original retained source. An
uncatchable resource abort still escapes unchanged. Public standalone/page
tests verify one report with known or explicitly unknown callee origins;
independent sibling work completes, repeated checkpoints do not duplicate
notifications, and instruction exhaustion remains a host abort.

### Exact copied-token maps for emitted BlueTS

The deferred host fixture checks the same dependency failure in JavaScript,
direct BlueTS and emitted BlueTS. Its emitted case exposed a gap: ordinary
TypeScript-compatible token maps can omit an opening pattern token after
trivia, while runtime diagnostic lookup correctly requires an exact anchor.
`CompilerOptions::diagnostic_source_map` adds starts for copied non-EOF tokens
when source maps are enabled. Mapped edits and verified relocations preserve
that mode, under the existing segment and payload limits. The page owner
selects it for emitted scripts. It changes metadata only, has its own
fingerprint tag, and defaults to false, preserving ordinary map output and
fingerprints. Generated JavaScript bytes are identical in both modes.

Public tests verify exact object/array/anonymous-export binding positions and
unchanged JavaScript, while generated decorator helpers/scaffolding remain
unmapped. Existing token-map oracle, System map, publication, target and output
controls pass. Frozen host artifact fixtures select the owner's diagnostic
mode and retain full map and JavaScript equality checks.

This follow-up passes 2,314 cases across fifty-nine disjoint targets with zero
failures and twenty-nine existing ignored: thirteen BlueJS targets (1,398),
eight BlueTS (287), all thirty-five bridge targets (292), launcher (313), real
pages (nineteen) and official MCP console (five). Three added page contexts,
two direct deferred classic contexts and the three-language deferred module
fixture preserve original positions. MCP adds root/default typed bindings at
2:2, preserving the earlier queued JavaScript 3:27 and 6:27 controls.
Formatting and related library/test Clippy pass with warnings denied.

`target/pattern-origins-source-20261016.json` records twenty-two final Rust
inputs, four Cargo inputs, three runtime and fifty-nine test executable hashes;
per-target receipts are in `target/pattern-origins-validation-20261016.json`.
Source/test refinements are explicit. The earlier thirteen-target BlueJS
cohort remains valid because its source/Cargo inputs and executable hashes
remain unchanged throughout the later compiler/host refinements. Final
executables are checked before and after acceptance; the changed launcher
fixture binary is rebuilt and replayed separately. Remaining dispatch/generated
precision, emitted-profile and integrated lifecycle/resource acceptance, and
full workspace/all-target/Test262 gates stay open. M1q remains at 3/10.

## Saved completions and cleanup

An abrupt finalizer saves the incoming diagnostic location, terminal debugger
site and propagation epoch with its pending completion. A normally completing
finalizer restores the saved throw with a fresh propagation epoch; an abrupt
replacement keeps its own origin. A yielded generator retains the saved
provenance in its heap-owned handler frame, independently of later scripts
and collections. Explicitly unavailable origins remain unavailable.

Iterator cleanup preserves the incoming throw's provenance when a close
failure is ignored. When closing replaces a return or jump, the first close
failure retains its own origin while outer iterators still receive cleanup.
Native collection/iterator/Promise and host cleanup paths use the same
preservation helper. Saved filename capacities participate in suspended
generator managed bytes and async/module continuation scheduling bytes.

The initial public cleanup regression failed four of five scenarios, and
the real Core/child finalizer fixture reported `:0:0`. A follow-up exposed a
cross-script generator epoch collision (14 passed, one failed). The corrected
diagnostic/error/page selection passes all 43 cases, including the original
throw, replacement, unknown-origin, GC and byte-charge controls. The complete
follow-up selection passes 1,837 cases across twenty targets, with one existing
ignored and zero failures. It covers the three related libraries, twenty
debugger groups, generator/async GC, completion, native Promise and scheduling
limits, and all sixteen real page cases. The page coordinate fixture verifies
six original-source contexts. Formatting and focused three-library Clippy
with `-D warnings` also pass (Clippy: 1m 11s). All nineteen Rust hashes in
`target/source-diagnostics-cleanup-source-20261015.json` match this complete
run; `target/source-diagnostics-cleanup-validation-20261015.json` records the
selection and remaining gates.

## Copied source records and transport

Console records now carry an optional copied `source` with filename, line
and column. Ordinary calls use their active instruction; uncaught reports
pass their captured origin explicitly after dispatching error handlers.
Unknown callbacks use an absent location even if a preceding root script
reported a mapped exception. No error-value source property is evaluated.

The private child channel is v61. The public field has a serde default, is
omitted when unknown and preserves the coarse public protocol v2. Filename
bytes are bounded at 8 KiB and share the existing 256 KiB console text limit
with messages. VM eviction and native accounting measure owned capacities;
wire validation measures copied UTF-8 text. Core and MCP each validate the
bounded record before publication. Filenames remain untrusted data inside
MCP's existing page-content frame and confer no loading/debugger authority.

The red baseline comprises two VM failures, one real Core/child failure and
three IPC failures. The first implementation passes the IPC controls and
eighteen VM cases but exposes a stale root-to-unknown-timer fallback. An
explicit root-reporting origin removes that fallback. Core's two dedicated
tests pass, including ten malformed-reply variants. The matched Core/child
page rerun passes all seventeen cases, and the unchanged Cargo-built official
MCP executable passes all three console cases (nine filtered, 1.71s). Known
JavaScript sources survive messages, tasks, native events and navigation;
direct BlueTS and unmapped code keep their explicit absence.

The final disjoint selection has 2,117 passes across twelve targets and one
existing ignored. The first real-process attempts used an older Core binary
and failed startup; rebuilding the matching private v61 Core/host binaries
resolved those attempts before acceptance. Formatting and focused six-library
Clippy with `-D warnings` pass (Clippy: 2m 48s). The thirteen Rust source hashes
and validated runtime/test hashes are in
`target/console-sources-source-20261015.json`; the per-selection results and
remaining gates are in `target/console-sources-validation-20261015.json`.

## Promise rejection provenance

A rejected native Promise retains a copied throw origin separately from its
JavaScript reason. Settlement jobs and already-settled await registration
carry that origin to later turns. A missing rejection handler and native
forwarding preserve it; a JavaScript callback captures its own new throw or
reject call, including an explicitly unavailable dynamic-code origin. Each
job displaces the ambient interpreter source and throw context. Rejected
awaits publish a fresh propagation epoch before entering catch/finally.

Native `finally` thrower thunks retain their original rejection provenance
in a weak side table owned by the thunk's heap state. Collection prunes the
metadata without rooting the Promise, reason or thunk. No JavaScript-visible
property stores diagnostic metadata, and no reason property is evaluated.
Promise records, queued/active jobs, displaced native scopes and retained
thunk filename capacities participate in document scheduling accounting.
The default rejection console report snapshots the retained origin before
event handlers run, preserving the original diagnostic if a handler throws.

The first public regression exercised nineteen forwarding/callback/await/
finally/species/async-generator scenarios: eighteen known origins were absent,
while the unknown-code control remained unavailable. The first implementation
passes all nineteen. The corresponding real MCP rejection assertion also
failed before the implementation because the original inline source was
absent. The expanded regression now passes all 32 source scenarios and all
23 diagnostic groups, including full-GC ownership and native-byte refusal
controls. Real MCP acceptance verifies the original inline line 4, column 25
after `finally` and keeps direct BlueTS unavailable. The final disjoint
selection passes 1,837 cases across twenty targets, with one existing ignored;
format and focused six-library Clippy pass. The initial sandboxed socket runs
failed during setup; the same compiled binaries pass with local socket access.
The fifteen Rust hashes and runtime/test hashes are recorded in
`target/promise-origins-source-20261015.json`; per-target results and remaining
gates are in `target/promise-origins-validation-20261015.json`.
This verifies a scoped implementation step, leaving M1q at 3/10 final gates.

Queued async-generator requests retain an immutable origin with their FIFO
entry. Queue snapshots share this native metadata; its allocation and filename
capacity are charged to the generator's managed heap bytes. Each head runs
under its own native provenance scope. Injected throws publish fresh epochs
before catch/finally can save them. A rejected return operand or a throwing
getter replaces the request origin; a fulfilled operand preserves it.
Delegate failures publish their own origin before entering the generator,
and an abrupt finalizer replaces that origin rather than retaining it.
No additional JavaScript roots or observable diagnostic properties are added.

The first request regression lost four of nine origins. A twelve-scenario
follow-up exposed two delegate/finalizer failures after the first nine passed.
The final twelve scenarios pass with normal and aggressive GC, including
unknown origins and abrupt replacement. Distinct known/unknown FIFO entries
with identical rejection values keep their own sources across later scripts;
a release assertion observes major collection before checking the weak owner.
Heap tests verify actual filename capacity charges and atomic refusal without
replacing the original request. All 26 diagnostic groups pass.

Real MCP reproduces an absent queued-throw source against the unchanged old
runtime binaries, then validates inline line 5, column 25 after `.throw()` and
`finally` with the new runtime. The final disjoint selection passes 1,841
cases across twenty targets, one existing ignored, plus formatting and focused
six-library Clippy. Launcher Python helper startup failures under concurrent
load and one serial timeout are recorded; an isolated complete replay of the
identical executable passes all 307 cases with four test threads. The seven
Rust hashes, runtime/test hashes and per-target receipts are in
`target/generator-request-origins-source-20261015.json` and
`target/generator-request-origins-validation-20261015.json`. This extends the
verified implementation; M1q's final acceptance remains at 3/10.

## Cached module completions

Module evaluation errors now retain the rejection value with immutable native
provenance. A cached import republishes the stored origin with a fresh throw
epoch. Async dependency failures share their origin across static parents,
and every waiting dynamic import receives the same certified source. Page
module error events use the stored dependency location. Unknown generated
code remains unavailable; a module path or later importer supplies no fallback.

A weak native ledger charges origin allocations and filename capacities even
while a linked graph is displaced into a Rust frame. It adds no JavaScript
roots and releases dead entries during collection. Admission includes ledger
growth before publishing a completion. Loader referrers are distinguished from
active module execution so an installed idle referrer cannot suppress host
rejection notification. Await continuations restore their ambient execution
state, and debugger resumption preserves enclosing checkpoint cleanup.

The initial public regressions report an unavailable dependency source and
zero import rejection notifications with an installed loader referrer. The
final selection passes all 1,487 BlueJS cases across twenty-six targets,
including 1,204 library cases and thirty diagnostic groups. It verifies
synchronous and awaited dependency errors, cached import replay, async parent
fan-out, rejected-await and unknown-code sources, argument getters, debugger
resumption, shared capacity charges, displaced ownership, major GC and native
admission refusal. Bridge, Launcher, real page and official MCP acceptance
bring the total to 1,981 passes over thirty targets, one existing ignored.
Formatting, matching Core/host/Gatekeeper builds and six-library Clippy pass.

Old build artifacts were cleared before validation; incremental caching and
dev/test debug info are disabled. The first MCP attempt stopped at a missing
Gatekeeper binary; the complete matched runtime passes all three cases.
The thirteen final Rust input hashes, three runtime hashes, test hashes and
receipts are in `target/module-origins-source-20261015.json` and
`target/module-origins-validation-20261015.json`. This verifies retention of
owner-certified module maps; original page-module/debugger source production
remains a separate open requirement. M1q remains at 3/10 final gates.

## Disposal completions

Both `await using` and the private `AsyncDisposableStack` helper now compile
the same awaited resource loop. A disposal catch folds its error into the
enclosing finalizer's native pending completion. The finalizer forwards that
completion with its saved source/debugger origin and a fresh epoch, instead
of generating a JavaScript rethrow. Successful cleanup preserves the incoming
origin; a new disposal failure replaces it, including explicit absence.
Synchronous disposal also preserves its chosen origin across later successful
disposers that catch unrelated errors.

Caught origins move through the existing handler-frame field. Suspended
generator and async/module frame accounting already measures those filename
capacities. No diagnostic property or additional JavaScript root is added.
Suppression uses the intrinsic construction path, consistent with
[DisposeResources](https://tc39.es/ecma262/multipage/abstract-operations.html#sec-disposeresources),
without calling a replaced global `SuppressedError` constructor.

The first ten-scenario matrix loses eighteen origins under two GC settings.
The corrected thirty-two diagnostic groups pass, including module and queued
generator disposal, source capacity deltas, cross-script suspension, observed
major collection and owner release after a completed host checkpoint. The
initial owner-release assertion ran collection in the same job that cleared
references; the final assertion uses a separate checkpoint and collection job.
The real MCP fixture also reproduces an absent original source with unchanged
prechange Core/host binaries after generator/finally/stack/`await using`
forwarding.

The final selection passes all 1,547 BlueJS cases across thirty targets,
including the complete thirty-two-group diagnostic target and resource
management, GC and allocation/instruction-refusal regressions. Bridge,
Launcher and real page tests add 491 passes and one existing ignored; all
three official MCP console cases pass. With the matching new Core/host, the
original inline line 5, column 25 survives the complete disposal chain.
The disjoint total is 2,041 passes over thirty-four targets, zero final
failures. Formatting, matching runtime/Gatekeeper builds and focused
six-library Clippy pass. Fifteen Rust inputs, old/new runtime hashes, test
hashes and per-target receipts are recorded in
`target/disposal-origins-source-20261015.json` and
`target/disposal-origins-validation-20261015.json`. M1q stays at 3/10 final gates.

## Required remaining work

Remaining expression dispatch, decorator application and other generated-code
provenance still need completion. Direct BlueTS requires its remaining precise
expression/pattern and generated-lowering evidence; emitted BlueTS and
page-module/debugger producers require their
remaining precision, emission-profile and integrated lifecycle acceptance.
The complete queued-origin matrix, including remaining delegate/protocol
cases and integrated finalizer/disposal acceptance beyond the verified cases,
remains part of M1q.
Other copies held by asynchronous diagnostic state must join document-native
accounting. Navigation, stale generations, GC, source-map refusals and resource
containment need integrated
coverage. Broader host-buffer/network/UI/default-on policy remains separate.

No source position may be inferred from a nearby token, an unrelated later
callback, a page-selected source-map URL or an unauthenticated generation.
Unknown and generated-only locations remain explicitly unavailable. Final
related Rust/Core/MCP, formatting, workspace Clippy/build and complete pinned
Test262 gates have not been claimed.

## Foundation implementation checkpoint (2026-10-15)

Commit `ce8c45707` records the foundation. The BlueJS, BlueTS bridge and launcher
library selection plus public
registry/lifecycle targets passed 1,691 cases, with one existing ignored.
After two Clippy-only type-alias corrections, the final public diagnostic,
host-error and real Core/child page replay passed all 40 cases. All 27 Rust
source hashes in `target/source-diagnostics-foundation-source-20261015.json`
match that final replay. The combined eight-target checkpoint has 1,731
passing cases and one existing ignored. Formatting and focused three-library
Clippy with `-D warnings` pass.

The [TODO progress checklist](BROWSER_INTEGRATION_TODO.md#milestone-record)
records the verified implementation steps and remaining work. At the foundation
checkpoint, two of the ten full M1q acceptance gates were complete. That
checkpoint does not claim
workspace/all-target Clippy/build, final Core/MCP acceptance, a fresh pinned
Test262 run or fresh coverage.
