# Fixed real-project audit — mitt 3.0.1

The user-authorized follow-up uses the complete, unchanged upstream source at
[`b240473b5707857ba2c6a8e6d707c28d1e39da49`](https://github.com/developit/mitt/tree/b240473b5707857ba2c6a8e6d707c28d1e39da49).
Source, license, configuration and reproduction hashes are frozen in the
[manifest](../../../differential-testing/bluets-projects/mitt-3.0.1.json).
The reusable [audit](../../../differential-testing/bluets-projects/README.md)
keeps project compatibility separate from completion of the investigation.
The exact [measurement](REAL_PROJECT_MEASUREMENT.json) and
[raw evidence index](REAL_PROJECT_ARTIFACTS.json) retain the commands, complete
diagnostics, original/edited outputs, consumers, tools and all audit attempts.

## Recorded outcome

The source-bound audit on `backend-tree:ad7f2dc43c2b7f2eefcaadf97312ee7d8da68fb3`
reports **blocked**, with `compatible: false`. The actual compiler SHA-256 is
`c3cdbb7e1979e98176d2b947e141fe55b87d3b233cb821a7075d696c15afff60`;
the complete Native regression build supplies its compiler/source binding.

| Property | TypeScript 5.9.3 | BlueTSC |
| --- | --- | --- |
| Complete original source, CommonJS/ES2022/strict | Accepted | Rejected |
| Original-source declaration | Published; exact bytes retained | Not published |
| Eight named event-runtime scenarios | 8/8 pass | Blocked by compilation |
| Declaration consumers | Positive accepts; wrong payload rejects with TS2345 | Blocked by compilation |
| Warm, edited and clean rebuilds | Pass; edited artifacts match clean output | Blocked by compilation |
| Minimal ordinary object method | Accepted | Accepted |
| Minimal generic class method | Accepted | Accepted |
| Minimal generic object method, unconstrained | Accepted | Explicitly refused |
| Minimal generic object method, `extends string` | Accepted | Incorrect parse diagnostics |
| Scalar project, ordinary configuration | Accepted | Accepted |
| Same scalar project, `incremental: true` | Accepted | Unsupported option rejection |

Project-level compiler/MCP publication and debugger execution remain blocked by
the failed compilation. The existing service/debugger regression gates retain
their separate scope.

The initial audit exposed the unsupported incremental option before reaching
the project parser. The final audit separates the common cold profile from the
explicit incremental profile and confirms both blockers independently. The
original attempt remains historical evidence. No source is trimmed, rewritten
or compiled through Native as a BlueTSC fallback.

These are **two independently reproduced blockers**. The constrained method's
diagnostic cascade is a symptom of the first blocker. It is not counted as
another missing feature. The global case denominator and full-parity-OFF policy
are unchanged.

## Next implementation within existing G-IDs

| Existing area | Reproduction and required behavior | Minimum repair gate |
| --- | --- | --- |
| **G-T2**, with **G-C1** diagnostic coverage | Preserve generic parameters and constraints on object-literal methods through parsing, checking and emission. Unconstrained forms currently receive a nested-expression annotation refusal; constrained forms receive spurious `?`, type and `:` errors. | Both frozen generic object programs and the original mitt source pass the pinned accepted boundary. Preserve the passing class and ordinary-method controls. Add wrong key/payload and malformed-syntax controls with matching complete diagnostics and UTF-16 spans. Compare complete `.d.ts` inventories and bytes, validate consumers and execute all eight project scenarios. Any subsequent blocker remains visible. |
| **G-M2**, with **G-C1** CLI coverage | Handle explicit `incremental` configuration consistently with the existing project build/state model. The minimal scalar project already builds without that option; enabling it alone currently rejects the configuration. | The frozen scalar control and project accept the valid option. Validate cold/warm/edit/clean behavior and state-path handling, including corrupt/unverifiable state recovery and invalid-option controls. Preserve owner confinement and existing intentional rebuild-policy differences. |

Each repair requires focused regressions plus the existing complete Native,
local-workspace and Windows 82-oracle gates before completion and commit.
The audit and triage are complete; these implementation gaps remain open.
