# Symbolic key relations — P0-E

The 64 authored forms run in four strict ES5/ES2022 × CommonJS/ESNext profiles
with an explicit ES5 library. The first 32 program strings are unchanged from
the accepted-D discovery. Pinned TypeScript 5.9.3 independently produces the
complete diagnostics, UTF-16 positions and byte-exact declarations in
`reference.json`; `FROZEN.json` binds those bytes before production changes.

Native calibration: 256 comparisons, 172 accepts /84 rejects, 176 declaration
files, 344 isolated positive/negative consumers, 176 selected-edition parses
and 24 executions. Three physical standard-library files and the Native
implementation hash are bound in the reference. Twelve separate owner/runtime/
resource controls receive no direct-agreement credit.

Accepted D has 104 complete agreements /152 differences in this matrix. The
original 32-form subset has 16 agreeing and 16 differing forms in each profile.
Retain every original failure and control; do not regenerate expectations from
BlueTSC or normalize declaration text to obtain agreement.

`record_symbolic_keys.cjs --verify` replays Native without changing the reference.
`verify_symbolic_keys.cjs` checks actual BlueTSC declarations, consumers,
JavaScript syntax and execution. Rejected projects must publish no output.
The Rust harness verifies the full CLI matrix and independent API policy limits.

The independent `algebra-controls.json` supplement was frozen after the primary
repairs, before repairing its ten observed differences. Its 16 cases require
15 accepted exact declarations, one complete rejection diagnostic and no output
on rejection. Both replay tools also verify the supplement against the pinned
Native implementation/libraries and parse its 15 emitted JavaScript files.
It adds zero inventory assertions; it is required regression coverage.

Focused success does not complete P0-E. Its same-source full Native/local/maps/
projects/tooling/Windows acceptance and published evidence remain required.
All 22 broad feature rows remain open and full TypeScript parity remains OFF.
