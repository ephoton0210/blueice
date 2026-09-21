// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

use super::*;

impl Vm {
    pub(in super::super) fn global(&mut self, name: &str) -> Result<Value, RuntimeError> {
        self.string_intrinsics()?;
        if name == "String" {
            let id = self
                .string_intrinsics
                .expect("String intrinsics initialized")
                .0;
            if self.globals.insert(name.into(), id).is_none() {
                if let Some(&global) = self.globals.get("globalThis") {
                    self.define_data(global, name, Value::Object(id), true, false, true)?;
                }
            }
            return Ok(Value::Object(id));
        }
        if matches!(
            name,
            "Error"
                | "TypeError"
                | "RangeError"
                | "SyntaxError"
                | "ReferenceError"
                | "EvalError"
                | "URIError"
                | "AggregateError"
                | "SuppressedError"
        ) {
            return self.error_global(name);
        }
        if name == "Intl" {
            return self.intl_global();
        }
        if name == "Temporal" {
            return self.temporal_global();
        }
        if name == "RegExp" {
            return self.regexp_global();
        }
        if name == "Math" {
            return self.math_global();
        }
        if name == "JSON" {
            return self.json_global();
        }
        if let Some(&id) = self.globals.get(name) {
            return Ok(Value::Object(id));
        }
        let constructor = self.string_intrinsics.unwrap().0;
        let prototype = self.heap.prototype(constructor)?.unwrap();
        let native = match name {
            "Function" => NativeFunction::Function,
            "Symbol" => NativeFunction::Symbol,
            "Array" => NativeFunction::Array,
            "Date" => NativeFunction::Date,
            "ArrayBuffer" => NativeFunction::ArrayBuffer,
            "SharedArrayBuffer" => NativeFunction::SharedArrayBuffer,
            "DataView" => NativeFunction::DataView,
            "Int8Array" => NativeFunction::TypedArray(TypedArrayKind::Int8),
            "Uint8Array" => NativeFunction::TypedArray(TypedArrayKind::Uint8),
            "Uint8ClampedArray" => NativeFunction::TypedArray(TypedArrayKind::Uint8Clamped),
            "Int16Array" => NativeFunction::TypedArray(TypedArrayKind::Int16),
            "Uint16Array" => NativeFunction::TypedArray(TypedArrayKind::Uint16),
            "Int32Array" => NativeFunction::TypedArray(TypedArrayKind::Int32),
            "Uint32Array" => NativeFunction::TypedArray(TypedArrayKind::Uint32),
            "Float16Array" => NativeFunction::TypedArray(TypedArrayKind::Float16),
            "Float32Array" => NativeFunction::TypedArray(TypedArrayKind::Float32),
            "Float64Array" => NativeFunction::TypedArray(TypedArrayKind::Float64),
            "BigInt64Array" => NativeFunction::TypedArray(TypedArrayKind::BigInt64),
            "BigUint64Array" => NativeFunction::TypedArray(TypedArrayKind::BigUint64),
            "Proxy" => NativeFunction::Proxy,
            "Map" => NativeFunction::Map,
            "Set" => NativeFunction::Set,
            "WeakMap" => NativeFunction::WeakMap,
            "WeakSet" => NativeFunction::WeakSet,
            "WeakRef" => NativeFunction::WeakRef,
            "FinalizationRegistry" => NativeFunction::FinalizationRegistry,
            "DisposableStack" => NativeFunction::DisposableStack { is_async: false },
            "AsyncDisposableStack" => NativeFunction::DisposableStack { is_async: true },
            "ShadowRealm" => NativeFunction::ShadowRealm,
            "Promise" => NativeFunction::Promise,
            "eval" => NativeFunction::Eval,
            "Object" => NativeFunction::Object,
            "Iterator" => NativeFunction::Iterator,
            "Number" => NativeFunction::PrimitiveConstructor(false),
            "Boolean" => NativeFunction::PrimitiveConstructor(true),
            "BigInt" => NativeFunction::BigInt,
            "isNaN" => NativeFunction::IsNaN,
            "isFinite" => NativeFunction::IsFinite,
            "parseInt" => NativeFunction::ParseInt,
            "parseFloat" => NativeFunction::ParseFloat,
            "encodeURI" => NativeFunction::EncodeUri { component: false },
            "encodeURIComponent" => NativeFunction::EncodeUri { component: true },
            "decodeURI" => NativeFunction::DecodeUri { component: false },
            "decodeURIComponent" => NativeFunction::DecodeUri { component: true },
            // The remaining compiler-recognized globals are namespace objects.
            _ => NativeFunction::Empty,
        };
        let object_prototype = self.object_prototype;
        let native_prototype = if matches!(native, NativeFunction::TypedArray(_)) {
            self.typed_array_intrinsics()?.0
        } else {
            prototype
        };
        let id = if matches!(name, "Reflect" | "globalThis" | "Atomics") {
            self.with_roots(|heap| heap.alloc_object(Some(object_prototype)))?
        } else {
            self.with_roots(|heap| heap.alloc_native_function(native, name, native_prototype))?
        };
        let root = self.heap.root(id)?;
        let result = (|| {
            if !matches!(name, "Reflect" | "globalThis" | "Atomics") {
                self.define_data(
                    id,
                    "length",
                    Value::Number(match name {
                        "Symbol" => 0.0,
                        "Iterator" => 0.0,
                        "Proxy" => 2.0,
                        "Date" => 7.0,
                        "WeakMap" | "WeakSet" => 0.0,
                        "FinalizationRegistry" => 1.0,
                        "DisposableStack" | "AsyncDisposableStack" => 0.0,
                        "ShadowRealm" => 0.0,
                        _ if matches!(native, NativeFunction::TypedArray(_)) => 3.0,
                        _ => 1.0,
                    }),
                    false,
                    false,
                    true,
                )?;
                self.define_data(id, "name", Value::String(name.into()), false, false, true)?;
            }
            if name == "Symbol" {
                let symbol_prototype =
                    self.with_roots(|heap| heap.alloc_object(Some(object_prototype)))?;
                self.define_data(
                    id,
                    "prototype",
                    Value::Object(symbol_prototype),
                    false,
                    false,
                    false,
                )?;
                self.define_data(
                    symbol_prototype,
                    "constructor",
                    Value::Object(id),
                    true,
                    false,
                    true,
                )?;
                self.install_native(
                    symbol_prototype,
                    prototype,
                    "toString",
                    0,
                    NativeFunction::SymbolToString,
                )?;
                self.install_native(
                    symbol_prototype,
                    prototype,
                    "valueOf",
                    0,
                    NativeFunction::SymbolValueOf,
                )?;
                self.install_getter(
                    symbol_prototype,
                    prototype,
                    "description".into(),
                    "get description",
                    NativeFunction::SymbolDescription,
                )?;
                self.install_symbol_native(
                    symbol_prototype,
                    prototype,
                    "toPrimitive",
                    1,
                    NativeFunction::SymbolValueOf,
                )?;
                let to_primitive = self
                    .heap
                    .get(symbol_prototype, JsSymbol::well_known("toPrimitive"))?;
                self.define_data(
                    symbol_prototype,
                    JsSymbol::well_known("toPrimitive"),
                    to_primitive,
                    false,
                    false,
                    true,
                )?;
                self.define_data(
                    symbol_prototype,
                    JsSymbol::well_known("toStringTag"),
                    Value::String("Symbol".into()),
                    false,
                    false,
                    true,
                )?;
                self.install_native(id, prototype, "for", 1, NativeFunction::SymbolFor)?;
                self.install_native(id, prototype, "keyFor", 1, NativeFunction::SymbolKeyFor)?;
                for &name in crate::property::WELL_KNOWN {
                    self.define_data(
                        id,
                        name,
                        Value::Symbol(JsSymbol::well_known(name)),
                        false,
                        false,
                        false,
                    )?;
                }
            } else if name == "Promise" {
                let promise_prototype = self.promise_prototype()?;
                self.define_data(
                    id,
                    "prototype",
                    Value::Object(promise_prototype),
                    false,
                    false,
                    false,
                )?;
                self.define_data(
                    promise_prototype,
                    "constructor",
                    Value::Object(id),
                    true,
                    false,
                    true,
                )?;
                self.install_native(id, prototype, "resolve", 1, NativeFunction::PromiseResolve)?;
                self.install_native(id, prototype, "reject", 1, NativeFunction::PromiseReject)?;
                self.install_native(id, prototype, "all", 1, NativeFunction::PromiseAll)?;
                self.install_native(id, prototype, "race", 1, NativeFunction::PromiseRace)?;
                self.install_native(id, prototype, "any", 1, NativeFunction::PromiseAny)?;
                self.install_native(
                    id,
                    prototype,
                    "allSettled",
                    1,
                    NativeFunction::PromiseAllSettled,
                )?;
                self.install_native(
                    id,
                    prototype,
                    "withResolvers",
                    0,
                    NativeFunction::PromiseWithResolvers,
                )?;
            } else if name == "Iterator" {
                let iterator_prototype = self.base_iterator_prototype()?;
                self.define_data(
                    id,
                    "prototype",
                    Value::Object(iterator_prototype),
                    false,
                    false,
                    false,
                )?;
                self.install_native_accessor(
                    iterator_prototype,
                    prototype,
                    "constructor",
                    NativeFunction::IteratorConstructorGetter,
                    NativeFunction::IteratorConstructorSetter,
                )?;
                self.install_native(id, prototype, "from", 1, NativeFunction::IteratorFrom)?;
                self.install_native(
                    id,
                    prototype,
                    "concat",
                    0,
                    NativeFunction::IteratorHelper(native::IteratorHelperMethod::Concat),
                )?;
                self.install_native(
                    id,
                    prototype,
                    "zip",
                    1,
                    NativeFunction::IteratorHelper(native::IteratorHelperMethod::Zip),
                )?;
                self.install_native(
                    id,
                    prototype,
                    "zipKeyed",
                    1,
                    NativeFunction::IteratorHelper(native::IteratorHelperMethod::ZipKeyed),
                )?;
            } else if name == "Array" {
                self.define_data(
                    id,
                    "prototype",
                    Value::Object(self.array_prototype),
                    false,
                    false,
                    false,
                )?;
                self.define_data(
                    self.array_prototype,
                    "constructor",
                    Value::Object(id),
                    true,
                    false,
                    true,
                )?;
                self.install_native(id, prototype, "isArray", 1, NativeFunction::ArrayIsArray)?;
                self.install_native(id, prototype, "of", 0, NativeFunction::ArrayOf)?;
                self.install_native(id, prototype, "from", 1, NativeFunction::ArrayFrom)?;
                self.install_native(
                    id,
                    prototype,
                    "fromAsync",
                    1,
                    NativeFunction::ArrayFromAsync,
                )?;
                self.install_symbol_native_getter(
                    id,
                    prototype,
                    "species",
                    NativeFunction::ArraySpecies,
                )?;
            } else if name == "Date" {
                // `%Date.prototype%` has ordinary object behavior and the
                // Date tag, but deliberately does not have a [[DateValue]].
                let date_prototype =
                    self.with_roots(|heap| heap.alloc_object(Some(object_prototype)))?;
                self.date_prototype = Some(date_prototype);
                self.define_data(
                    id,
                    "prototype",
                    Value::Object(date_prototype),
                    false,
                    false,
                    false,
                )?;
                self.define_data(
                    date_prototype,
                    "constructor",
                    Value::Object(id),
                    true,
                    false,
                    true,
                )?;
                self.install_native(id, prototype, "now", 0, NativeFunction::DateNow)?;
                self.install_native(id, prototype, "parse", 1, NativeFunction::DateParse)?;
                self.install_native(id, prototype, "UTC", 7, NativeFunction::DateUtc)?;
                for (name, length, method) in [
                    ("getYear", 0, native::DateMethod::GetYear),
                    ("getTime", 0, native::DateMethod::GetTime),
                    ("setTime", 1, native::DateMethod::SetTime),
                    ("toISOString", 0, native::DateMethod::ToIsoString),
                    ("toJSON", 1, native::DateMethod::ToJson),
                    ("toDateString", 0, native::DateMethod::ToDateString),
                    ("toString", 0, native::DateMethod::ToString),
                    ("toTimeString", 0, native::DateMethod::ToTimeString),
                    (
                        "toLocaleDateString",
                        0,
                        native::DateMethod::ToLocaleDateString,
                    ),
                    ("toLocaleString", 0, native::DateMethod::ToLocaleString),
                    (
                        "toLocaleTimeString",
                        0,
                        native::DateMethod::ToLocaleTimeString,
                    ),
                    ("toUTCString", 0, native::DateMethod::ToUtcString),
                    ("valueOf", 0, native::DateMethod::ValueOf),
                ] {
                    self.install_native(
                        date_prototype,
                        prototype,
                        name,
                        length,
                        NativeFunction::DateMethod(method),
                    )?;
                }
                // Annex B requires this to be the same function object, not
                // merely an equivalent native implementation.
                let to_utc_string = self.heap.get(date_prototype, "toUTCString")?;
                self.define_data(
                    date_prototype,
                    "toGMTString",
                    to_utc_string,
                    true,
                    false,
                    true,
                )?;
                for (name, part) in [
                    ("getDate", native::DatePart::Date),
                    ("getDay", native::DatePart::Day),
                    ("getFullYear", native::DatePart::FullYear),
                    ("getHours", native::DatePart::Hours),
                    ("getMilliseconds", native::DatePart::Milliseconds),
                    ("getMinutes", native::DatePart::Minutes),
                    ("getMonth", native::DatePart::Month),
                    ("getSeconds", native::DatePart::Seconds),
                    ("getTimezoneOffset", native::DatePart::TimezoneOffset),
                    ("getUTCDate", native::DatePart::Date),
                    ("getUTCDay", native::DatePart::Day),
                    ("getUTCFullYear", native::DatePart::FullYear),
                    ("getUTCHours", native::DatePart::Hours),
                    ("getUTCMilliseconds", native::DatePart::Milliseconds),
                    ("getUTCMinutes", native::DatePart::Minutes),
                    ("getUTCMonth", native::DatePart::Month),
                    ("getUTCSeconds", native::DatePart::Seconds),
                ] {
                    self.install_native(
                        date_prototype,
                        prototype,
                        name,
                        0,
                        NativeFunction::DateMethod(native::DateMethod::Get(part)),
                    )?;
                }
                self.install_symbol_native(
                    date_prototype,
                    prototype,
                    "toPrimitive",
                    1,
                    NativeFunction::DateMethod(native::DateMethod::ToPrimitive),
                )?;
                self.with_roots(|heap| {
                    heap.define_own_property(
                        date_prototype,
                        JsSymbol::well_known("toPrimitive"),
                        PropertyDescriptor {
                            writable: Some(false),
                            ..PropertyDescriptor::default()
                        },
                    )
                })?;
                for (name, length, setter) in [
                    ("setDate", 1, native::DateSetter::Date),
                    ("setFullYear", 3, native::DateSetter::FullYear),
                    ("setHours", 4, native::DateSetter::Hours),
                    ("setMilliseconds", 1, native::DateSetter::Milliseconds),
                    ("setMinutes", 3, native::DateSetter::Minutes),
                    ("setMonth", 2, native::DateSetter::Month),
                    ("setSeconds", 2, native::DateSetter::Seconds),
                    ("setUTCDate", 1, native::DateSetter::Date),
                    ("setUTCFullYear", 3, native::DateSetter::FullYear),
                    ("setUTCHours", 4, native::DateSetter::Hours),
                    ("setUTCMilliseconds", 1, native::DateSetter::Milliseconds),
                    ("setUTCMinutes", 3, native::DateSetter::Minutes),
                    ("setUTCMonth", 2, native::DateSetter::Month),
                    ("setUTCSeconds", 2, native::DateSetter::Seconds),
                    ("setYear", 1, native::DateSetter::Year),
                ] {
                    self.install_native(
                        date_prototype,
                        prototype,
                        name,
                        length,
                        NativeFunction::DateMethod(native::DateMethod::Set(setter)),
                    )?;
                }
            } else if matches!(name, "ArrayBuffer" | "SharedArrayBuffer") {
                let buffer_prototype =
                    self.with_roots(|heap| heap.alloc_object(Some(object_prototype)))?;
                let shared = name == "SharedArrayBuffer";
                self.define_data(
                    id,
                    "prototype",
                    Value::Object(buffer_prototype),
                    false,
                    false,
                    false,
                )?;
                self.define_data(
                    buffer_prototype,
                    "constructor",
                    Value::Object(id),
                    true,
                    false,
                    true,
                )?;
                self.define_data(
                    buffer_prototype,
                    JsSymbol::well_known("toStringTag"),
                    Value::String(name.into()),
                    false,
                    false,
                    true,
                )?;
                self.install_native_getter(
                    buffer_prototype,
                    prototype,
                    "byteLength",
                    if shared {
                        NativeFunction::SharedArrayBufferByteLength
                    } else {
                        NativeFunction::ArrayBufferByteLength
                    },
                )?;
                if !shared {
                    self.install_native_getter(
                        buffer_prototype,
                        prototype,
                        "detached",
                        NativeFunction::ArrayBufferDetached,
                    )?;
                }
                self.install_native_getter(
                    buffer_prototype,
                    prototype,
                    "maxByteLength",
                    if shared {
                        NativeFunction::SharedArrayBufferMaxByteLength
                    } else {
                        NativeFunction::ArrayBufferMaxByteLength
                    },
                )?;
                self.install_native_getter(
                    buffer_prototype,
                    prototype,
                    if shared { "growable" } else { "resizable" },
                    if shared {
                        NativeFunction::SharedArrayBufferGrowable
                    } else {
                        NativeFunction::ArrayBufferResizable
                    },
                )?;
                self.install_native(
                    buffer_prototype,
                    prototype,
                    if shared { "grow" } else { "resize" },
                    1,
                    if shared {
                        NativeFunction::SharedArrayBufferGrow
                    } else {
                        NativeFunction::ArrayBufferResize
                    },
                )?;
                if !shared {
                    self.install_native(
                        buffer_prototype,
                        prototype,
                        "transfer",
                        0,
                        NativeFunction::ArrayBufferTransfer,
                    )?;
                    self.install_native(
                        buffer_prototype,
                        prototype,
                        "transferToFixedLength",
                        0,
                        NativeFunction::ArrayBufferTransferToFixedLength,
                    )?;
                    // Immutable ArrayBuffer proposal.
                    self.install_native_getter(
                        buffer_prototype,
                        prototype,
                        "immutable",
                        NativeFunction::ArrayBufferImmutable,
                    )?;
                    self.install_native(
                        buffer_prototype,
                        prototype,
                        "transferToImmutable",
                        0,
                        NativeFunction::ArrayBufferTransferToImmutable,
                    )?;
                    self.install_native(
                        buffer_prototype,
                        prototype,
                        "sliceToImmutable",
                        2,
                        NativeFunction::ArrayBufferSliceToImmutable,
                    )?;
                }
                self.install_native(
                    buffer_prototype,
                    prototype,
                    "slice",
                    2,
                    if shared {
                        NativeFunction::SharedArrayBufferSlice
                    } else {
                        NativeFunction::ArrayBufferSlice
                    },
                )?;
                if !shared {
                    self.install_native(
                        id,
                        prototype,
                        "isView",
                        1,
                        NativeFunction::ArrayBufferIsView,
                    )?;
                }
                self.install_getter(
                    id,
                    prototype,
                    JsSymbol::well_known("species").into(),
                    "get [Symbol.species]",
                    if shared {
                        NativeFunction::SharedArrayBufferSpecies
                    } else {
                        NativeFunction::ArrayBufferSpecies
                    },
                )?;
            } else if name == "DataView" {
                let view_prototype =
                    self.with_roots(|heap| heap.alloc_object(Some(object_prototype)))?;
                self.define_data(
                    id,
                    "prototype",
                    Value::Object(view_prototype),
                    false,
                    false,
                    false,
                )?;
                self.define_data(
                    view_prototype,
                    "constructor",
                    Value::Object(id),
                    true,
                    false,
                    true,
                )?;
                self.define_data(
                    view_prototype,
                    JsSymbol::well_known("toStringTag"),
                    Value::String("DataView".into()),
                    false,
                    false,
                    true,
                )?;
                for (name, native) in [
                    ("buffer", NativeFunction::DataViewBuffer),
                    ("byteLength", NativeFunction::DataViewByteLength),
                    ("byteOffset", NativeFunction::DataViewByteOffset),
                ] {
                    self.install_native_getter(view_prototype, prototype, name, native)?;
                }
                for (name, width, signed, floating, bigint) in [
                    ("getUint8", 1, false, false, false),
                    ("getInt8", 1, true, false, false),
                    ("getUint16", 2, false, false, false),
                    ("getInt16", 2, true, false, false),
                    ("getUint32", 4, false, false, false),
                    ("getInt32", 4, true, false, false),
                    ("getFloat16", 2, false, true, false),
                    ("getFloat32", 4, false, true, false),
                    ("getFloat64", 8, false, true, false),
                    ("getBigUint64", 8, false, false, true),
                    ("getBigInt64", 8, true, false, true),
                ] {
                    self.install_native(
                        view_prototype,
                        prototype,
                        name,
                        1,
                        NativeFunction::DataViewGet {
                            width,
                            signed,
                            floating,
                            bigint,
                        },
                    )?;
                    let set_name = name.replacen("get", "set", 1);
                    self.install_native(
                        view_prototype,
                        prototype,
                        &set_name,
                        2,
                        NativeFunction::DataViewSet {
                            width,
                            signed,
                            floating,
                            bigint,
                        },
                    )?;
                }
            } else if let Some(kind) = typed_array_kind(name) {
                let typed_array_prototype = self.typed_array_intrinsics()?.1;
                let typed_prototype =
                    self.with_roots(|heap| heap.alloc_object(Some(typed_array_prototype)))?;
                self.define_data(
                    id,
                    "prototype",
                    Value::Object(typed_prototype),
                    false,
                    false,
                    false,
                )?;
                self.define_data(
                    typed_prototype,
                    "constructor",
                    Value::Object(id),
                    true,
                    false,
                    true,
                )?;
                self.define_data(
                    id,
                    "BYTES_PER_ELEMENT",
                    Value::Number(kind.byte_width() as f64),
                    false,
                    false,
                    false,
                )?;
                self.define_data(
                    typed_prototype,
                    "BYTES_PER_ELEMENT",
                    Value::Number(kind.byte_width() as f64),
                    false,
                    false,
                    false,
                )?;
                if kind == TypedArrayKind::Uint8 {
                    self.install_native(
                        id,
                        prototype,
                        "fromBase64",
                        1,
                        NativeFunction::Uint8ArrayFromBase64,
                    )?;
                    self.install_native(
                        id,
                        prototype,
                        "fromHex",
                        1,
                        NativeFunction::Uint8ArrayFromHex,
                    )?;
                    for (method, length, native) in [
                        ("setFromBase64", 1, Uint8ArrayMethod::SetFromBase64),
                        ("setFromHex", 1, Uint8ArrayMethod::SetFromHex),
                        ("toBase64", 0, Uint8ArrayMethod::ToBase64),
                        ("toHex", 0, Uint8ArrayMethod::ToHex),
                    ] {
                        self.install_native(
                            typed_prototype,
                            prototype,
                            method,
                            length,
                            NativeFunction::Uint8ArrayMethod(native),
                        )?;
                    }
                }
            } else if name == "Proxy" {
                self.install_native(
                    id,
                    prototype,
                    "revocable",
                    2,
                    NativeFunction::ProxyRevocable,
                )?;
            } else if matches!(name, "Map" | "Set") {
                let collection_prototype = self.collection_prototype(name == "Map")?;
                self.define_data(
                    id,
                    "prototype",
                    Value::Object(collection_prototype),
                    false,
                    false,
                    false,
                )?;
                self.define_data(
                    collection_prototype,
                    "constructor",
                    Value::Object(id),
                    true,
                    false,
                    true,
                )?;
            } else if matches!(name, "WeakMap" | "WeakSet") {
                let collection_prototype = self.weak_collection_prototype(name == "WeakMap")?;
                self.define_data(
                    id,
                    "prototype",
                    Value::Object(collection_prototype),
                    false,
                    false,
                    false,
                )?;
                self.define_data(
                    collection_prototype,
                    "constructor",
                    Value::Object(id),
                    true,
                    false,
                    true,
                )?;
            } else if name == "WeakRef" {
                let weak_ref_prototype = self.weak_ref_prototype()?;
                self.define_data(
                    id,
                    "prototype",
                    Value::Object(weak_ref_prototype),
                    false,
                    false,
                    false,
                )?;
                self.define_data(
                    weak_ref_prototype,
                    "constructor",
                    Value::Object(id),
                    true,
                    false,
                    true,
                )?;
            } else if name == "FinalizationRegistry" {
                let registry_prototype = self.finalization_registry_prototype()?;
                self.define_data(
                    id,
                    "prototype",
                    Value::Object(registry_prototype),
                    false,
                    false,
                    false,
                )?;
                self.define_data(
                    registry_prototype,
                    "constructor",
                    Value::Object(id),
                    true,
                    false,
                    true,
                )?;
            } else if name == "DisposableStack" || name == "AsyncDisposableStack" {
                let is_async = name == "AsyncDisposableStack";
                let stack_prototype = self.disposable_stack_prototype(is_async)?;
                self.define_data(
                    id,
                    "prototype",
                    Value::Object(stack_prototype),
                    false,
                    false,
                    false,
                )?;
                self.define_data(
                    stack_prototype,
                    "constructor",
                    Value::Object(id),
                    true,
                    false,
                    true,
                )?;
            } else if name == "ShadowRealm" {
                let shadow_realm_prototype = self.shadow_realm_prototype()?;
                self.define_data(
                    id,
                    "prototype",
                    Value::Object(shadow_realm_prototype),
                    false,
                    false,
                    false,
                )?;
                self.define_data(
                    shadow_realm_prototype,
                    "constructor",
                    Value::Object(id),
                    true,
                    false,
                    true,
                )?;
            } else if name == "Function" {
                self.define_data(
                    id,
                    "prototype",
                    Value::Object(prototype),
                    false,
                    false,
                    false,
                )?;
                self.define_data(
                    prototype,
                    "constructor",
                    Value::Object(id),
                    true,
                    false,
                    true,
                )?;
                let thrower = self.throw_type_error()?;
                let restricted = PropertyDescriptor {
                    get: Some(Value::Object(thrower)),
                    set: Some(Value::Object(thrower)),
                    enumerable: Some(false),
                    configurable: Some(true),
                    ..PropertyDescriptor::default()
                };
                for name in ["caller", "arguments"] {
                    let defined = self.with_roots(|heap| {
                        heap.define_own_property(prototype, name, restricted.clone())
                    })?;
                    assert!(defined, "Function prototype accepts restricted properties");
                }
            } else if matches!(name, "Number" | "Boolean" | "BigInt") {
                let boolean = name == "Boolean";
                let bigint = name == "BigInt";
                let value = if boolean {
                    Value::Bool(false)
                } else {
                    Value::Number(0.0)
                };
                // Unlike %Number.prototype%/%Boolean.prototype%, the BigInt
                // prototype is explicitly *not* a BigInt exotic object (no
                // [[BigIntData]] internal slot) per "Properties of the
                // BigInt Prototype Object" -- an ordinary object instead, so
                // e.g. `BigInt.prototype.toString(1)` throws TypeError
                // rather than treating the prototype itself as 0n.
                let boxed_prototype = self.with_roots(|heap| {
                    if bigint {
                        heap.alloc_object(Some(object_prototype))
                    } else {
                        heap.alloc_boxed_primitive(value, object_prototype)
                    }
                })?;
                self.define_data(
                    id,
                    "prototype",
                    Value::Object(boxed_prototype),
                    false,
                    false,
                    false,
                )?;
                self.define_data(
                    boxed_prototype,
                    "constructor",
                    Value::Object(id),
                    true,
                    false,
                    true,
                )?;
                if bigint {
                    self.install_native(
                        boxed_prototype,
                        prototype,
                        "toString",
                        0,
                        NativeFunction::BigIntToString,
                    )?;
                    self.install_native(
                        boxed_prototype,
                        prototype,
                        "valueOf",
                        0,
                        NativeFunction::BigIntValueOf,
                    )?;
                    self.install_native(
                        boxed_prototype,
                        prototype,
                        "toLocaleString",
                        0,
                        NativeFunction::BigIntToLocaleString,
                    )?;
                    self.define_data(
                        boxed_prototype,
                        JsSymbol::well_known("toStringTag"),
                        Value::String("BigInt".into()),
                        false,
                        false,
                        true,
                    )?;
                    self.install_native(id, prototype, "asIntN", 2, NativeFunction::BigIntAsIntN)?;
                    self.install_native(
                        id,
                        prototype,
                        "asUintN",
                        2,
                        NativeFunction::BigIntAsUintN,
                    )?;
                } else {
                    if name != "Number" {
                        self.install_native(
                            boxed_prototype,
                            prototype,
                            "toString",
                            0,
                            NativeFunction::PrimitiveMethod {
                                boolean,
                                string: true,
                            },
                        )?;
                    }
                    self.install_native(
                        boxed_prototype,
                        prototype,
                        "valueOf",
                        0,
                        NativeFunction::PrimitiveMethod {
                            boolean,
                            string: false,
                        },
                    )?;
                }
                if name == "Number" {
                    for (property, length, method) in [
                        ("toString", 1, native::NumberMethod::ToString),
                        ("toLocaleString", 0, native::NumberMethod::LocaleString),
                        ("toFixed", 1, native::NumberMethod::Fixed),
                        ("toExponential", 1, native::NumberMethod::Exponential),
                        ("toPrecision", 1, native::NumberMethod::Precision),
                    ] {
                        self.install_native(
                            boxed_prototype,
                            prototype,
                            property,
                            length,
                            NativeFunction::NumberMethod(method),
                        )?;
                    }
                    for (property, value) in [
                        ("EPSILON", f64::EPSILON),
                        ("MAX_SAFE_INTEGER", 9_007_199_254_740_991.0),
                        ("MAX_VALUE", f64::MAX),
                        ("MIN_SAFE_INTEGER", -9_007_199_254_740_991.0),
                        ("MIN_VALUE", f64::from_bits(1)),
                        ("NaN", f64::NAN),
                        ("NEGATIVE_INFINITY", f64::NEG_INFINITY),
                        ("POSITIVE_INFINITY", f64::INFINITY),
                    ] {
                        self.define_data(id, property, Value::Number(value), false, false, false)?;
                    }
                    self.install_native(
                        id,
                        prototype,
                        "isFinite",
                        1,
                        NativeFunction::NumberIsFinite,
                    )?;
                    self.install_native(id, prototype, "isNaN", 1, NativeFunction::NumberIsNaN)?;
                    self.install_native(
                        id,
                        prototype,
                        "isInteger",
                        1,
                        NativeFunction::NumberIsInteger,
                    )?;
                    self.install_native(
                        id,
                        prototype,
                        "isSafeInteger",
                        1,
                        NativeFunction::NumberIsSafeInteger,
                    )?;
                    self.install_native(id, prototype, "parseInt", 2, NativeFunction::ParseInt)?;
                }
            } else if name == "globalThis" {
                self.define_data(id, "String", Value::Object(constructor), true, false, true)?;
                self.define_data(id, "globalThis", Value::Object(id), true, false, true)?;
                // A global-property lookup must observe Date before any
                // lexical `Date` reference has caused its lazy intrinsic to
                // be materialized (for example, property-descriptor probes).
                // It is then copied into this global object below along with
                // the already-created String intrinsic.
                self.global("Date")?;
            } else if name == "Reflect" {
                self.install_native(id, prototype, "apply", 3, NativeFunction::ReflectApply)?;
                self.install_native(
                    id,
                    prototype,
                    "ownKeys",
                    1,
                    NativeFunction::ObjectMethod(ObjectMethod::OwnKeys),
                )?;
                self.install_native(
                    id,
                    prototype,
                    "construct",
                    2,
                    NativeFunction::ReflectConstruct,
                )?;
                for (name, length, method) in [
                    ("get", 2, ObjectMethod::ReflectGet),
                    (
                        "getOwnPropertyDescriptor",
                        2,
                        ObjectMethod::ReflectGetOwnPropertyDescriptor,
                    ),
                    ("getPrototypeOf", 1, ObjectMethod::ReflectGetPrototypeOf),
                    ("defineProperty", 3, ObjectMethod::ReflectDefineProperty),
                    ("set", 3, ObjectMethod::ReflectSet),
                    ("deleteProperty", 2, ObjectMethod::ReflectDeleteProperty),
                    (
                        "preventExtensions",
                        1,
                        ObjectMethod::ReflectPreventExtensions,
                    ),
                    ("setPrototypeOf", 2, ObjectMethod::ReflectSetPrototypeOf),
                    ("isExtensible", 1, ObjectMethod::ReflectIsExtensible),
                    ("has", 2, ObjectMethod::ReflectHas),
                ] {
                    self.install_native(
                        id,
                        prototype,
                        name,
                        length,
                        NativeFunction::ObjectMethod(method),
                    )?;
                }
                self.define_data(
                    id,
                    JsSymbol::well_known("toStringTag"),
                    Value::String("Reflect".into()),
                    false,
                    false,
                    true,
                )?;
            } else if name == "Atomics" {
                for (name, length, function) in [
                    ("add", 3, NativeFunction::Atomics(AtomicOp::Add)),
                    ("and", 3, NativeFunction::Atomics(AtomicOp::And)),
                    (
                        "compareExchange",
                        4,
                        NativeFunction::Atomics(AtomicOp::CompareExchange),
                    ),
                    ("exchange", 3, NativeFunction::Atomics(AtomicOp::Exchange)),
                    ("load", 2, NativeFunction::Atomics(AtomicOp::Load)),
                    ("or", 3, NativeFunction::Atomics(AtomicOp::Or)),
                    ("store", 3, NativeFunction::Atomics(AtomicOp::Store)),
                    ("sub", 3, NativeFunction::Atomics(AtomicOp::Sub)),
                    ("xor", 3, NativeFunction::Atomics(AtomicOp::Xor)),
                    ("isLockFree", 1, NativeFunction::AtomicsIsLockFree),
                    ("notify", 3, NativeFunction::AtomicsNotify),
                    ("pause", 0, NativeFunction::AtomicsPause),
                    ("wait", 4, NativeFunction::AtomicsWait),
                    ("waitAsync", 4, NativeFunction::AtomicsWaitAsync),
                ] {
                    self.install_native(id, prototype, name, length, function)?;
                }
                self.define_data(
                    id,
                    JsSymbol::well_known("toStringTag"),
                    Value::String("Atomics".into()),
                    false,
                    false,
                    true,
                )?;
            } else if name == "Object" {
                self.define_data(
                    id,
                    "prototype",
                    Value::Object(self.object_prototype),
                    false,
                    false,
                    false,
                )?;
                self.define_data(
                    self.object_prototype,
                    "constructor",
                    Value::Object(id),
                    true,
                    false,
                    true,
                )?;
                use ObjectMethod::*;
                for (name, length, method) in [
                    ("assign", 2, Assign),
                    ("fromEntries", 1, FromEntries),
                    ("groupBy", 2, GroupBy),
                    ("hasOwn", 2, HasOwn),
                    ("is", 2, Is),
                    ("getOwnPropertyDescriptor", 2, GetOwnPropertyDescriptor),
                    ("getOwnPropertyDescriptors", 1, GetOwnPropertyDescriptors),
                    ("defineProperty", 3, DefineProperty),
                    ("defineProperties", 2, DefineProperties),
                    ("keys", 1, Keys),
                    ("values", 1, Values),
                    ("entries", 1, Entries),
                    ("getOwnPropertyNames", 1, GetOwnPropertyNames),
                    ("getOwnPropertySymbols", 1, GetOwnPropertySymbols),
                    ("getPrototypeOf", 1, GetPrototypeOf),
                    ("setPrototypeOf", 2, SetPrototypeOf),
                    ("create", 2, Create),
                    ("isExtensible", 1, IsExtensible),
                    ("preventExtensions", 1, PreventExtensions),
                    ("seal", 1, Seal),
                    ("freeze", 1, Freeze),
                    ("isSealed", 1, IsSealed),
                    ("isFrozen", 1, IsFrozen),
                ] {
                    self.install_native(
                        id,
                        prototype,
                        name,
                        length,
                        NativeFunction::ObjectMethod(method),
                    )?;
                }
            }
            Ok(Value::Object(id))
        })();
        if result.is_err() {
            self.heap.unroot(root)?;
        } else {
            self.globals.insert(name.into(), id);
            if name == "globalThis" {
                let globals = self.globals.clone();
                for (global_name, value) in globals {
                    if global_name != "globalThis" {
                        self.define_data(id, global_name, Value::Object(value), true, false, true)?;
                    }
                }
            } else if let Some(&global) = self.globals.get("globalThis") {
                self.define_data(global, name, Value::Object(id), true, false, true)?;
            }
        }
        result
    }
}
