# P0-A — NoInfer acceptance

Accepted 2026-11-12, source `backend-tree:1b539964f6587dc42b42a87f53001f1ee6d5751f`, against TypeScript 5.9.3.
The [measurement](NOINFER_MEASUREMENT.json), [artifact manifest](NOINFER_ARTIFACTS.json)
and [frozen design](NOINFER_FOLLOWUP.md) bind this finite package.
The [publication checks](NOINFER_PUBLICATION_CHECKS.json) record final document,
case-accounting and archive-integrity verification.
Full TypeScript parity remains **OFF**; all 22 broader open feature rows remain open.

The type model carries an inference barrier through ordinary substitution and
aliases. Candidate and dependency collection respect that operation; checking
still validates the instantiated operand. Conditional infer, contextual result
evidence, rest packs and callback/overload contexts share these boundaries.
Trusted embedded intrinsic lowering supplies NoInfer; local, imported and owner
declarations retain precedence. The type-only declaration adds no runtime binding.
Native declaration identities remain exact, including retained object/array
wrappers and reduced scalar forms. Diagnostic rendering preserves ordinary named
owners and complete import-type spans.

| Frozen acceptance | Actual result |
| --- | --- |
| Primary programs | 94 forms × 3 strict target/module/library profiles = 282 observations; 153 accepts, 129 rejects |
| Supplementary diagnostics | Four independent named-key/operator cases; complete primary/related diagnostics and UTF-16 positions |
| Accepted declarations, per actual compiler | 153 byte-exact inventories / 159 files |
| Isolated declaration consumers | 306 positive/negative consumers per compiler |
| Syntax and execution | 159 selected-edition parses and six executions per compiler |
| Separate boundaries | Three owner/local/runtime controls and nine work/depth refusals; excluded from direct agreement |
| Complete Native catalog | 185 suites / 1,053 functions; zero failed, ignored or filtered |
| Complete local workspace | 7,847 passed, zero failed; explicit Native-only cases are separately selected above |
| Complete maps | Fresh process-bound 8,407 configurations × two builds for both actual local compiler profiles; zero differences |
| Windows x86_64 | Retained same-source 82/82 core oracles, 25/25 path cases and all seven complete regression cohorts |
| Real projects and tooling | Full mitt/defu, eight prerequisite reproductions and interchange; formatting, warnings-denied workspace Clippy, Node/Python and accounting gates pass |
| Module size | 538 source files audited; maximum 1,477 lines, none over 1,500 |

The unchanged previous accepted compiler has six complete agreements and twelve
accepted builds across the same 282 primary observations. Its executable was
recovered from the hash-verified receiver acceptance archive; all 282 raw baseline
observations were reproduced. Baseline and corrected 22-name/two-profile presence
witnesses are separate from the direct denominator. Five selected utility names
remain missing in ES2022; ES5 additionally needs Awaited and its supporting types.

The original local temporary evidence directory became unavailable after the
interruption. This acceptance uses a fresh complete local run at the unchanged
source and unchanged 588 oracle tools, and the byte-identical Windows archive
recovered from the VM. Earlier failed Windows candidates and recovered historical
tool records are retained separately, with no pass credit for this source.
The missing earlier local raw logs are not represented as retained evidence.

All 39,927 previous suite-qualified case IDs and comparison/status/scope/reason
classifications are preserved. The 282 primary and four supplementary observations
add 286 assertions, making **N = 40,213**. These are case assertions, not unique
programs or a TypeScript language-completeness percentage. P0-B and P0-C remain
unchecked; broader category closure needs its own explicit contract.

The stored gate commands select all ignored Native tests, then run the complete
local workspace, explicit declaration-consumer oracles, formatting, Clippy and
tool checks. Reproduction requires pinned TypeScript 5.9.3 and Acorn 8.15.0;
compiler, source, input, library, command and raw-report hashes are in the
measurement and archives. Windows evidence is the original same-source run,
independently reverified after recovery; no new hosted-CI or OS gate is added.
