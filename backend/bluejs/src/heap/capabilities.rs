// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

//! Pure object capabilities after a single fallible heap lookup.

use super::*;

/// An immutable result, not an object handle or a GC root. Reading these bits
/// never invokes a Proxy trap, even after its target/handler were revoked.
pub(crate) struct ObjectCapabilities {
    pub callable: bool,
    pub constructible: bool,
    pub html_dda: bool,
}

impl Heap {
    pub(crate) fn object_capabilities(
        &self,
        object: ObjectId,
    ) -> Result<ObjectCapabilities, HeapError> {
        let stored = self.object(object)?;
        let (callable, constructible) = match &stored.kind {
            ObjectKind::Proxy {
                callable,
                constructible,
                ..
            } => (*callable, *constructible),
            ObjectKind::NativeFunction { function, .. } => (true, function.has_construct()),
            ObjectKind::Closure { code, .. } => (true, code.constructible),
            ObjectKind::BoundFunction(bound) => (true, bound.constructible),
            _ => (false, false),
        };
        Ok(ObjectCapabilities {
            callable,
            constructible,
            html_dda: stored.is_html_dda,
        })
    }
}

impl NativeFunction {
    fn has_construct(self) -> bool {
        matches!(
            self,
            NativeFunction::Function
                    | NativeFunction::String
                    | NativeFunction::Array
                    | NativeFunction::Date
                    | NativeFunction::TemporalConstructor(_)
                    | NativeFunction::ArrayBuffer
                    | NativeFunction::SharedArrayBuffer
                    | NativeFunction::DataView
                    | NativeFunction::TypedArray(_)
                    // `%TypedArray%` has [[Construct]] so concrete and host
                    // subclasses may extend it, even though a direct
                    // construction attempt deliberately throws.
                    | NativeFunction::TypedArrayIntrinsic
                    | NativeFunction::Proxy
                    | NativeFunction::Map
                    | NativeFunction::Set
                    | NativeFunction::WeakMap
                    | NativeFunction::WeakSet
                    | NativeFunction::WeakRef
                    | NativeFunction::FinalizationRegistry
                    | NativeFunction::DisposableStack { .. }
                    | NativeFunction::ShadowRealm
                    | NativeFunction::Promise
                    // `Symbol` has [[Construct]] (it may head a class `extends`
                    // clause) but its behavior always throws for `new`.
                    | NativeFunction::Symbol
                    | NativeFunction::AsyncFunction
                    | NativeFunction::GeneratorFunction
                    | NativeFunction::AsyncGeneratorFunction
                    | NativeFunction::Object
                    | NativeFunction::Iterator
                    | NativeFunction::RegExp
                    | NativeFunction::Collator
                    | NativeFunction::IntlService(_)
                    | NativeFunction::Locale
                    | NativeFunction::Error(_)
                    | NativeFunction::PrimitiveConstructor(_)
                    // BigInt has [[Construct]] (`class Foo extends BigInt`
                    // is legal, and Reflect.construct(BigInt, ...) doesn't
                    // fail the IsConstructor check) even though invoking it
                    // always throws once NewTarget is observed not to be
                    // undefined -- "is a constructor" and "constructing it
                    // never actually succeeds" are independent facts.
                    | NativeFunction::BigInt
        )
    }
}
