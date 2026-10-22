# Minimum ECMAScript declarations (K.1.4)

`backend/bluets/src/standard_library/` contains original declarations for a
selected ECMAScript surface. They were written for BlueTS's declaration
subset; they do not copy TypeScript's standard library. The pinned TypeScript
5.9.3 oracle uses `--lib es2020` or `--lib es2022` to check the selected forms.
This is a minimum library, and full parity remains off.

The current `blue-ts-ecma-lib-v3` identity selects cumulative ES5–ES2023
profiles. ES2020/ES2022 retain the original base sources, v2 additions and
async-iteration declarations; ES2022 also selects its original additions.
Every profile includes the original decorator context declarations and the
eligible original v3 API layers (core/buffers, iterable/Promise, ES2017–ES2022).
A content digest includes the
version, target, source names and exact source bytes. The compiler fingerprint
includes that identity, and `bluetsc.manifest.json` records `standardLibrary`.
An explicit library selection is independent of the output target and is
included in that identity. The sources are embedded and parsed once per
selected edition. They never enter an
owner's source graph, consume its module budget, produce debugger symbols,
write artifacts, or acquire a loader or runtime binding.

Local and owner declarations take precedence over each library name. With
`require_declared_global_calls`, the library supplies types and static name
recognition while value/function bindings continue to require a local or
owner declaration. Console retains its existing name-only compatibility;
DOM and other host APIs require owner declarations.

Implicit, unused library constants do not make an unrelated opaque receiver
readonly. Lexical binding identities distinguish library defaults from local
and owner shadows. Explicit library references, including alias initializers,
retain the existing conservative readonly protection.

Primitive and array syntax look up properties through the selected interface
definitions. Array syntax retains its element type when compared with the
original Array/ReadonlyArray definitions; owner replacements use ordinary
structural checking. Iterable and the collection/array iterator interfaces have
declared computed well-known-symbol keys. Those identities come from trusted
declarations, not a source's same-spelled shadow of `Symbol`. Type-only namespace
queries bind static exported types without introducing a runtime value.

G-T10 adds an original conditional `ReturnType<F>` declaration for its frozen
generic-function import-query witnesses. It uses the existing conditional/infer
operators, not an unchecked utility-name shortcut. Owner-local and imported
`import('specifier').Type` / `typeof import('specifier')` queries reuse the
authorized project graph and static export surfaces; they do not acquire a
runtime loader, execute dependencies or broaden the library catalog. This
finite matrix does not establish every utility or recursive query composition.

The parser and checker already admit generic call/construct signatures and
their overloads. G-T8 also supports signature-only declaration-file classes,
while refusing runtime bodies/initializers and source-file `declare class`.
The v3 Date, RegExp and Error-family declarations use actual call and construct
signatures. Refined layers replace legacy opaque members without leaving the old
unchecked namespace/function path as an alternate route. Interface augmentations
retain generic variance/defaults and all constructor overload groups. Their
instance interfaces and static fields remain parsed declarations.

Array callbacks may ignore trailing arguments supplied by the caller. Their
consumed parameter types and return types remain checked; broader optional
and rest-parameter function compatibility remains in the type-system inventory.
Method checking selects each postfix receiver independently of surrounding
operators and nested arguments, including calls on generic function results.
Unary operators retain their own result type instead of adopting a method's result.
Enum members use their underlying primitive for property lookup while assignment
keeps their nominal identity. A selected `while (!result.done)` loop projects the
yield branch only inside its matched body and before a write to the same lexical
binding; shadowed bindings and following statements do not inherit the guard.
General discriminant and control-flow narrowing remains K.4.

| Surface | Selected behavior | Recorded omissions |
| --- | --- | --- |
| Array/ReadonlyArray | numeric indices, computed iteration, rest push/unshift, constructors/from/of, generic map/flatMap/reduce and inferred predicate filter; selected ES2022 methods | full overload/prototype catalog, polymorphic `this`, arbitrary callback and nested protocol compositions |
| String/Number/Boolean | call/new constructors, boxed methods, selected static/locale/normalization/replace/match forms | complete prototype/static catalogs, locale options and overload combinations |
| Object/Function | assign/entries/values/fromEntries, descriptors, freeze/seal/create, selected call/apply/bind/new forms | all utility overload combinations, generalized receiver/bind/descriptor compositions |
| Promise | executor constructor, resolve/reject, value/thenable adoption, then/catch/finally, tuple all, race/allSettled/any | arbitrary recursive/custom thenables, full aggregate/overload compositions and post-ES2022 APIs |
| Map/Set/WeakMap/WeakSet | generic constructors, methods/callbacks, entries/keys/values and computed iteration | full weak-key and iterable-input combinations, arbitrary custom iterators |
| Math/JSON | selected full numeric families/constants, parse reviver, stringify callback/key-list/spacing results | complete overload combinations and inference for arbitrary reviver/replacer bodies |
| Symbol | primitive/boxed adapter, registry/factory, unique well-known computed keys and description | complete well-known key catalog and arbitrary owner-symbol protocol compositions |
| Error family | seven callable/new constructors, ES2022 cause/options, iterable AggregateError inputs | complete overload and subclass combinations |
| Iterable/Iterator/Generator | generic element/return/next types, computed sync/async keys and selected iterator result protocols | arbitrary protocol composition and full async-generator/iterator catalogs |
| Date/RegExp | call/new forms, selected component/UTC/set/get/locale overloads, match arrays/groups/indices/flags and regexp literal receivers | complete locale/overload combinations and general regexp lexical contexts |
| Reflect/Intl/BigInt | thirteen Reflect operations, seven Intl constructor/method families, canonical locales, BigInt call/truncation | complete Intl option/prototype catalogs, arbitrary generic Reflect compositions |
| Buffers/DataView/typed arrays/Atomics | buffers/shared buffers, numeric/bigint DataView, all eleven typed arrays and selected static/map/slice/subarray/buffer forms, numeric/bigint Atomics | complete prototype/overload catalogs and arbitrary buffer/iterator compositions |

The frozen G-T7 manifest is 217 native accept/reject pairs, fourteen library
edition/ES5-output controls, 224 exact declaration graphs/files, 434 downstream
diagnostic observations and five executions. Five separate owner controls are
not native agreement. The manifest is finite, not a claim that every combination
of the declarations above matches TypeScript. Completion and source-frozen
measurement are tracked separately in TODO.md and PLAN.md.

Broader generic inference, structural protocols, overloads, index signatures
and declaration precision remain K.4/K.8 and the compatibility inventory.
Each additional form needs a pinned oracle before the minimum set expands.
