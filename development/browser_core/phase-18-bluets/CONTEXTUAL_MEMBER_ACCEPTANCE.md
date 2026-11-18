# P0-I — Contextual generic member results

All measured runtime gates pass within the frozen
[contract](CONTEXTUAL_MEMBER_CONTRACT.json) and
[assertion supplement](CONTEXTUAL_MEMBER_REGRESSION_SUPPLEMENT.json) and
[freshness controls](CONTEXTUAL_CALL_FRESHNESS_SUPPLEMENT.json).
Compiler source: `backend-tree:dc685c689478832169cc6e4904fe07d737d97d22`.
Final acceptance requires successful verification of published bytes in
[CONTEXTUAL_MEMBER_ACCEPTANCE_AUDIT.json](CONTEXTUAL_MEMBER_ACCEPTANCE_AUDIT.json).
This document does not check TODO or authorize a partial commit.

Shared contextual inference handles the frozen object, computed, class and
imported generic callable results. Complete constrained-assertion diagnostics
preserve literal/union/alias overlap and fail closed when expansion budgets are
exhausted. JavaScript/declaration maps preserve each source event; ES5/CommonJS
shorthand rewriting consumes the existing anchored prefix once. The original
Native fixtures remain unchanged.

| Requirement | Result |
| --- | --- |
| Contextual contract, each original compiler profile | 960 producers;7,296 consumers;960 syntax checks;128 executions;40 original replays;zero differences |
| Declarations and maps, each profile | 960 exact declarations;1,920 complete maps /32,752 decoded segments |
| Assertion supplement and owner controls | 192 full comparisons;four separate original controls;fail-closed budget0/1;zero extra inventory credit |
| Freshness regression | All102 original observations plus8 Native-frozen zero-credit controls;all6 original G-T4 functions on each profile |
| Complete Native catalog | 192 suites /1,095 functions;zero failed, ignored or filtered |
| Complete ordinary workspace | 8060 passed;0 failed;401 ignored;0 filtered;all737 built test executables ran |
| Both original map profiles | All55 families /298 names each;8,407 configurations ×two builds each |
| Accepted program/project regressions | Both complete14-step profiles;512 primary observations /528 lifecycle states and owner controls;four real projects each |
| Original Native corrections | All572 IDs /452 configurations ×two builds, bound to each actual compiler |
| Windows x86_64 | 82 core oracles;25 paths;72 complete cohorts;source-bound rebuilt PE |
| Tooling | 308 Node tests;zero skips;complete Python/accounting/status;fmt and warnings-denied workspace/all-target Clippy |
| Preserved authorities | 43,112 original case identities/assertions;12,201 fixture paths;618 original tool names |
| New inventory | Exactly164 primary/owner controls;UTF16 siblings and supplements add zero extra denominator credit |
| Module size | 584 production Rust/declaration files;maximum1,333 lines;all below1,500 |

Actual compiler profiles remain separate and hash bound. Earlier failed versions,
test-first red output, verifier corrections and the failed Windows61 source/PE/
raw invalid JavaScript are retained as history, with no complete acceptance credit.
The aborted source62 Windows job remains separate from the successful source63
Windows archive and all complete current-source runtime records
are separate. Full manifests and raw results are indexed in
[CONTEXTUAL_MEMBER_ARTIFACTS.json](CONTEXTUAL_MEMBER_ARTIFACTS.json) and
[CONTEXTUAL_MEMBER_MEASUREMENT.json](CONTEXTUAL_MEMBER_MEASUREMENT.json).

Broader22 feature rows remain open; full TypeScript parity remains OFF. The
preexisting `unknown as T` result-type mismatch remains a G-T5 gap outside this
finite package. Owner/default-deny/opaque-handle and resource boundaries remain
mandatory. Completion requires the successful independent published-byte audit,
then checking P0-I and committing implementation plus matching measurements
before automatic push.
