# P0-A — NoInfer and shared candidate boundaries

Status: complete within the frozen scope (2026-11-12). See [acceptance](NOINFER_ACCEPTANCE.md) and [measurement](NOINFER_MEASUREMENT.json).

The [priority checklist](TODO.md#next-implementation-packages--p0-first) supplies
the scope: candidate collection, contextual results, nested/aliased positions,
constraints/defaults, callbacks, overloads and owner/local shadowing. This package
targets G-T2/G-T4/G-T6/G-T7/G-T8; broader feature rows retain their existing scope.

The authored input contains **94 forms under three strict profiles**:
ES5/CommonJS/ES5 library, ES2022/CommonJS/ES2022 library and
ES2022/ESNext/ES5 library. TypeScript 5.9.3 independently records **282
observations: 153 accepts and 129 rejects**, **153 exact declaration inventories /
159 files**, **306 positive/negative consumers of only emitted declarations**
and **six executions**. Imported aliases and imported same-spelled owner types
use multiple real source files. Conditional infer, keyof, blocked rest packs,
ordinary unblocked evidence, CRLF and non-BMP coordinates remain explicit cases.
Every rejected noEmitOnError project must publish no artifacts. Verify all 159
accepted emitted JavaScript files against their selected Acorn syntax edition.

Frozen before production changes:

- Input SHA-256: `f93542b0ad33c9ca5fbc3122e732548427bff77108eec7cc3f74046e0f8734e6`.
- Native reference SHA-256: `5af732d9e989b8dbaa687615ac4837e51f7994220b3bd43637ca260a20d18c65`.
- Previous accepted compiler SHA-256: `a2e85a3c66972996537b979b762e38d02787c70c15681b730a81994ec514aa2a`,
  source `backend-tree:4687d8d818e1d6b08bb437b41b19c9d84813356f`.

The previous accepted executable has **6/282 complete agreements** and accepts
12 builds. Its raw diagnostics, source/configuration, declaration bytes and
executable remain retained. The local/imported owner-shadow controls expose
an existing nested generic literal-widening difference; repair the common
contextual-inference path while keeping those owner types ordinary aliases.

Design requirements established by independent Native probes:

1. Represent an inference barrier in the type model. Lower the original
   embedded intrinsic declaration within trusted library construction; ordinary
   source/owner declarations keep precedence. Candidate collection must inspect
   that type operation rather than special-case the spelling of a called alias.
2. Preserve the barrier through substitution and aliases. Assignment, properties
   and ordinary type relations still validate its underlying type. Blocked
   argument evidence does not block an independent ordinary result context:
   `choose<T>(value: NoInfer<T>): T` can infer T from an explicit result type;
   `choose<T>(): NoInfer<T>` cannot obtain that evidence from its blocked result.
3. Conditional NoInfer patterns also suppress infer candidates. An unbound
   inferred parameter uses Native's observed unknown result, while unblocked
   positions remain able to supply evidence. This is an explicit inference
   result, with no general unknown/any escape for failed checking.
4. Declaration materialization is part of acceptance. Native preserves
   `NoInfer<{...}>` and `NoInfer<number[]>` for inferred results, while the frozen
   scalar, empty-object and primitive-union results omit the wrapper. Exact
   public names and downstream consumers must survive that distinction.
5. Retain readonly/owner identity, configured budgets, default-denied runtime
   authority and opaque handles. Type-only NoInfer acquires no runtime binding.

Authoring calibration and the original 21 semantic probes are historical design
context outside the frozen comparison denominator. They identified literal
preservation after candidate blocking and ordinary scalar/rest result-context
inference. The original temporary evidence directory later became unavailable;
recovered tool records are retained separately. Fresh complete local acceptance
and the hash-identical recovered Windows archive support the current result.
The earlier missing local raw logs receive no retained-evidence claim. Frozen
input and Native-reference bytes remain unchanged.

Four additional ES2022/CommonJS diagnostic regressions independently record
ordinary `keyof Events`, named NoInfer operands, a blocked key type and an
operator alias. They exposed an over-eager diagnostic expansion in the first
implementation. Their Native codes/messages/UTF-16 offsets were recorded before
the diagnostic repair; subsequent recording added file and related-information
fields while asserting those original expectations unchanged. These four new
assertions are accounted separately from the original 282-observation benchmark.
They add no accepted declaration or execution cases. The original input,
reference and baseline denominator remain unchanged.

The accepted package passes all frozen diagnostics and UTF-16 primary/related
positions, exact declarations, consumers, syntax, execution and separate
owner/budget controls. Acceptance also includes the complete same-source Native
catalog, local workspace, both actual compiler profiles' complete map reports,
current project regressions, formatting/Clippy/tooling and the Windows 82-core
boundary with its path/regression cohorts. The [acceptance](NOINFER_ACCEPTANCE.md)
and [measurement](NOINFER_MEASUREMENT.json) retain the source and raw evidence
bindings. Keep implementation and corresponding measurements together when
committing. P0-B/P0-C and later TODO priorities remain open.
