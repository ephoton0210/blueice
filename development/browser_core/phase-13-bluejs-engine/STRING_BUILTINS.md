# String builtins — ECMAScript 2026

Requested 2026-09-09: implement the **complete** String builtin surface, not just storage. Status: **in progress**. The UTF-16 migration is a prerequisite, not the completion criterion. Follow the [edition 17 track](ECMASCRIPT_2026.md), public-pipeline TDD and 100% BlueJS line gate.

## Authoritative inventory

The published [§22.1 String Objects](https://262.ecma-international.org/17.0/#sec-string-objects) inventory was fetched on 2026-09-09. Read each algorithm before implementing it. The constructor, static methods, indexing, concatenation, searching, slicing and well-formedness clauses were also read that day. Other rows require their own algorithm review before implementation.

| Area | Required surface / dependencies |
| --- | --- |
| Constructor/statics | `String`, `new String`, `fromCharCode`, `fromCodePoint`, `raw`, constructor/prototype links |
| Indexed access | `length`, indexed properties, `at`, `charAt`, `charCodeAt`, `codePointAt` |
| Search/slice | `endsWith`, `includes`, `indexOf`, `lastIndexOf`, `startsWith`, `slice`, `substring` |
| Build/trim | `concat`, `padEnd`, `padStart`, `repeat`, `trim`, `trimEnd`, `trimStart` |
| Unicode/value | `isWellFormed`, `toWellFormed`, `normalize`, `toLowerCase`, `toUpperCase`, `toString`, `valueOf` |
| Locale | `localeCompare`, `toLocaleLowerCase`, `toLocaleUpperCase`; distinguish ECMA-262's non-402 behavior from the separate ECMA-402 project |
| Replacement/splitting | `replace`, `replaceAll`, `split`, substitution patterns, callable replacement and symbol dispatch |
| RegExp | `match`, `matchAll`, `search`, RegExp integration for replacement/split, IsRegExp and symbol hooks |
| Iteration | `[Symbol.iterator]`, String iterator objects/prototype and code-point iteration |
| Legacy/browser | Audit edition 17 Annex B String extensions, HTML wrappers, `substr`, `trimLeft`/`trimRight` aliases and applicability |
| Cross-cutting | Generic receivers, object coercion/error ordering, native function metadata/descriptors, boxed String exotic properties, strict/sloppy writes, constructors/subclassing, realms, GC and bounded allocation |

## Execution design

Add native function identities to heap-owned objects, not ad-hoc AST recognizers for named String calls. Extend fixed-width bytecode with calls that evaluate the receiver/callee before arguments, retain `this` for member calls and keep all call inputs rooted through allocation. Locals may shadow the global `String`; detached methods and borrowed methods must use their actual receiver. Initialize String intrinsics lazily per VM so scripts not using them retain their existing small-heap footprint. Temporarily root the graph while building it and keep that root permanently only after successful bootstrap; failed setup must not leak registrations. All growing bootstrap stores must additionally root VM-held values. Intrinsics live for the VM lifetime, while execution bindings are currently reset per `execute`; this is not a complete realm/global-environment implementation.

Keep UTF-16 in all algorithms. Check resulting sizes before repeating/padding/concatenating allocations. User-defined functions, RegExp, Symbol dispatch, Unicode data and full descriptors are genuine dependencies in the inventory, not blanket exclusions from the request. Record completed methods and remaining semantic gaps separately: mere presence of a callable property or passing selected primitive cases does not count as full method conformance.

### Unicode implementation decision

On 2026-09-09, Unicode's [latest published version](https://www.unicode.org/versions/latest/) resolves to **17.0.0**. Edition 17 §22.1.3.15 requires normalization according to the latest Unicode standard. Use `unicode-normalization` **0.1.25**, whose [upstream release](https://github.com/unicode-rs/unicode-normalization/commits) updates its tables to Unicode 17; this is a text algorithm/data dependency, not an embedded JS runtime. Rust's default case conversion supplies Unicode casing, with its Unicode version asserted by tests. Process well-formed runs independently and preserve lone surrogate code units as boundaries; neither casing nor normalization repairs them. Output growth is checked as transformed code points are appended. Locale-specific behavior and ECMA-402 are not inferred from these locale-insensitive operations.

Also read on 2026-09-09: §20.2.3.3 `Function.prototype.call`; §22.1.3 padding/repeat/trim, `String.raw`, string-search `split`/`replace`/`replaceAll` and GetSubstitution, casing/normalization/locale method requirements; Annex B `substr`, CreateHTML/all 13 HTML wrappers, and [trimLeft](https://262.ecma-international.org/17.0/#String.prototype.trimleft)/[trimRight](https://262.ecma-international.org/17.0/#String.prototype.trimright) identity aliases.

## Implemented foundation and remaining completion requirements

The runtime now has `String(...)`, `new String(...)`, all three static method entry points and **28 of the 35 core prototype method entry points**, plus the **16 Annex B properties** (14 methods and two identity aliases). Counts describe the callable surface, not full conformance of each method.

- UTF-16 `length`/index reads, `at`, `charAt`, `charCodeAt`, `codePointAt`; boxed String storage with virtual read-only/non-configurable indices and length.
- `concat`, `endsWith`, `includes`, `indexOf`, `lastIndexOf`, `startsWith`, `slice`, `substring`, padding/repetition and all three trim methods.
- `isWellFormed`, `toWellFormed`, all four normalization forms, default Unicode case conversion, `toString`/`valueOf` brand checks.
- `String.raw` for supported array-like inputs, String-search splitting/replacement, `$$`/`$&`/prefix/suffix substitutions, native-function replacement callbacks. Unknown capture patterns remain literal for String searches, as required.
- Annex B `substr` and HTML wrappers, escaping only quotation marks in attribute values; trim aliases share the original function object/name.
- Heap-owned native function identities, compiler `GlobalString`/`GetMethod`/`Call`/`Construct` instructions, `this`-preserving member calls, generic primitive receivers through `.call`, string-size checks, GC rooting and native raw/split/replace loop fuel. Existing compiled-byte limits cover new call/construct instructions too.

**Not complete:** seven core method entry points are still absent: `localeCompare`, `toLocaleLowerCase`, `toLocaleUpperCase`, `match`, `matchAll`, `search`, and `[Symbol.iterator]`. They require locale policy/collation, RegExp and Symbol/iterator support. Unicode normalization and default case conversion do not substitute for locale semantics.

Cross-cutting gaps also remain for methods already present: full callable `ToPrimitive`/object argument coercion; observable overrides on boxed Strings (the current path directly unboxes); user-defined replacement functions; `IsRegExp` and symbol protocol hooks; native function/property descriptors (current stored metadata/method properties still have ordinary data-property attributes); strict-mode write behavior, complete Function intrinsics and general constructor/subclass/realm semantics. `String.raw` does not yet have tagged-template syntax. Resource ceilings are implementation limits, not a substitute for spec error categories; byte/heap budgets are not RSS limits, and dispatch fuel is not a hard native-work/wall-clock bound. These gaps remain part of the user's complete-String request.

### Test-content review

Seven storage tests and twelve builtin integration tests drive public APIs. Review added exact UTF-16 payload accounting, distinct surrogate keys and old-to-young edges, boxed-vs-array virtual-property behavior, surrogate-splitting indices/padding, contextual casing, canonical/compatibility normalization, assignment/call evaluation order, prefix/suffix replacement patterns, empty searches/separators, signed/infinite/fractional positions, primitive borrowing, bootstrap retries at six heap ceilings, native-loop limits and repeated execution. The GC pressure test found a real missing-root bug in bootstrap property stores; it failed before all such stores were routed through VM safepoints. Host tests comparing separate VMs compare observable primitive results, never heap-local object IDs.

The independent oracle now transmits result strings as exact UTF-16 code units. It tests all 2,048 lone surrogates via both escapes and `fromCharCode`, plus 1,430 generated position/search cases and 128 additional fixed scripts. Review also caught cross-fixture prototype contamination after intrinsics became mutable: a two-script probe failed before the harness changed from reusing one VM to creating a fresh VM per script, matching Node's fresh realm.

### Validation (2026-09-09)

`cargo llvm-cov -p blueice-bluejs --fail-under-lines 100 --summary-only -- --quiet` passes: **3,562/3,562 lines (100%)**, 394/394 functions, 95.69% regions, with no engine-file exclusions. **174 default unit/integration tests** and two doc examples pass. Node.js **v24.18.0** (Unicode 17.0, ICU 78.3) passes **16,498 isolated scripts**. Workspace all-target build/Clippy (`-D warnings`), full workspace tests, crate rustdoc and the array-summing example pass. Workspace real-socket tests ran outside the socket-restricted sandbox; the final byte-budget test extension, oracle-isolation fix and widened String key-sort indices were validated by the subsequent BlueJS gate/oracle and workspace Clippy (the key-sort-only change followed the oracle run). Full workspace coverage was not rerun. **Neither complete String support nor complete ECMAScript conformance is claimed.**
