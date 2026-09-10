# String builtins — ECMAScript 2026

Requested 2026-09-09: implement the complete String builtin surface. The constructor, three statics, all **35 core prototype methods** and **16 Annex B properties** (14 methods and two identity aliases) now have implementations, including their conversion, callback, Symbol, RegExp and iterator paths. This is a String implementation within BlueJS's supported runtime, not a completed edition-wide or Test262 conformance claim. Follow the [edition 17 track](ECMASCRIPT_2026.md).

## Project and recent commit analysis

BlueIce owns its browser pipeline in Rust. `backend/bluejs` is its independent JavaScript library: source → tokenizer/parser → AST → fixed-width bytecode → VM, with a heap of stable handles and nursery/tenured garbage collection. Browser process/DOM integration remains a separate Phase 13 task.

| Commit | Foundation used by this change |
| --- | --- |
| `f97158f` | Complete String method surface and supporting object, callback, Symbol, iterator and RegExp protocols |
| `76e001e` | UTF-16 strings, boxed String exotic properties, native calls and 28/35 core String methods |
| `ed814bc` | TDZ, ECMAScript 2026 lexical corrections, source newline and numeric rules |
| `fcc3126` | Conformance regressions, Node oracle and mandatory 100% BlueJS line coverage |
| `6f88756` | Sparse arrays, virtual length, holes and truncation |
| `1321d26` | Compiler and bounded bytecode VM |
| `fb48e83` | Ordinary objects, prototypes, roots and generational GC |

The missing String methods required runtime capabilities, not just new names in a builtin table. The implementation extends this compiler/VM/heap path without embedding another JavaScript engine.

## Implemented inventory

| Area | Surface and semantics |
| --- | --- |
| Constructor/statics | `String`, `new String`, `fromCharCode`, `fromCodePoint`, `raw`; constructor/prototype links; primitive Symbol special case; `Reflect.construct` custom target prototypes |
| Indexed access | UTF-16 `length`, indexed properties, `at`, `charAt`, `charCodeAt`, `codePointAt`; read-only/non-configurable String indices and length |
| Search/slice | `endsWith`, `includes`, `indexOf`, `lastIndexOf`, `startsWith`, `slice`, `substring`; IsRegExp and observable conversion order |
| Build/trim | `concat`, `padEnd`, `padStart`, `repeat`, `trim`, `trimEnd`, `trimStart`; growth checks and empty-result boundaries |
| Unicode/value | `isWellFormed`, `toWellFormed`, four normalization forms, default case conversion, `toString`, `valueOf`; lone surrogate preservation and brand checks |
| Locale | `localeCompare`, `toLocaleLowerCase`, `toLocaleUpperCase`, using the non-ECMA-402 contract below |
| Patterns | `match`, `matchAll`, `search`, `replace`, `replaceAll`, `split`; five Symbol hooks, callable replacements, generic/custom exec results and global-pattern checks |
| Literal patterns | `RegExp.escape`; strict String input, leading ASCII alphanumeric escaping, syntax/control/punctuation/whitespace categories, paired/lone surrogate handling |
| Iteration | String, Array and RegExp String iterators; code-point advancement, spread, `for…of`, iterator identity and abrupt loop closing |
| Templates | Tagged-template syntax, cooked/raw values, invalid tagged escapes, frozen cached template arrays, parser-selected RegExp lexical goals inside placeholders |
| Legacy/browser | `substr`, all 13 HTML wrappers, quotation-mark escaping, `trimLeft`/`trimRight` identity aliases |
| Dependencies | Compiled functions/arrows, captured binding cells, `this`, default/rest parameters, call/apply/bind, bound construction, `instanceof`/`Symbol.hasInstance`, throw propagation, Symbol keys, descriptors/accessors, strict/sloppy writes/deletes, primitive boxing and observable ToPrimitive |

`property.rs` defines public `JsSymbol`, `PropertyName` and partial `PropertyDescriptor`. Existing string-only `Heap::own_keys` remains available; `own_property_keys` includes Symbols. The heap stores/traces descriptors without calling JavaScript; the VM handles getters, setters and coercions. Primitive String setters receive the original primitive receiver. Array conversion observes `join`; cyclic array joins terminate. Native function stringification uses immutable initial names. Compiled functions use the permitted `HostHasSourceTextAvailable = false` policy and produce NativeFunction syntax rather than retained source text.

## RegExp and locale decisions

