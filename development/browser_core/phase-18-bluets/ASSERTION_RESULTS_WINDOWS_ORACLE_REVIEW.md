# P0-J Windows Native recorder path correction — review only

The source70 Windows run is **failed**. G-T4 finishes four passing functions and
two failures: the Native recorder stores virtual paths with backslashes, while
TypeScript queries normalized forward-slash paths and reports TS6053. Seventy-four
earlier cohorts pass; the 82 core oracles and 25 path cases have not run.
The original source, actual x86_64 PE and raw outcomes are sealed in the failed
archive; SHA256 `e805479a096e1809e267fef5e3b8d75bc1c287584b06660d4e274a81c2801ce7`.

The proposed patch changes only the virtual source lookup in these two tools:

| Original tool | Proposed change |
| --- | --- |
| `tools/record_gt4_overload_chains.cjs` | Replace its raw-key virtual host overrides with the existing `virtual_compiler_host.cjs` adapter. |
| `tools/record_gt8_return_context.cjs` | Apply the same correction to its independently reproduced Windows failure. |

Library caching, authored inputs, options, diagnostics, declaration comparisons,
consumer checks, syntax checks and runtime comparisons retain their assertions.
There is no compiler, oracle fixture or expected verdict change.

Eight local tests exercise the actual recorder functions: both original Windows
lookups fail, and the proposed functions preserve exact accepted declarations,
wrong-type and missing-module rejection, and empty virtual files. On the actual
Windows x86_64 VM, an in-memory draft loader reproduces G-T8's original TS6053
failure, then both complete proposed cohorts pass **6/6**, with zero ignored or
filtered tests. All 14,955 guest source files remain unchanged before and after.
The actual PE SHA256 remains
`2f0b46926ba72225ac0f35d7db5a9659d66f361d8accd96a93704b37edb42854`.
These preview results grant no acceptance credit.

Separate unchanged-source diagnostics subsequently pass all82 core oracles,
25 frozen path cases and4 dynamic-class functions. The complete8,526,129-byte
archive, all135 members and exact compiled selections are independently verified
locally. This preserves the original failed run and its original tool identity;
complete corrected-tool acceptance remains required.

The complete diff, proposed byte hashes, tests and raw observations are under
`target/assertion-results-acceptance-source70-20261119-1/virtual-path-repair-review/`.
The tool-code patch is **applied**. The superseded local run was explicitly
stopped after45 completed map families; its source, state and raw artifacts remain
preserved. All identified old local profile processes terminated before tool
bytes changed. The independent old-tool Windows tail passes82 core oracles,
25 paths and4 dynamic-class functions, with all135 archived members verified.
The existing task authorizes this necessary, reversible recorder repair; the
previously inferred extra approval requirement was withdrawn after reviewing the
scope. Elapsed time or silence is not treated as approval. The original contract
and unknown-return policy amendment retain their original bytes and hashes.
The [mechanical correction definition](ASSERTION_RESULTS_ORACLE_HOST_CORRECTIONS.json)
derives exactly the two host-block replacements from the original tool bytes.
The new assertion verifier checks all621 original tools: only these two
exact replacements can differ; the other619 retain their original digests.
All11 integrated policy tests pass from the actual files (the original3 plus8
new refusal controls). Four new P0-J verifier files record and enforce the
correction digest; no additional original tool is modified. Every frozen fixture,
Native expectation and complete acceptance requirement remains in force.
The actual corrected tools pass complete G-T4/G-T8/assertion cohorts:6/6/18,
zero ignores or filters, plus the full25,488-file /29,520-read assertion audit.
The corrected626-tool snapshot is under
`target/assertion-results-acceptance-source70-tools2-20261119-1/`; its compiler
backend remains `fd642c71683fbd992d1319e5598c51f5379dc766`. The new193-suite
Native run is active and all319 Node tests pass. Complete same-source acceptance
and independent delivery are still required. P0-J remains unchecked and uncommitted.
