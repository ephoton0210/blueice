# K.8.1 inferred declaration native references

TypeScript 5.9.3 records 26 forms in CommonJS and ESNext: 52 configurations,
49 accepts and three exact export-assignment rejections. Every accepted actual
emitted JavaScript executes under Node. The corpus retains exact declarations,
source hashes, primary diagnostics and execution observations without allowances.

Forms cover primitive/object/readonly/array inference, arrow and generic return
inference, overload declarations, generic and abstract classes, accessor pairs,
merged function/class/enum namespaces, nested namespaces, export assignments,
retained private aliases, and type-only imports from ambient external modules.
`ambient.d.ts` is an explicit owner project input in both ambient controls.

The portable recorder only writes when
`BLUEICE_WRITE_DECLARATION_INFERENCE_MATRIX=1`; ordinary verification requires
byte-equivalent native observations. K.8.1 production correction remains open.
