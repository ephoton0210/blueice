// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

//! Native call identities and String algorithms from ECMA-262 edition 17.
//! Heap/receiver dispatch stays in the VM; these operations preserve code
//! units and bound string growth before allocating the result.

use crate::{
    heap::{ArrayIteratorKind, TypedArrayKind},
    primitive, JsString, ObjectId, RuntimeError, Value,
};
use unicode_normalization::UnicodeNormalization;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum AtomicOp {
    Add,
    And,
    CompareExchange,
    Exchange,
    Load,
    Or,
    Store,
    Sub,
    Xor,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum TypedArrayMethod {
    At,
    CopyWithin,
    Every,
    Fill,
    Filter,
    Find,
    FindIndex,
    FindLast,
    FindLastIndex,
    Map,
    LastIndexOf,
    ForEach,
    Includes,
    IndexOf,
    Join,
    Reduce,
    ToString,
    ReduceRight,
    Reverse,
    Slice,
    Some,
    Sort,
    ToReversed,
    ToSorted,
    With,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum NativeFunction {
    Function,
    /// The intrinsic constructor reached through an async function's
    /// `constructor` property. It is deliberately not installed as a global.
    AsyncFunction,
    String,
    Array,
    Date,
    DateNow,
    ArrayBuffer,
    ArrayBufferByteLength,
    ArrayBufferMaxByteLength,
    ArrayBufferResizable,
    ArrayBufferResize,
    ArrayBufferTransfer,
    ArrayBufferTransferToFixedLength,
    ArrayBufferSlice,
    ArrayBufferIsView,
    ArrayBufferSpecies,
    SharedArrayBuffer,
    SharedArrayBufferByteLength,
    SharedArrayBufferMaxByteLength,
    SharedArrayBufferGrowable,
    SharedArrayBufferGrow,
    SharedArrayBufferSlice,
    SharedArrayBufferSpecies,
    Atomics(AtomicOp),
    AtomicsIsLockFree,
    AtomicsNotify,
    AtomicsPause,
    AtomicsWait,
    AtomicsWaitAsync,
    DataView,
    DataViewBuffer,
    DataViewByteLength,
    DataViewByteOffset,
    DataViewGet {
        width: usize,
        signed: bool,
        floating: bool,
        bigint: bool,
    },
    DataViewSet {
        width: usize,
        signed: bool,
        floating: bool,
        bigint: bool,
    },
    TypedArray(TypedArrayKind),
    /// The abstract `%TypedArray%` constructor. It exists only as the common
    /// prototype of concrete typed-array constructors and must not construct.
    TypedArrayIntrinsic,
    TypedArrayBuffer,
    TypedArrayByteLength,
    TypedArrayByteOffset,
    TypedArrayLength,
    TypedArraySet,
    TypedArraySubarray,
    TypedArraySpecies,
    TypedArrayIterator(ArrayIteratorKind),
    TypedArrayMethod(TypedArrayMethod),
    Proxy,
    ProxyRevocable,
    ProxyRevoker(ObjectId),
    Map,
    Set,
    ArrayIsArray,
    ArrayFrom,
    ArrayForEach,
    ArrayFilter,
    ArrayIncludes,
    ArrayReduce,
    ArrayPush,
    ArrayIndexOf,
    ArraySlice,
    ArraySplice,
    ArraySort,
    Eval,
    IsNaN,
    IsFinite,
    ParseInt,
    ParseFloat,
    EncodeUri {
        component: bool,
    },
    DecodeUri {
        component: bool,
    },
    /// The source/defer variants of dynamic import. They remain separate from
    /// ordinary `import()` because their host phase is observable.
    DynamicImport {
        source: bool,
    },
    JsonParse,
    JsonStringify,
    Math(MathMethod),
    Error(&'static str),
    ErrorToString,
    /// Test262 exposes this otherwise non-global intrinsic through `$262`.
    AbstractModuleSource,
    AbstractModuleSourceToStringTag,
    Test262(&'static str),
    ToLocaleLowerCase,
    ToLocaleUpperCase,
    LocaleCompare,
    Collator,
    Locale,
    CanonicalLocales,
    SupportedLocales,
    CollatorCompareGetter,
    CollatorCompare,
    CollatorResolvedOptions,
    LocaleToString,
    LocaleMaximize,
    LocaleMinimize,
    LocaleGetter(LocaleGetter),
    LocaleInfo(LocaleInfo),
    FromCharCode,
    FromCodePoint,
    Raw,
    Split,
    Replace,
    ReplaceAll,
    Call,
    Apply,
    Bind,
    HasInstance,
    ReflectApply,
    ReflectConstruct,
    FunctionToString,
    ThrowTypeError,
    Empty,
    ObjectToString,
    ObjectValueOf,
    ObjectIsPrototypeOf,
    ArrayToString,
    ArrayConcat,
    ArrayJoin,
    ArrayIterator(ArrayIteratorKind),
    ArrayIteratorNext,
    GeneratorNext,
    GeneratorReturn,
    GeneratorThrow,
    AsyncGeneratorNext,
    AsyncGeneratorReturn,
    AsyncGeneratorThrow,
    Promise,
    /// One half of a Promise capability. The target identity is carried by
    /// the otherwise ordinary native function, so the resolving functions
    /// can be passed to an executor without exposing VM bookkeeping to JS.
    PromiseResolvingFunction {
        promise: ObjectId,
        fulfill: bool,
    },
    /// The executor supplied while `NewPromiseCapability(C)` invokes a
    /// user-provided constructor. The storage object keeps its resolve/reject
    /// pair alive and observable for the enclosing static Promise method.
    PromiseCapabilityExecutor {
        storage: ObjectId,
    },
    AsyncFromSyncFulfill {
        target: ObjectId,
        done: bool,
    },
    AsyncFromSyncReject {
        target: ObjectId,
        record: ObjectId,
    },
    PromiseThen,
    PromiseCatch,
    PromiseFinally,
    PromiseResolve,
    PromiseReject,
    PromiseAll,
    PromiseAllResolve {
        target: ObjectId,
        index: u32,
    },
    PromiseAllReject {
        target: ObjectId,
    },
    PromiseWithResolvers,
    Test262Done,
    Symbol,
    SymbolToString,
    SymbolValueOf,
    BigInt,
    BigIntToString,
    BigIntValueOf,
    PrimitiveConstructor(bool),
    PrimitiveMethod {
        boolean: bool,
        string: bool,
    },
    Object,
    ObjectMethod(ObjectMethod),
    StringIterator,
    IteratorNext,
    IteratorSelf,
    AsyncIteratorSelf,
    Pattern(PatternMethod),
    RegExp,
    RegExpEscape,
    RegExpMethod(RegExpMethod),
    RegExpGetter(&'static str),
    RegExpIteratorNext,
    StringMethod(StringMethod),
}

/// Failures particular to the URI encode/decode abstract operations.  The VM
/// turns malformed input into the realm's `URIError` object, while preserving
/// its normal resource-limit reporting for an oversized result.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum UriCodingError {
    Malformed,
    StringLimit { limit: usize },
}

impl NativeFunction {
    /// Native closures can carry heap identities outside ordinary property
    /// storage.  Keep those identities visible to both allocation protection
    /// and tracing, just like captured JavaScript closures.
    pub(crate) fn references(self) -> Vec<ObjectId> {
        match self {
            Self::ProxyRevoker(proxy) => vec![proxy],
            Self::PromiseResolvingFunction { promise, .. } => vec![promise],
            Self::PromiseCapabilityExecutor { storage } => vec![storage],
            Self::AsyncFromSyncFulfill { target, .. } => vec![target],
            Self::AsyncFromSyncReject { target, record } => vec![target, record],
            Self::PromiseAllResolve { target, .. } | Self::PromiseAllReject { target } => {
                vec![target]
            }
            _ => Vec::new(),
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum MathMethod {
    Abs,
    Acos,
    Acosh,
    Asin,
    Asinh,
    Atan,
    Atanh,
    Atan2,
    Ceil,
    Cbrt,
    Cos,
    Cosh,
    Exp,
    Expm1,
    Floor,
    Fround,
    Hypot,
    Imul,
    Log,
    Log1p,
    Log2,
    Log10,
    Max,
    Min,
    Pow,
    Random,
    Round,
    Sign,
    Sin,
    Sinh,
    Sqrt,
    Tan,
    Tanh,
    Trunc,
    Clz32,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum ObjectMethod {
    GetOwnPropertyDescriptor,
    DefineProperty,
    DefineProperties,
    Keys,
    Values,
    Entries,
    GetOwnPropertyNames,
    GetOwnPropertySymbols,
    GetPrototypeOf,
    SetPrototypeOf,
    Create,
    OwnKeys,
    IsExtensible,
    PreventExtensions,
    Seal,
    Freeze,
    IsSealed,
    IsFrozen,
    ReflectGet,
    ReflectGetOwnPropertyDescriptor,
    ReflectGetPrototypeOf,
    ReflectDefineProperty,
    ReflectSet,
    ReflectDeleteProperty,
    ReflectPreventExtensions,
    ReflectSetPrototypeOf,
    ReflectIsExtensible,
    ReflectHas,
    HasOwnProperty,
    PropertyIsEnumerable,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum LocaleGetter {
    BaseName,
    Language,
    Script,
    Region,
    Variants,
    Calendar,
    Collation,
    HourCycle,
    CaseFirst,
    Numeric,
    NumberingSystem,
    FirstDayOfWeek,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum LocaleInfo {
    Calendars,
    Collations,
    HourCycles,
    NumberingSystems,
    TextInfo,
    TimeZones,
    WeekInfo,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum PatternMethod {
    Match,
    MatchAll,
    Search,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum RegExpMethod {
    Exec,
    Test,
    ToString,
    Match,
    MatchAll,
    Search,
    Split,
    Replace,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum StringMethod {
    At,
    CharAt,
    CharCodeAt,
    CodePointAt,
    Concat,
    EndsWith,
    Includes,
    IndexOf,
    LastIndexOf,
    StartsWith,
    Slice,
    Substring,
    IsWellFormed,
    ToWellFormed,
    ToString,
    ValueOf,
    PadStart,
    PadEnd,
    Repeat,
    Trim,
    TrimStart,
    TrimEnd,
    Normalize,
    ToLowerCase,
    ToUpperCase,
    Substr,
    Html {
        tag: &'static str,
        attribute: &'static str,
    },
}

pub(crate) const STRING_METHODS: &[(&str, u32, StringMethod)] = &[
    ("at", 1, StringMethod::At),
    ("charAt", 1, StringMethod::CharAt),
    ("charCodeAt", 1, StringMethod::CharCodeAt),
    ("codePointAt", 1, StringMethod::CodePointAt),
    ("concat", 1, StringMethod::Concat),
    ("endsWith", 1, StringMethod::EndsWith),
    ("includes", 1, StringMethod::Includes),
    ("indexOf", 1, StringMethod::IndexOf),
    ("lastIndexOf", 1, StringMethod::LastIndexOf),
    ("startsWith", 1, StringMethod::StartsWith),
    ("slice", 2, StringMethod::Slice),
    ("substring", 2, StringMethod::Substring),
    ("isWellFormed", 0, StringMethod::IsWellFormed),
    ("toWellFormed", 0, StringMethod::ToWellFormed),
    ("toString", 0, StringMethod::ToString),
    ("valueOf", 0, StringMethod::ValueOf),
    ("padStart", 1, StringMethod::PadStart),
    ("padEnd", 1, StringMethod::PadEnd),
    ("repeat", 1, StringMethod::Repeat),
    ("trim", 0, StringMethod::Trim),
    ("trimStart", 0, StringMethod::TrimStart),
    ("trimEnd", 0, StringMethod::TrimEnd),
    ("normalize", 0, StringMethod::Normalize),
    ("toLowerCase", 0, StringMethod::ToLowerCase),
    ("toUpperCase", 0, StringMethod::ToUpperCase),
    ("substr", 2, StringMethod::Substr),
    (
        "anchor",
        1,
        StringMethod::Html {
            tag: "a",
            attribute: "name",
        },
    ),
    (
        "big",
        0,
        StringMethod::Html {
            tag: "big",
            attribute: "",
        },
    ),
    (
        "blink",
        0,
        StringMethod::Html {
            tag: "blink",
            attribute: "",
        },
    ),
    (
        "bold",
        0,
        StringMethod::Html {
            tag: "b",
            attribute: "",
        },
    ),
    (
        "fixed",
        0,
        StringMethod::Html {
            tag: "tt",
            attribute: "",
        },
    ),
    (
        "fontcolor",
        1,
        StringMethod::Html {
            tag: "font",
            attribute: "color",
        },
    ),
    (
        "fontsize",
        1,
        StringMethod::Html {
            tag: "font",
            attribute: "size",
        },
    ),
    (
        "italics",
        0,
        StringMethod::Html {
            tag: "i",
            attribute: "",
        },
    ),
    (
        "link",
        1,
        StringMethod::Html {
            tag: "a",
            attribute: "href",
        },
    ),
    (
        "small",
        0,
        StringMethod::Html {
            tag: "small",
            attribute: "",
        },
    ),
    (
        "strike",
        0,
        StringMethod::Html {
            tag: "strike",
            attribute: "",
        },
    ),
    (
        "sub",
        0,
        StringMethod::Html {
            tag: "sub",
            attribute: "",
        },
    ),
    (
        "sup",
        0,
        StringMethod::Html {
            tag: "sup",
            attribute: "",
        },
    ),
];

pub(crate) fn argument(args: &[Value], index: usize) -> &Value {
    args.get(index).unwrap_or(&Value::Undefined)
}

pub(crate) fn integer(value: &Value) -> Result<f64, RuntimeError> {
    let number = primitive::number(value)?;
    Ok(if number.is_nan() { 0.0 } else { number.trunc() })
}

pub(crate) fn uint32(value: &Value) -> Result<u32, RuntimeError> {
    Ok(primitive::to_uint32(primitive::number(value)?))
}

pub(crate) fn length(value: &Value) -> Result<f64, RuntimeError> {
    Ok(integer(value)?.clamp(0.0, 9007199254740991.0))
}

pub(crate) fn append(
    result: &mut JsString,
    part: &JsString,
    limit: usize,
) -> Result<(), RuntimeError> {
    if result
        .byte_len()
        .checked_add(part.byte_len())
        .is_none_or(|bytes| bytes > limit)
    {
        return Err(RuntimeError::StringLimit { limit });
    }
    result.push_str(part);
    Ok(())
}

pub(crate) fn from_codes(
    args: &[Value],
    points: bool,
    limit: usize,
) -> Result<Value, RuntimeError> {
    let mut result = JsString::default();
    for value in args {
        let number = primitive::number(value)?;
        let code = if points {
            if !(0.0..=0x10ffff as f64).contains(&number) || number.fract() != 0.0 {
                return Err(RuntimeError::RangeError("invalid String code point".into()));
            }
            number as u32
        } else if !number.is_finite() {
            0
        } else {
            number.trunc().rem_euclid(65536.0) as u32
        };
        let mut part = JsString::default();
        part.push_code_point(code);
        append(&mut result, &part, limit)?;
    }
    Ok(Value::String(result))
}

fn uri_append(output: &mut Vec<u16>, units: &[u16], limit: usize) -> Result<(), UriCodingError> {
    let length = output
        .len()
        .checked_add(units.len())
        .ok_or(UriCodingError::StringLimit { limit })?;
    if length
        .checked_mul(std::mem::size_of::<u16>())
        .is_none_or(|bytes| bytes > limit)
    {
        return Err(UriCodingError::StringLimit { limit });
    }
    output.extend_from_slice(units);
    Ok(())
}

fn uri_hex(unit: u16) -> Option<u8> {
    match unit {
        0x30..=0x39 => Some((unit - 0x30) as u8),
        0x61..=0x66 => Some((unit - 0x61 + 10) as u8),
        0x41..=0x46 => Some((unit - 0x41 + 10) as u8),
        _ => None,
    }
}

fn uri_percent_byte(units: &[u16], index: usize) -> Result<u8, UriCodingError> {
    if units.get(index) != Some(&u16::from(b'%')) {
        return Err(UriCodingError::Malformed);
    }
    let high = units
        .get(index + 1)
        .and_then(|unit| uri_hex(*unit))
        .ok_or(UriCodingError::Malformed)?;
    let low = units
        .get(index + 2)
        .and_then(|unit| uri_hex(*unit))
        .ok_or(UriCodingError::Malformed)?;
    Ok((high << 4) | low)
}

fn uri_unescaped(code_point: u32, component: bool) -> bool {
    matches!(code_point, 0x41..=0x5a | 0x61..=0x7a | 0x30..=0x39)
        || matches!(
            code_point,
            0x2d | 0x5f | 0x2e | 0x21 | 0x7e | 0x2a | 0x27 | 0x28 | 0x29
        )
        || (!component
            && matches!(
                code_point,
                0x3b | 0x2f | 0x3f | 0x3a | 0x40 | 0x26 | 0x3d | 0x2b | 0x24 | 0x2c | 0x23
            ))
}

fn uri_reserved(byte: u8) -> bool {
    matches!(
        byte,
        b';' | b'/' | b'?' | b':' | b'@' | b'&' | b'=' | b'+' | b'$' | b',' | b'#'
    )
}

/// ECMA-262 §19.2.6.6 / §19.2.6.7 Encode.
pub(crate) fn encode_uri(
    string: &JsString,
    component: bool,
    limit: usize,
) -> Result<JsString, UriCodingError> {
    let units = string.as_code_units();
    let mut output = Vec::with_capacity(units.len());
    let mut index = 0;
    while let Some(&unit) = units.get(index) {
        let (code_point, width) = match unit {
            0xd800..=0xdbff => {
                let Some(&low) = units.get(index + 1) else {
                    return Err(UriCodingError::Malformed);
                };
                if !(0xdc00..=0xdfff).contains(&low) {
                    return Err(UriCodingError::Malformed);
                }
                (
                    0x10000 + ((u32::from(unit) - 0xd800) << 10) + (u32::from(low) - 0xdc00),
                    2,
                )
            }
            0xdc00..=0xdfff => return Err(UriCodingError::Malformed),
            _ => (u32::from(unit), 1),
        };
        if uri_unescaped(code_point, component) {
            uri_append(&mut output, &units[index..index + width], limit)?;
        } else {
            let scalar = char::from_u32(code_point).expect("URI code point is a Unicode scalar");
            let mut buffer = [0; 4];
            for byte in scalar.encode_utf8(&mut buffer).bytes() {
                const HEX: &[u8; 16] = b"0123456789ABCDEF";
                uri_append(
                    &mut output,
                    &[
                        u16::from(b'%'),
                        u16::from(HEX[(byte >> 4) as usize]),
                        u16::from(HEX[(byte & 0x0f) as usize]),
                    ],
                    limit,
                )?;
            }
        }
        index += width;
    }
    Ok(JsString::from_code_units(output))
}

/// ECMA-262 §19.2.6.4 / §19.2.6.5 Decode.  This deliberately performs
/// strict UTF-8 validation instead of accepting Rust's replacement-character
/// conversion, because malformed percent octets must throw `URIError`.
pub(crate) fn decode_uri(
    string: &JsString,
    component: bool,
    limit: usize,
) -> Result<JsString, UriCodingError> {
    let units = string.as_code_units();
    let mut output = Vec::with_capacity(units.len());
    let mut index = 0;
    while let Some(&unit) = units.get(index) {
        if unit != u16::from(b'%') {
            uri_append(&mut output, &[unit], limit)?;
            index += 1;
            continue;
        }

        let start = index;
        let first = uri_percent_byte(units, start)?;
        if first < 0x80 {
            index += 3;
            if !component && uri_reserved(first) {
                uri_append(&mut output, &units[start..index], limit)?;
            } else {
                uri_append(&mut output, &[u16::from(first)], limit)?;
            }
            continue;
        }

        let octets = match first {
            0xc2..=0xdf => 2,
            0xe0..=0xef => 3,
            0xf0..=0xf4 => 4,
            _ => return Err(UriCodingError::Malformed),
        };
        let mut bytes = [0; 4];
        bytes[0] = first;
        for (offset, slot) in bytes.iter_mut().enumerate().take(octets).skip(1) {
            let byte = uri_percent_byte(units, start + offset * 3)?;
            if !(0x80..=0xbf).contains(&byte) {
                return Err(UriCodingError::Malformed);
            }
            *slot = byte;
        }
        let decoded =
            std::str::from_utf8(&bytes[..octets]).map_err(|_| UriCodingError::Malformed)?;
        let mut encoded = [0; 2];
        let utf16_width = decoded
            .chars()
            .next()
            .expect("a valid non-empty UTF-8 sequence has a character")
            .encode_utf16(&mut encoded)
            .len();
        uri_append(&mut output, &encoded[..utf16_width], limit)?;
        index = start + octets * 3;
    }
    Ok(JsString::from_code_units(output))
}

pub(crate) fn substitution(
    string: &JsString,
    matched: &JsString,
    position: usize,
    template: &JsString,
    limit: usize,
) -> Result<JsString, RuntimeError> {
    let units = template.as_code_units();
    let mut index = 0;
    let mut result = JsString::default();
    while index < units.len() {
        let expansion = if units[index] == u16::from(b'$') {
            match units.get(index + 1).copied() {
                Some(0x24) => Some(&units[index..index + 1]),
                Some(0x26) => Some(matched.as_code_units()),
                Some(0x60) => Some(&string.as_code_units()[..position]),
                Some(0x27) => Some(&string.as_code_units()[position + matched.len()..]),
                _ => None, // No captures for a String search; $n and $<name> stay literal.
            }
        } else {
            None
        };
        let part = expansion.unwrap_or(&units[index..index + 1]);
        append(
            &mut result,
            &JsString::from_code_units(part.to_vec()),
            limit,
        )?;
        index += if expansion.is_some() { 2 } else { 1 };
    }
    Ok(result)
}

pub(crate) fn string_method(
    method: StringMethod,
    receiver: &Value,
    args: &[Value],
    limit: usize,
) -> Result<Value, RuntimeError> {
    use StringMethod::*;
    if matches!(receiver, Value::Undefined | Value::Null) {
        return Err(RuntimeError::TypeError(
            "String method requires a non-null receiver".into(),
        ));
    }
    if matches!(method, ToString | ValueOf) && !matches!(receiver, Value::String(_)) {
        return Err(RuntimeError::TypeError(
            "String value method requires a String receiver".into(),
        ));
    }
    let mut string = primitive::string(receiver)?;
    let units = string.as_code_units();
    let len = units.len();
    let first = argument(args, 0);
    let second = argument(args, 1);
    Ok(match method {
        At | CharAt | CharCodeAt | CodePointAt => {
            let mut index = integer(first)?;
            if method == At && index < 0.0 {
                index += len as f64;
            }
            if index < 0.0 || index >= len as f64 {
                match method {
                    CharAt => Value::String(JsString::default()),
                    CharCodeAt => Value::Number(f64::NAN),
                    _ => Value::Undefined,
                }
            } else {
                let index = index as usize;
                let unit = units[index];
                if method == CharCodeAt || method == CodePointAt {
                    let point = if method == CodePointAt
                        && (0xd800..=0xdbff).contains(&unit)
                        && units
                            .get(index + 1)
                            .is_some_and(|unit| (0xdc00..=0xdfff).contains(unit))
                    {
                        (u32::from(unit) - 0xd800) * 0x400 + u32::from(units[index + 1]) - 0xdc00
                            + 0x10000
                    } else {
                        u32::from(unit)
                    };
                    Value::Number(f64::from(point))
                } else {
                    Value::String(JsString::from_code_units(vec![unit]))
                }
            }
        }
        Slice | Substring => {
            let start = integer(first)?;
            let end = if matches!(second, Value::Undefined) {
                len as f64
            } else {
                integer(second)?
            };
            let bound = |index: f64| {
                let index = if method == Slice && index < 0.0 {
                    len as f64 + index
                } else {
                    index
                };
                index.clamp(0.0, len as f64) as usize
            };
            let (start, end) = (bound(start), bound(end));
            let (start, end) = if method == Substring {
                (start.min(end), start.max(end))
            } else {
                (start, end.max(start))
            };
            Value::String(JsString::from_code_units(units[start..end].to_vec()))
        }
        IndexOf | LastIndexOf | Includes | StartsWith | EndsWith => {
            // Observable conversions and IsRegExp have run in VM dispatch.
            let search = primitive::string(first)?;
            let needle = search.as_code_units();
            let position = if method == LastIndexOf {
                let number = primitive::number(second)?;
                if number.is_nan() {
                    f64::INFINITY
                } else {
                    number.trunc()
                }
            } else if method == EndsWith && matches!(second, Value::Undefined) {
                len as f64
            } else {
                integer(second)?
            };
            let position = position.clamp(0.0, len as f64) as usize;
            match method {
                StartsWith => Value::Bool(units[position..].starts_with(needle)),
                EndsWith => Value::Bool(units[..position].ends_with(needle)),
                _ => {
                    let index = if needle.len() > len {
                        None
                    } else if method == LastIndexOf {
                        (0..=position.min(len - needle.len()))
                            .rev()
                            .find(|&index| units[index..].starts_with(needle))
                    } else {
                        (position..=len - needle.len())
                            .find(|&index| units[index..].starts_with(needle))
                    };
                    if method == Includes {
                        Value::Bool(index.is_some())
                    } else {
                        Value::Number(index.map_or(-1.0, |index| index as f64))
                    }
                }
            }
        }
        Concat => {
            for arg in args {
                append(&mut string, &primitive::string(arg)?, limit)?;
            }
            Value::String(string)
        }
        IsWellFormed => {
            Value::Bool(char::decode_utf16(units.iter().copied()).all(|unit| unit.is_ok()))
        }
        ToWellFormed => {
            let mut result = JsString::default();
            for scalar in char::decode_utf16(units.iter().copied()) {
                result.push_code_point(scalar.unwrap_or('\u{fffd}') as u32);
            }
            Value::String(result)
        }
        ToString | ValueOf => Value::String(string),
        PadStart | PadEnd => {
            let target = length(first)?;
            debug_assert!(
                target > len as f64,
                "short padding requests return before filler conversion in VM dispatch"
            );
            let fill = if matches!(second, Value::Undefined) {
                JsString::from(" ")
            } else {
                primitive::string(second)?
            };
            if fill.is_empty() {
                return Ok(Value::String(string));
            }
            if target > (limit / 2) as f64 {
                return Err(RuntimeError::StringLimit { limit });
            }
            let padding: Vec<_> = fill
                .as_code_units()
                .iter()
                .copied()
                .cycle()
                .take(target as usize - len)
                .collect();
            let padding = JsString::from_code_units(padding);
            if method == PadStart {
                let mut result = padding;
                result.push_str(&string);
                Value::String(result)
            } else {
                string.push_str(&padding);
                Value::String(string)
            }
        }
        Repeat => {
            let count = integer(first)?;
            if count < 0.0 || count == f64::INFINITY {
                return Err(RuntimeError::RangeError(
                    "invalid String repeat count".into(),
                ));
            }
            if count == 0.0 || string.is_empty() {
                return Ok(Value::String(JsString::default()));
            }
            if count > (limit / string.byte_len()) as f64 {
                return Err(RuntimeError::StringLimit { limit });
            }
            Value::String(JsString::from_code_units(units.repeat(count as usize)))
        }
        Trim | TrimStart | TrimEnd => {
            let space =
                |unit: &u16| char::from_u32(u32::from(*unit)).is_some_and(primitive::whitespace);
            let start = if method == TrimEnd {
                0
            } else {
                units.iter().take_while(|unit| space(unit)).count()
            };
            let end = if method == TrimStart {
                len
            } else {
                len - units[start..]
                    .iter()
                    .rev()
                    .take_while(|unit| space(unit))
                    .count()
            };
            Value::String(JsString::from_code_units(units[start..end].to_vec()))
        }
        Normalize | ToLowerCase | ToUpperCase => {
            let form = match method {
                ToLowerCase => "lower".into(),
                ToUpperCase => "upper".into(),
                _ => {
                    let form = if matches!(first, Value::Undefined) {
                        JsString::from("NFC")
                    } else {
                        primitive::string(first)?
                    };
                    if !["NFC", "NFD", "NFKC", "NFKD"]
                        .iter()
                        .any(|name| form == *name)
                    {
                        return Err(RuntimeError::RangeError(
                            "invalid String normalization form".into(),
                        ));
                    }
                    form.to_utf8().expect("validated ASCII normalization form")
                }
            };
            Value::String(unicode_transform(&string, &form, limit)?)
        }
        Substr => {
            let start = integer(first)?;
            let start = (if start < 0.0 {
                len as f64 + start
            } else {
                start
            })
            .clamp(0.0, len as f64) as usize;
            let count = if matches!(second, Value::Undefined) {
                len as f64
            } else {
                integer(second)?
            };
            let count = count.clamp(0.0, (len - start) as f64) as usize;
            Value::String(JsString::from_code_units(
                units[start..start + count].to_vec(),
            ))
        }
        Html { tag, attribute } => {
            let mut result: JsString = format!("<{tag}").into();
            if !attribute.is_empty() {
                let value = primitive::string(first)?;
                append(&mut result, &format!(" {attribute}=\"").into(), limit)?;
                for &unit in value.as_code_units() {
                    let escaped = if unit == u16::from(b'"') {
                        "&quot;".into()
                    } else {
                        JsString::from_code_units(vec![unit])
                    };
                    append(&mut result, &escaped, limit)?;
                }
                append(&mut result, &"\"".into(), limit)?;
            }
            append(&mut result, &">".into(), limit)?;
            append(&mut result, &string, limit)?;
            append(&mut result, &format!("</{tag}>").into(), limit)?;
            Value::String(result)
        }
    })
}

fn unicode_transform(
    string: &JsString,
    form: &str,
    limit: usize,
) -> Result<JsString, RuntimeError> {
    let mut result = JsString::default();
    let mut run = String::new();
    for scalar in char::decode_utf16(string.as_code_units().iter().copied()) {
        match scalar {
            Ok(scalar) => run.push(scalar),
            Err(error) => {
                transform_run(&run, form, &mut result, limit)?;
                run.clear();
                append(
                    &mut result,
                    &JsString::from_code_units(vec![error.unpaired_surrogate()]),
                    limit,
                )?;
            }
        }
    }
    transform_run(&run, form, &mut result, limit)?;
    Ok(result)
}

fn transform_run(
    run: &str,
    form: &str,
    result: &mut JsString,
    limit: usize,
) -> Result<(), RuntimeError> {
    // Case conversion uses the complete run for contextual mappings
    // (notably final sigma); normalization streams expanded code points.
    let case = match form {
        "lower" => run.to_lowercase(),
        "upper" => run.to_uppercase(),
        _ => String::new(),
    };
    let scalars: Box<dyn Iterator<Item = char> + '_> = match form {
        "NFC" => Box::new(run.nfc()),
        "NFD" => Box::new(run.nfd()),
        "NFKC" => Box::new(run.nfkc()),
        "NFKD" => Box::new(run.nfkd()),
        _ => Box::new(case.chars()),
    };
    for scalar in scalars {
        let mut part = JsString::default();
        part.push_code_point(scalar as u32);
        append(result, &part, limit)?;
    }
    Ok(())
}
