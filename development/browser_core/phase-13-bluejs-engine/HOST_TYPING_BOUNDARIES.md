# Checked host-member and value-query boundaries

[← Browser integration TODO](BROWSER_INTEGRATION_TODO.md)

M1n completed on 2026-10-15. Its public failing baseline and final acceptance
are recorded in the [milestone record](BROWSER_INTEGRATION_MILESTONES.md#milestone-record).

## Declaration ownership

Ambient function values retain their complete callable signature, including
overloads and generic parameter/result relationships. A value query used by a
host interface belongs to the declaration module and original reference offset.
Its internal alias cannot resolve against a consumer's local value of the same
name or collide with a query at the same byte offset in another module.

Declaration modules see the shared global ambient surface after their own
bindings. Queries are resolved after module value exports have been registered,
using the ordinary binder and lexical scope model. Function values supplied to
that model preserve callable types; local values and imports retain precedence.
An unresolved query prevents checked output. Query work uses the existing
type-expansion limit.

The existing host declaration graph remains closed. A value query grants no
module-loading or runtime import authority. Referenced declaration modules use
the existing owner-authorized edges and supported type-only imports.

## Member chains

The checker reuses its postfix receiver ranges to validate property reads and
intermediate receivers, including those nested in arguments. A missing member
is diagnosed before a later lookup can turn its receiver into `unknown`.
Indexed calls perform the same known-property validation as ordinary reads.
Call arguments, writes, readonly enforcement and access restrictions retain
their existing checks.

Generic type arguments remain erased type syntax. Prefix updates retain their
actual object receiver, quoted object keys use their property spelling, and an
unreachable flow narrowing does not manufacture a missing property on `never`.
Declared `never` and existing unknown/unchecked policies remain distinct from
that flow-only case.

This repairs checked host-member lookup; the remaining general TypeScript
compatibility items stay in Phase 18's own plan.

## Publication and identity

Checked modules retain the resolved declaration-owned query types needed by
inferred declaration output. The declaration writer expands those internal
aliases instead of publishing their private names. A recursive callable can
refer back to an accessible, unshadowed public query; an unnameable private
cycle is refused through the existing declaration diagnostic boundary.

Compiler fingerprints include `value-boundaries-v1`, invalidating artifacts
checked under the previous policy. This changes compiler acceptance without
changing the source bytes or runtime inventory of File DOM v13/v14. It installs
no browser global and does not change private IPC.

Acceptance includes public compiler verdicts, inferred declaration parsing,
module/query identity collisions, lexical shadows, overloads/generics, resource
refusal and the public page-profile compiler. All 164 compiler/bridge targets,
six complete host targets and the focused official-MCP-client acceptance pass
(2,767 Rust cases; 225 existing ignored). A real Core/child fixture rejects
absent member chains before earlier DOM writes can run. Workspace/all-target
build and Clippy, formatting and the complete pinned Test262 comparison pass;
all 102,926 modes match M1m's accepted source/result inventory.
