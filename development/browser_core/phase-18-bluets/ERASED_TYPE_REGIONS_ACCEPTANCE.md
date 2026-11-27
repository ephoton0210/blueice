# Parser-owned erased type regions acceptance

P0-K uses parser-owned type boundaries when traversing runtime call arguments. Commas inside generic, assertion and satisfies types preserve runtime argument ownership. The accepted unknown-as-T pure return and readonly/opaque mutation policies remain covered.

Source: `backend-tree:f37bddc3ad3d842b3dcf261e7c6e6fedcca1290e`. All frozen runtime and independently published-byte requirements pass. The [final source/evidence review](ERASED_TYPE_REGIONS_FINAL_REVIEW.json) accepts the finite package; its implementation and matching measurements are delivered together. Broader 22 feature rows remain open; full TypeScript parity remains OFF.

| Actual compiler profile | SHA-256 | CLI surfaces |
| --- | --- | --- |
| native | `f1f4a15464039cd58981c31a5c51224b4936be2bffff33cfd430afcdfec245e9` | legacy / native |
| workspace | `559e2ce7183c7b180a9c70ed501f04189c7df087353193823b38af7f7cd56f7f` | legacy / native |
| standalone | `470922d6d9d32d9f36bfe3364f5b6451a8019e99baf357f973956a93a56a029f` | legacy / native |
| Windows x86_64 | `7aaf514b9034a812ad0681b878d85ee17aae15f091bb1fab5e7ac06a63b97170` | legacy / native |

Each surface passes all 124 primary observations (96 accepts / 28 rejects), exact diagnostic arrays and declaration bytes, 192 isolated consumers, eight executions, 208 complete decoded maps and 104 JavaScript syntax checks. All 12 separate discovery controls retain their original diagnostics and receive no repair credit. New exact JavaScript layout and map differences are enumerated as open; this package does not claim arbitrary exact map parity.

- Complete Native catalog: 194 suites / 1,118 functions; no failures, ignores or filters.
- Complete workspace: 8,086 passed, 0 failed, 403 existing or Native-only ignores and 0 filters; all 739 built test executables ran.
- Both original map profiles: 55 families / 298 functions, 8,407 configurations twice and all 572 original Native corrections. Both program profiles pass all 14 steps and four projects.
- Format, workspace all-target warnings-denied Clippy, 322 Node tests and Python/accounting/status pass.
- Windows x86_64: 78 complete cohorts, 82 core oracles and 25 path cases; full source, actual PE and raw evidence independently verified.
- All 43,492 original case objects remain; 136 new observations yield 43,628. Original frozen expectations remain unchanged. The three bounded checkout/tooling changes are recorded separately.

[Frozen contract](ERASED_TYPE_REGIONS_CONTRACT.json) · [Measurements](ERASED_TYPE_REGIONS_MEASUREMENT.json) · [Independent acceptance audit](ERASED_TYPE_REGIONS_ACCEPTANCE_AUDIT.json) · [Complete source and raw evidence](ERASED_TYPE_REGIONS_ARTIFACTS.json) · [Open output differences](ERASED_TYPE_REGIONS_OPEN_LAYOUT_OBSERVATIONS.json)

Failed source revisions and orchestration attempts remain archived without acceptance credit. Complete file and fragment ledgers reconstruct every published file digest from bounded archives.

The tool host lost its controller sessions while the original workspace Cargo process continued. A separately verified macOS kernel exit-status notification captured that original process exit; only the remaining orchestration resumed. No compiler, fixture or regression requirement changed, and the original workspace test was not restarted.

The measurement and artifact-audit JSON files retain their pre-review snapshot fields (`TODOUncheckedAtAudit: true`, `wholeP0KComplete: false`). The dated final-review record establishes the subsequent finite acceptance without rewriting those historical proofs. Git history records delivery.
