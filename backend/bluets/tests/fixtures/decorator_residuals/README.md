# K.7.4 decorator residuals

Pinned TypeScript 5.9.3 records six additional forms across eleven ECMAScript
targets, both module modes and both class-field modes: 264 configurations,
240 accepts and 24 exact no-output rejections. The forms cover a replaced
private auto-accessor, a private setter and static super reads/writes in a
field initializer, block, getter and setter with a replacement class.

All accepted actual native JavaScript executes: 228 directly under Node and
12 preserved ESNext proposal outputs through the same documented
TypeScript 5.9.3 emitted-JavaScript-only ES2023 adapter as `decorator_targets`.
Keep raw output, syntax/proposal observations, precise diagnostics and exact
declarations. Original TypeScript transpilation is never used as runtime
execution evidence. The independent 880-case corpus remains unchanged.

Regenerate only with `BLUEICE_WRITE_DECORATOR_RESIDUALS_MATRIX=1` and the pinned
compiler; otherwise `tools/record_decorator_residuals.cjs` verifies every row.
