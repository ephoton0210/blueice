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
        ) {
            return self.error_global(name);
        }
        if name == "Intl" {
            return self.intl_global();
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
            "Float32Array" => NativeFunction::TypedArray(TypedArrayKind::Float32),
            "Float64Array" => NativeFunction::TypedArray(TypedArrayKind::Float64),
            "BigInt64Array" => NativeFunction::TypedArray(TypedArrayKind::BigInt64),
            "BigUint64Array" => NativeFunction::TypedArray(TypedArrayKind::BigUint64),
            "Proxy" => NativeFunction::Proxy,
            "Map" => NativeFunction::Map,
            "Set" => NativeFunction::Set,
            "Promise" => NativeFunction::Promise,
            "eval" => NativeFunction::Eval,
            "Object" => NativeFunction::Object,
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
        let id = if matches!(name, "Reflect" | "globalThis" | "import" | "Atomics") {
            self.with_roots(|heap| heap.alloc_object(Some(object_prototype)))?
        } else {
            self.with_roots(|heap| heap.alloc_native_function(native, name, native_prototype))?
        };
        let root = self.heap.root(id)?;
        let result = (|| {
            if !matches!(name, "Reflect" | "globalThis" | "import" | "Atomics") {
                self.define_data(
                    id,
                    "length",
                    Value::Number(match name {
                        "Symbol" => 0.0,
                        "Proxy" => 2.0,
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
                self.install_native(
                    id,
                    prototype,
                    "withResolvers",
                    0,
                    NativeFunction::PromiseWithResolvers,
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
                self.install_native(id, prototype, "from", 1, NativeFunction::ArrayFrom)?;
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
                    typed_prototype,
                    JsSymbol::well_known("toStringTag"),
                    Value::String(kind.name().into()),
                    false,
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
                    false,
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
                } else if bigint {
                    Value::BigInt(0.into())
                } else {
                    Value::Number(0.0)
                };
                let boxed_prototype =
                    self.with_roots(|heap| heap.alloc_boxed_primitive(value, object_prototype))?;
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
                } else {
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
                }
            } else if name == "globalThis" {
                self.define_data(id, "String", Value::Object(constructor), true, false, true)?;
                self.define_data(id, "globalThis", Value::Object(id), true, false, true)?;
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
                    ("getOwnPropertyDescriptor", 2, GetOwnPropertyDescriptor),
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
            } else if name == "import" {
                self.install_native(
                    id,
                    prototype,
                    "source",
                    1,
                    NativeFunction::DynamicImport { source: true },
                )?;
                self.install_native(
                    id,
                    prototype,
                    "defer",
                    1,
                    NativeFunction::DynamicImport { source: false },
                )?;
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
