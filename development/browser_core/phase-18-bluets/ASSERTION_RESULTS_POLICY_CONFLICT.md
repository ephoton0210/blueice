# P0-J: resolved assertion policy conflict

Status: **resolved by user instruction on 2026-11-19: `接受 unknown as T`.**
The single old pure-return refusal is superseded by the
[approved amendment](ASSERTION_RESULTS_POLICY_AMENDMENT.json). Its original
source, Native diagnostics/declaration and every other projection expectation
remain unchanged. P0-J is unchecked; complete package acceptance, commit and
push remain pending. The following analysis records the decision's basis.

The source68 Native run terminated after 153 suites: the original
`mapped_inference` ownership test failed at
`unknown-cannot-acquire-parameter`. Its source is:

```ts
export function result<T>(value: unknown): T {
    return value as unknown as T;
}
```

The original
`backend/bluets/tests/fixtures/mapped_inference/projection-owner-controls.json`
required BlueTSC to reject this program with TS2322, despite the pinned native
TypeScript compiler accepting it. There is no mutation in this program.

P0-J requires the semantic result of `unknown as T` to be `T`, with runtime
provenance retained separately and owner refusals located at actual mutations.
Its frozen `unknown-object-method-unconstrained` case requires acceptance of
the same generic return conversion inside an object method.

Independent probes of all 13 original projection controls, using both CLI
surfaces and immutable parent/current executables, establish:

| Observation | Accepted parent | Source68 | Native TypeScript 5.9.3 |
| --- | --- | --- | --- |
| Original unknown-to-parameter return | Reject TS2322 | Accept | Accept |
| Same return without intermediate `as unknown` | Reject TS2322 | Accept | Accept |
| Same return in an object method, direct or double cast | Reject TS2322 | Accept | Accept |
| Other 12 original projection controls | Match original policy | Match original policy | Native accepts all |

Preserving the old refusal solely by distinguishing the extra `as unknown`,
or a function declaration from an object method, would introduce an arbitrary
exception. No such production exception was introduced. The user-approved
amendment explicitly resolves the base contract's conflicting requirement to
preserve that old refusal. The base contract remains byte-identical; the
amendment records the exhaustive preservation exception and the original bytes.

Approved resolution: retain every original input and Native observation,
explicitly supersede this one legacy return-refusal expectation, and enforce
readonly/opaque authority at actual mutations. Keep the other 12 controls and
all unrelated authorities unchanged. The amendment records this correction;
mutation checks and complete package acceptance remain mandatory.

The proposed correction affects one supplemental `blueAccepts` field and its
digest in the mapped-inference `FROZEN.json`. The original verifier reads that
digest dynamically; its source need not change. All 43,276 primary inventory
case objects contain no reference to this supplemental control and would retain
their assertions. The original TypeScript input, Native diagnostics/declaration
and historical published evidence remain unchanged. The original proposal is
recorded in `projection-boundary-repair-1/proposed-policy-impact-2.json`; its
exact correction is now applied under the approved amendment. The new P0-J
verifier and independent audit bind the amendment's digest and verify that only
the approved verdict and dependent digest changed.
The current13 projection controls therefore contain four accepted programs and
nine intentional refusals; older fixture scope prose retains its historical
wording. Active verdicts come from the explicitly amended case expectations.

Additional real mutation probes expose an independent implementation defect:
direct and double casts through a generic result reject readonly record writes,
but source68 accepts readonly array writes. `deep_contains_readonly` traverses
array elements but does not recognize a root `Type::Readonly`. Repair and
regression coverage for this defect are now complete as a bounded development
change: 429 Rust test functions pass, one complete Native contract test remains
ignored, and format/compiler all-target Clippy checks pass. Both actual CLI
surfaces reject all four direct/double-cast array/record mutation forms at their
authored write, while the unrelated opaque and declared-any positive controls
pass in checked and legacy library modes. The other 12 original projection
controls retain their original verdicts. The return-policy conflict is resolved
by the amendment; see [diagnostic measurements](ASSERTION_RESULTS_POLICY_DIAGNOSIS.json).
This is not complete package acceptance.

The immutable repaired compiler also passes the entire frozen assertion
contract: 1,296 producer observations, 6,144 consumers and 768 executions.
Independent auditing verifies 25,488 distinct files, 1,008 exact declarations,
2,016 complete maps with 31,116 segments and 1,008 syntax checks. All 14,239
backend source files remain bound throughout this run. The report is
`projection-boundary-repair-1/complete-contract/report.json`, SHA256
`2864ad84fff3a6f311453464dcfff4c78eacf83e2d353321afb5da28c0ea5860`;
the independent audit is SHA256
`dfbdf42797421a0a175964ec9f26e3607edf04c14c11eca019745bc0d2b97a68`.
This is separate development proof, with no fresh Native-catalog/workspace/
standalone or Windows acceptance credit. This was the pre-amendment development
run; post-amendment verification is recorded separately under
`target/assertion-result-contract-20261118-1/policy-acceptance-20261119-1/`.

Raw observations are in
`target/assertion-result-contract-20261118-1/projection-boundary-repair-1/`:
`source68/report.json`, `accepted-parent/report.json`, and
`mutation-controls/report.json`. These are diagnostic records, not complete
package acceptance. Pre-repair source68 CLI SHA256:
`283dfec84d0888a5378ca106d60d60666cc45bc94b6a620ed4a9135fba191169`;
parent CLI SHA256:
`e0c681a84d830e8e04bf1215d07c97aee8af8ce8421283981c40dbafda283cfa`.
The immutable repaired CLI used for the complete contract and independent audit
is SHA256 `b2d09d2f09292be4f7640fdfdf9c39f2909f38dd4490cbb144712c34dad6d830`.

Post-amendment acceptance passes all7 mapped-inference functions unfiltered,
the entire1,296-producer /6,144-consumer /768-execution assertion contract and
its independent25,488-file audit. The48 Node test files pass all311 tests after
92 fresh safe-home observations with two builds each. Preservation independently
confirms all43,276 original case objects byte-identical, all12,224 old fixture
paths present (12,220 identical, two append-only adapters and the two approved
policy files), and all621 original tools identical. The complete measurements
and digests are in [diagnostic measurements](ASSERTION_RESULTS_POLICY_DIAGNOSIS.json).
The40 original Native suites never started by source68 are now a development
preflight before the next complete formal snapshot. Fresh complete Native/
workspace/standalone/Windows/maps/projects and durable delivery remain required.
