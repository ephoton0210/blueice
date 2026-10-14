# K.8.2 declaration option native references

Pinned TypeScript 5.9.3 records 48 configurations across CommonJS and ESNext:
28 accepts, 20 rejects, 24 actual emitted Node executions and four accepted
projects that publish declarations only. Exact declarations, artifact inventories,
declaration maps, source hashes and primary config/source diagnostics are retained.

Forms cover declarationDir, emitDeclarationOnly, declarationMap and
isolatedDeclarations, including dependencies on declaration, mapRoot/sourceRoot,
inlineSources/sourceMap, primitive/record/const-tuple inference, function/arrow
return annotations, mutable arrays, accessor/field inference and spread objects.

The portable recorder is read-only unless
BLUEICE_WRITE_DECLARATION_OPTIONS_MATRIX=1 is explicitly enabled.
Production correction and the complete K.0 gate remain open.
