# Minimum ECMAScript declarations (K.1.4)

`backend/bluets/src/standard_library/` contains original declarations for a
selected ECMAScript surface. They were written for BlueTS's declaration
subset; they do not copy TypeScript's standard library. The pinned TypeScript
5.9.3 oracle uses `--lib es2020` or `--lib es2022` to check the selected forms.
This is a minimum library, and full parity remains off.

The `blue-ts-ecma-lib-v1` identity selects the base declarations for both
targets and an additional file for ES2022. A content digest includes the
version, target, source names and exact source bytes. The compiler fingerprint
includes that identity, and `bluetsc.manifest.json` records `standardLibrary`.
The sources are embedded and parsed once per target. They never enter an
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
structural checking. The otherwise empty Iterable interface retains its
element compatibility without inventing a computed iterator property that
the parser cannot represent.

The declaration parser does not admit `declare class` or construct signatures.
Date, RegExp and Error-family constructors therefore use parsed function-type
descriptors named `*Constructor`, connected to the existing `new` checker.
Their instance interfaces and static fields remain parsed declarations.
The parser's existing refusals stay in place.

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
| Array/ReadonlyArray | element types, length, push/pop, basic search, join, slice, sort and filter; ES2022 `at` | numeric index signatures, computed iterator keys, multi-item/zero-item push and unshift, additional methods, polymorphic `this`, predicate narrowing and generic callback result inference; `map` retains an opaque result |
| String/Number/Boolean | boxed types and selected primitive methods | the complete prototype catalogs, overloads and locale variants |
| Object/Function | selected properties, string conversion, keys and property names; ES2022 `hasOwn` | complete constructors, reflective descriptors, callable/apply/bind signatures and object utilities |
| Promise | typed resolve, rejection, awaited values and fulfillment parameters | constructor overloads, aggregate operations, thenable adoption and precise chaining results |
| Map/Set/WeakMap | generic instance methods and size; object-key constraint for WeakMap | generic constructors, entries/keys/values, computed iteration and the full weak-key model |
| Math/JSON | selected numeric functions with checked arguments; parse/stringify, callback/key-list replacers and spacing; stringify's pinned declaration result | remaining Math methods, JSON parse reviver and the complete overload catalog |
| Symbol | boxed/primitive adapter, registry functions and callable factory | unique-symbol checker identity and well-known computed keys; factory const declarations retain tested `unique symbol` spelling |
| Error family | name/message/stack and selected `new` signatures; ES2022 AggregateError | callable constructor forms, cause/options overloads and all iterable AggregateError inputs |
| Iterable/Iterator/Generator | generic element/return/next types and selected protocol methods | computed Symbol.iterator, async iteration, full protocol structural checking |
| Date/RegExp | selected instance/static methods and `new` signatures | callable forms, remaining overloads, UTC arities, regexp match-array/index metadata and the full catalogs |
| Other ECMAScript names | existing opaque compatibility for Reflect, Intl, BigInt, buffers, typed arrays and Atomics | typed declarations and method precision; these entries confer no runtime authority |

Broader generic inference, structural protocols, overloads, index signatures
and declaration precision remain K.4/K.8 and the compatibility inventory.
Each additional form needs a pinned oracle before the minimum set expands.
