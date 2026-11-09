# Object generic methods and their real-project dependencies

`cases.json` freezes 53 finite public compilation forms. `reference.json` comes
only from TypeScript 5.9.3, before the corresponding implementation. The matrix
retains accepted and rejected syntax, binder identity/constraints, calls,
indexed results, local aliases, mapped key domains and optional collection flow.
It compares complete diagnostics (including UTF-16 spans and related locations),
declaration bytes, Native declaration consumers, Acorn syntax and runtime output.

The three `ownerControls` are separate readonly/mutable controls, each exercised
in strict and legacy modes. Native acceptance of a cast does not authorize
discarding BlueTS's existing runtime readonly data qualifiers.

Run `cargo test -p blueice-bluets --test gc1_object_generic_methods
-- --include-ignored` with the pinned `BLUEICE_BLUETSC_ORACLE` and
`BLUEICE_ACORN_ORACLE`. The recorder is
`development/browser_core/phase-18-bluets/tools/record_gc1_object_generic_methods.cjs`.
Set `BLUEICE_WRITE_GC1_OBJECT_GENERIC_METHODS=1` only to freeze new Native inputs;
ordinary validation must not rewrite the reference.

This matrix does not establish general TypeScript parity. The unchanged complete
mitt 3.0.1 project, declaration consumers and eight runtime scenarios are measured
separately by `differential-testing/bluets-projects/audit.cjs`.
