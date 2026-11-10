# Defu project prerequisites

Forty-nine Native 5.9.3 controls retain the original eight independent project
reproductions and add seven lexical-flow controls for assignment, branch joins,
shadowing, loose null comparisons, AND guards and intervening calls.
Eleven later project-body controls cover trailing call commas, explicit-any
versus unknown assignment results, constrained aliases, conditional/infer bounds
and quantified callback/return contexts. Each extension is recorded from Native
before its corresponding implementation; earlier observations stay unchanged.
Further positive and negative witnesses cover missing call arguments, quantified
wrong results and readonly bounds, exact rest tuple identities, branch-local
inferred tail bounds, indexed-any versus unknown constraints, mapped key-domain
coverage and recursive record compositions under the unchanged expansion limit.

The public CLI regression compares strict project verdicts and every accepted
declaration byte. Rejected controls retain their complete Native raw diagnostics
but do not claim full diagnostic parity. No source, verdict or declaration
expectation comes from BlueTSC.

`record_defu_prerequisites.cjs` checks the immutable Native observations. The
original eight must also match the separately committed real-project controls.
`BLUEICE_WRITE_DEFU_PREREQUISITES=1` records Native changes explicitly, before
compiler implementation; ordinary execution never rewrites reference data.

The full unchanged project, its consumers, runtime and incremental stages are
separate acceptance properties. These forty-nine controls alone do not
complete project compatibility or multi-source state interchange.
