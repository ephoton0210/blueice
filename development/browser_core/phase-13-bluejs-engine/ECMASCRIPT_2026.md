# ECMAScript 2026 implementation track

Status: **in progress, not conformant**. Requested 2026-09-09. This extends the language target beyond Phase 2's historical MVP cuts; those cuts are no longer the completion criterion for BlueJS. Keep the from-scratch Rust engine, fixed-width stack bytecode, abstracted object storage and generational GC design. Do not replace the engine with Node/V8 or an AST interpreter.

## Authority and completion criteria

Target the [published ECMA-262 edition 17 HTML](https://262.ecma-international.org/17.0/), not features added only to the living 2027 draft. The [baseline record](../research/js-conformance-baseline.md) explains their difference. Read each affected clause before implementation and record the link/date. DOM, HTML event-loop integration and ECMA-402 internationalization are separate host/specification projects; they must not be counted as ECMAScript language completion. JIT optimization is not required for conformance.

Full completion requires an audited clause/feature inventory, a pinned Test262 revision appropriate to the edition, explicit strict/non-strict/module/async harness modes, negative-phase/error-type checking, and no silently skipped mandatory tests. Record unsupported cases separately from passed cases; Node differential results are supplemental. Feature tests must drive public APIs, BlueJS's 100% line gate remains, and every slice gets a dedicated test-content review. Passing coverage or a selected Test262 subset never means the edition is complete. Normative optional, legacy and host-defined requirements need explicit applicability decisions, not blanket exclusions.

## Dependency-ordered work inventory

| Workstream | Current state / remaining work |
| --- | --- |
| Source grammar (§§11–16) | Existing subset parser; close numeric/whitespace/short-circuit gaps first. Unicode identifiers and escapes, all operators, labels, strict-mode early errors, regex lexical goals, classes/private names, generators, async and modules remain. |
| Values/conversions (§§6–7) | Basic primitives, Number conversions and lossless UTF-16 strings/property names exist. String boxing is implemented; BigInt, Symbol keys, remaining boxing, callable ToPrimitive and other abstract operations remain. |
| Environments/execution (§§8–10, 14–16) | Slot-based scripts/blocks/loops with TDZ and native calls/receivers exist. Persistent globals, user call frames/closures/`this`, arguments, eval, general constructors, abrupt completions, per-iteration captured environments and module records remain. |
| Object semantics (§§6, 10) | Ordinary data properties, prototypes, sparse arrays, native callable objects, boxed String virtual properties and GC exist. General descriptors/accessors/extensibility, user/bound/proxy functions, complete exotic algorithms and remaining internal methods remain. |
| Fundamental and numeric builtins (§§18–21) | Ambient undefined/NaN/Infinity only. Global functions, Object/Function/Boolean/Symbol/Error, Number/BigInt/Math/Date and all prototypes remain. |
| Text/indexed/collections (§§22–24) | UTF-16 literals and many String builtins now run; see the [complete String work inventory](STRING_BUILTINS.md). RegExp/Symbol/locale-dependent String methods, Array methods, typed arrays, keyed/weak collections and iterator helpers remain. |
| Structured/control/reflection (§§25–28) | ArrayBuffer/DataView/Atomics/JSON, promises/jobs/generators/async, resource-management objects, WeakRef/finalization, Reflect/Proxy and module namespaces remain. |
| Memory model and annexes (§29, A–F) | Shared-memory semantics, host-agent applicability, normative-optional/legacy browser requirements and edition-specific amendments need implementation/audit. |
| Conformance infrastructure | Fixed public regressions and opt-in Node oracle exist. Full CLI/host harness, edition-pinned Test262 inventory and per-feature reporting remain. |

This is a workstream inventory, not yet a complete clause-by-clause audit. UTF-16 storage and native call dispatch are now available; the complete String request still needs user callables/coercion, Symbol/RegExp/iteration, locale policy and descriptor work.

## Second slice: lossless UTF-16 storage

Official edition 17 clauses fetched and read on 2026-09-09: [§6.1.4 String type](https://262.ecma-international.org/17.0/#sec-ecmascript-language-types-string-type), [§11.1.1 UTF16EncodeCodePoint](https://262.ecma-international.org/17.0/#sec-utf16encodecodepoint), [§12.9.4 string literals](https://262.ecma-international.org/17.0/#sec-literals-string-literals), and [§12.9.6.1 template values](https://262.ecma-international.org/17.0/#sec-static-semantics-tv). Strings are sequences of 16-bit values, without implicit normalization or replacement of lone surrogates. Both four-digit escapes and braced code-point escapes can represent surrogate code units.

Design before implementation: introduce a public `JsString` owning UTF-16 code units, with explicit lossless host UTF-8 conversion. Use it for literal/cooked AST and token values, runtime strings, and heap property keys. Source text, raw template placeholder source and identifier names remain Rust UTF-8 strings. Equality, ordering, concatenation and key identity must never go through lossy conversion. Keep ergonomic UTF-8 host inputs via `Into<JsString>` on heap property APIs; enumeration returns `JsString` keys. `VmConfig::max_string_bytes` and managed-heap string/key payload accounting now count two bytes per code unit, not UTF-8 bytes; both stored key copies remain charged. The Node oracle's **result** strings must use exact four-hex-digit code units, while source transport stays hex UTF-8.

Test first through parse → compile → execute and public heap APIs; include lone/pair/reversed surrogates, astral/BMP boundaries, embedded NUL, distinct surrogate keys, numeric coercion, GC edges and atomic budget failures. The user expanded this slice to the [complete String builtin request](STRING_BUILTINS.md): indexed reads, String boxing and native methods are now underway, not deferred wholesale. [§10.4.3.4 StringCreate](https://262.ecma-international.org/17.0/#sec-stringcreate) and [§10.4.3.5 StringGetOwnProperty](https://262.ecma-international.org/17.0/#sec-stringgetownproperty) were read before those implementations. The four initial storage tests all failed before implementation; later method groups likewise failed before their respective dispatch/algorithms were added.

## First corrective slice: lexical declarations and source grammar

Official edition 17 clauses fetched and read on 2026-09-09:

- [§9.1.1.1.5 SetMutableBinding](https://262.ecma-international.org/17.0/#sec-declarative-environment-records-setmutablebinding-n-v-s), [§9.1.1.1.6 GetBindingValue](https://262.ecma-international.org/17.0/#sec-declarative-environment-records-getbindingvalue-n-s), and [§14.3.1 lexical declarations](https://262.ecma-international.org/17.0/#sec-let-and-const-declarations): keep uninitialized bindings distinct from the JavaScript value undefined. Reads, `typeof`, writes and updates during TDZ must raise ReferenceError; check this before const assignment TypeError. `var` starts initialized; `let x;` initializes at its declaration. Preserve rooting of initialized object values and cleanup/re-entry on loops/errors.
- [§13.13 short-circuit grammar](https://262.ecma-international.org/17.0/#sec-binary-logical-operators): reject unparenthesized `??` with `&&`/`||`, including dead branches. Track operators consumed by the current grammar level rather than inspecting an AST that has already discarded parentheses.
- [§12.9.3 Number literals](https://262.ecma-international.org/17.0/#sec-literals-numeric-literals): add binary/octal/hex prefixes and numeric separators with digit-boundary validation, reject incomplete exponents and forbidden trailing identifiers/digits, use single-rounding radix conversion. Preserve sloppy legacy leading-zero distinctions; strict-mode rejection and BigInt literals remain with their respective workstreams.
- [§12.2 whitespace](https://262.ecma-international.org/17.0/#sec-white-space), [§12.3 line terminators](https://262.ecma-international.org/17.0/#sec-line-terminators), [§12.9.4 strings](https://262.ecma-international.org/17.0/#sec-literals-string-literals) and [§12.9.6 templates](https://262.ecma-international.org/17.0/#sec-template-literal-lexical-components): distinguish ECMAScript whitespace from Rust Unicode whitespace; LF/CR/LS/PS affect ASI and line comments. Handle CRLF as one continuation, reject raw LF/CR in quoted strings, preserve raw LS/PS in strings and normalize raw template CR/CRLF to LF.

Implementation is test-first. Add public regression fixtures before the changes, update old tests that intentionally asserted the now-superseded lenient behavior, extend the Node corpus, then run the full BlueJS gate and workspace regressions. Do not mark any broader workstream complete from this slice alone.

### First-slice results (2026-09-09)

- [x] TDZ for the implemented script/block/classic-loop bindings, including `typeof`, updates, initializer order and const error precedence. Private VM slots now distinguish uninitialized from initialized undefined; initialized object values retain the existing GC rooting behavior.
- [x] Reject unparenthesized nullish/logical mixtures, while preserving parenthesized, conditional, array and template expression boundaries and short-circuit behavior.
- [x] Number radix prefixes and separators; single-rounding conversion; incomplete exponent/trailing-identifier rejection; sloppy leading-zero octal/decimal distinction. This does not implement BigInt or strict-mode source processing.
- [x] ECMAScript whitespace/line terminators, ASI/comment boundaries, quoted-string continuation and template CR/CRLF normalization. Review additionally fixed comment contents being mistaken for closing braces/quotes/backticks inside template placeholders.
- [x] Default tests and 100% BlueJS line gate; opt-in Node oracle; workspace regression checks.
- [ ] Full source grammar and strict/module processing.
- [ ] Full runtime environments, user callables and the other inventory workstreams above; UTF-16 storage/native calls are now implemented by the second slice.
- [ ] Edition-pinned full Test262 harness and conformance audit.

**Evidence**: the initial nine integration tests had eight failing cases before implementation; the later template-comment regression was also observed failing before its fix. Dedicated review added TDZ RHS error ordering, loop re-entry/root cleanup, token-boundary and raw/cooked template assertions, plus multiline independent-oracle inputs. The previous test asserting undefined before a lexical declaration and the lexer test accepting an incomplete exponent were corrected to edition 17 behavior.

**Validation**: 155 default BlueJS tests plus two doc examples pass. `cargo llvm-cov -p blueice-bluejs --fail-under-lines 100 --summary-only -- --quiet` reports **2,994/2,994 lines (100.00%)**, 325/325 functions and 95.85% region coverage, with no engine-file exclusions. Node.js v24.18.0 passes **10,844 isolated scripts**; 151 were added in this slice, including 60 generated multiline/Unicode-whitespace scripts. Oracle input now uses one hex-encoded UTF-8 source per line, so physical line terminators cannot split a fixture. Workspace build/all-target Clippy (`-D warnings`), workspace tests, crate rustdoc and the array example pass. The final lexer-only test additions were checked by the subsequent BlueJS gate; workspace real-socket tests required execution outside the socket-restricted sandbox. Full workspace coverage was not rerun. **The complete edition remains unimplemented.**
