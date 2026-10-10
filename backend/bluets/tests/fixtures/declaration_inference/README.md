# K.8.1 inferred declaration native references

TypeScript 5.9.3 records 40 forms in CommonJS and ESNext: 80 configurations,
73 accepts and seven exact export-assignment/const-position rejections. Every accepted actual
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

Sixteen additional configurations preserve regular annotated/asserted literals,
const type parameters, asserted/explicit generic arguments, annotated literal
returns, mixed fresh/regular elements and explicit enum element types. All
original 52 observations are unchanged. These controls precede array inference
production correction.

Eight further configurations pin legal const type parameters on function and
constructor type aliases and illegal positions on interfaces and generic type
aliases. Native rejects the latter with TS1277. Original 68 observations are
unchanged; this supplement precedes parser support.

Four explicit enum member assertion/annotation controls confirm that mutable arrays widen to the owning enum even when a member type is explicit. Original 76 native observations remain unchanged.