`regress` **0.12.0**, with its `utf16` feature, supplies standalone ECMAScript pattern compilation/matching, including `d g i m s u v y` flags, captures, named groups, backreferences, lookarounds and Unicode sets/properties. BlueJS owns the constructor/prototype, `lastIndex`, sticky/global execution, UTF-16 indices, capture result objects, species construction, replacement and split algorithms. Named indices reference the same pair arrays as numbered indices, including nested captures with identical ranges and duplicate names in alternatives.

`unicode-normalization` **0.1.25** and Rust casing use Unicode **17.0.0**, asserted by tests. Well-formed runs are transformed independently; lone surrogates remain boundaries. `icu_collator` **2.2.0** supplies fixed root collation with canonical equivalence. Under ECMA-262's non-402 contract, locale methods use this fixed default and ignore reserved locale/options arguments; locale casing uses default Unicode mappings. `Intl`, locale negotiation and locale-specific ECMA-402 behavior are not implemented by this change.

Reviewed primary algorithms on 2026-09-09: [edition 17 String/RegExp objects, GetSubstitution and iterator algorithms](https://tc39.es/ecma262/2026/multipage/text-processing.html), [String exotic and ordinary property algorithms](https://tc39.es/ecma262/2026/multipage/ordinary-and-exotic-objects-behaviours.html), [conversion/property operations](https://tc39.es/ecma262/2026/multipage/abstract-operations.html), [Function stringification and HostHasSourceTextAvailable](https://tc39.es/ecma262/2026/multipage/fundamental-objects.html#sec-function.prototype.tostring), and [Annex B String extensions](https://262.ecma-international.org/17.0/#sec-additional-properties-of-the-string.prototype-object).

Edition 17 RegExp `@@match` and `@@replace` read `flags`; Node 24 still reads the older `global`/`unicode` properties on custom receivers. `edition_17_regexp_flags_and_species_order_are_observable` asserts the published algorithm separately and is intentionally absent from the Node corpus. SpeciesConstructor resolution precedes the flags getter for `@@matchAll` and `@@split`.

## Resources and remaining conformance scope

Intrinsics initialize lazily per VM, remain rooted after successful setup and roll back failed bootstrap edges. Closures, captures, bound targets/receivers/arguments, iterators, descriptor values/accessors and thrown objects participate in GC rooting. A thrown or returned object survives until the next `execute`; execution bindings are fresh each time. Nested functions share the compiled instruction-byte budget. Dispatch, native loops and callbacks consume fuel; runtime calls have a 32-frame depth limit returning RangeError. Bound wrappers add no execution frames: calls and intrinsic instanceof delegation traverse them iteratively with fuel, and calls concatenate their argument prefixes once in target-first order.

The heap budget accounts for managed records, UTF-16 payloads, keys/descriptors and internal capture/name data, not allocator capacity, compiled regex code, bytecode/AST storage or total RSS. The string limit bounds each runtime string. **Regress does not expose a matcher step budget or hard timeout: a single backtracking match or compilation is not bounded by VM instruction fuel.** Template placeholder extraction validates successive closing-brace candidates with the expression parser; source parsing is not covered by VM execution fuel. These are explicit resource limits, not language conformance exceptions.

Supporting language features remain incomplete: BigInt, comprehensive classes/super, proxy functions, full argument/environment semantics, complete modules/direct eval, and complete Object/Function/RegExp/Array builtins are separate work. A persistent classic-script global realm is now implemented, but multiple realms and complete global-environment semantics are not. Arbitrary programs using those dependencies cannot yet be used to claim complete String/Test262 conformance. RegExp.escape is implemented; the complete RegExp Test262 inventory remains open.

## Test review and validation

### Continuation design (2026-09-09)

The preceding slice is commit `f97158f`. This continuation implements [Function.prototype.bind](https://tc39.es/ecma262/2026/multipage/fundamental-objects.html#sec-function.prototype.bind), [bound exotic call/construct](https://tc39.es/ecma262/2026/multipage/ordinary-and-exotic-objects-behaviours.html#sec-bound-function-exotic-objects), [OrdinaryHasInstance](https://tc39.es/ecma262/2026/multipage/abstract-operations.html#sec-ordinaryhasinstance), [InstanceofOperator](https://tc39.es/ecma262/2026/multipage/ecmascript-language-expressions.html#sec-instanceofoperator), and [RegExp.escape/EncodeForRegExpEscape](https://tc39.es/ecma262/2026/multipage/text-processing.html#sec-regexp.escape), read before implementation against published edition 17.

Use an immutable heap bound-function record with target, bound receiver/arguments and the target's constructor capability. Trace all internal object edges and charge retained argument/string/Symbol payloads. Create the bound object with the target's actual prototype before reading its own length and observable name; define only configurable, non-writable/non-enumerable length/name properties. Dispatch bound chains iteratively with fuel, prepend arguments and substitute newTarget only on identity with the current bound wrapper. Add the instanceof opcode and the non-writable/non-configurable Function.prototype Symbol.hasInstance method; respect custom methods and bound-target delegation, including primitive operands and throwing accessors. Iterative internal traversal avoids unbounded Rust recursion.

RegExp.escape rejects non-String arguments without coercion, decodes UTF-16 code points without replacing lone surrogates, implements leading ASCII alphanumeric hex escaping plus the specified syntax/control/punctuation/whitespace rules, and enforces string growth and per-code-point instruction limits. The first nine public-pipeline tests failed before implementation. The completed continuation has eight bound-function/instanceof tests and five escape tests. Intl and matcher timeouts remain separate work.

Dedicated review checked exact thrown values, signed-zero length, non-coercing metadata, getter order/prototype snapshots, overridden toString names, native/bound species, nonconstructible targets, direct/custom hasInstance, long argument-prefix order, every lone surrogate and surrogate-pair boundaries. GC tests retain only bound internal edges across executions, then check reclamation and return to the warmed heap baseline; failed oversized bindings are retried. A 1,100-wrapper test isolates traversal fuel from setup and confirms VM recovery. The Node corpus includes error classes but excludes primitive throws, whose exact values are asserted in the default tests.

Public-pipeline tests cover observable conversion/getter order, Symbol identity and protocol lookup on objects and primitives, native metadata, boxed String descriptors, strict/sloppy operations, replacement callbacks/captures, named index identity, UTF-16 and zero-width matching, custom exec/species, templates, iterator closing, recursion, GC pressure and bootstrap retries across allocation ceilings. Heap tests cover descriptor transitions, non-extensibility, accessor tracing and partial array truncation. The dedicated test review found and fixed missing roots, bootstrap leaks, primitive protocol lookup, array join overrides, template/RegExp brace parsing, iterator unwinding and array length descriptor conversion.

The Node oracle uses fresh VMs/realms, hex UTF-8 source transport and exact UTF-16 result transport, with **21,604 isolated scripts** on **Node v24.19.0**: the previous 21,261-script corpus, 63 fixed bind/instanceof/escape regressions, 24 generated metadata/invalid-input/surrogate-pair cases and 256 batches containing **131,072 escape comparisons** (all 65,536 code units in initial and non-initial positions). Edition-17-only getter tests remain separate.

Final validation on 2026-09-09:

- **217 default unit/integration tests and two doc examples pass.**
- `cargo llvm-cov -p blueice-bluejs --fail-under-lines 100 --summary-only -- --quiet`: **6,220/6,220 lines (100%)**, 650/650 functions, 94.65% regions. No engine files are excluded; line coverage is not full branch or specification coverage.
- Workspace all-target build, workspace Clippy (`-D warnings`), full workspace tests, crate rustdoc and the array-summing example (`Number(15.0)`) pass. Socket-based workspace tests ran with local socket permissions after sandbox runs reported `Operation not permitted`.
- The continuation reran the BlueJS gate/oracle, workspace build/tests/lint and crate rustdoc. Full workspace coverage was not rerun.

The complete String method surface and tested protocols are implemented. Complete arbitrary-program String/ECMAScript conformance still requires the supporting language work and edition-pinned Test262 audit described above.

## Intl, hard deadlines and full inventory continuation

The [2026-09-10 continuation report](INTL_CONFORMANCE.md) supersedes the fixed-default locale behavior and absent regex timeout described in earlier slices above. String locale methods now use ICU locale/case/collation data and Intl.Collator/getCanonicalLocales; Intl.Locale provides canonical locale objects, Unicode options, likely-subtag transforms and Locale-info queries; Math constants/functions and regex compilation/matching run in the same bounded VM and terminable subprocess respectively. Declarative and assignment array/object patterns, object spread, global numeric conversion functions and JSON's data-only `parse`/`stringify` paths are also present. Current validation is 243 default tests plus two docs, a passing no-exclusion BlueJS coverage gate and 22,269 Node oracle scripts. The full pinned Test262 inventory has 15,921 passes, 63,054 failures, 22,695 unsupported modes and 908 resource timeouts; remaining Intl constructors and supporting language/global/host semantics prevent a complete conformance claim.
