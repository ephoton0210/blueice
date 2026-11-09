# Explicit incremental configuration

Eighteen configurations freeze TypeScript 5.9.3 diagnostics, public state paths
and exact declarations: default/custom paths, rootDir/outDir, source maps,
declaration disabled, composite, true/false/null, an inert custom path without
incremental, invalid values, composite disabling incremental and a source-map
marker string that excludes Native state publication, and a transitive import
that widens the default source root, and inherited incremental configuration. Its complete declaration inventory and
imported-source warm/edit/clean behavior are compared.

Both `--build` and direct TypeScript `build --config` use the same project
build/state implementation. Owner `bluetsc.json` configurations retain their
existing policy path. Ordinary direct builds retain their manifest publisher;
only effective build-only options and project references select the stateful runner. The build entry implements current-input warm reuse, edits, clean reconstruction,
missing/corrupt/foreign state recovery, and output hash verification. Paths are
preflighted before emission; state cannot overwrite inputs, private state or
source artifacts, or escape the owner through paths or symlinks. The twelve owner
control families are classified separately from Native agreements. Switching
from ordinary publication to explicit incremental retires its recognized legacy
manifest before emission; user data remains, and inputs or symlink metadata
cannot be retired. Cache reuse also revalidates unexpected reserved metadata;
unrecognized user data is retained and can require a rebuild.

The finite Native interchange profile includes the frozen single-source strict
ES2022 CommonJS/ESNext TypeScript projects with declarations and the ES2022 library, now also allowing
explicit incremental and source maps. Source-only exported metadata never
discovers or authorizes a Native installation. Native `-p` reconstructs its own
library graph and re-emits; this is an explicit owner difference, not a Native
warm no-op claim. Native state containing transitive imported sources triggers
an owner-current rebuild; its declaration and runtime results still match.
Native-to-BlueTSC warm reuse in the single-source profile, reverse consumption, exact
declarations, execution and missing-map recovery are tested. Unverifiable
source-map markers and other explicit
incremental profiles use a versioned BlueTSC state at the configured path and
rebuild on foreign state rather than claiming Native interchange.

`incremental: false`/null emits no public incremental state. Existing build-mode
private owner state remains independent. The accepted `tsBuildInfoFile` alone
does not enable public incremental state.

Run `cargo test -p blueice-bluets --test gm2_incremental_options
-- --include-ignored` with pinned `BLUEICE_BLUETSC_ORACLE`.
The independent recorder is
`development/browser_core/phase-18-bluets/tools/record_gm2_incremental_options.cjs`;
`BLUEICE_WRITE_GM2_INCREMENTAL_OPTIONS=1` freezes new Native observations only.
The original 296 G-M2 workflows and their existing owner differences remain
unchanged. Full unchanged mitt incremental acceptance is measured separately.
