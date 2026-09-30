// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

//! Lazy installation of the foundational realm intrinsics.

use crate::heap::ArrayIteratorKind;
use crate::native::{self, NativeFunction};
use crate::{JsString, JsSymbol, ObjectId, PropertyName, Value};

use super::{RuntimeError, Vm};

impl Vm {
    pub(super) fn string_intrinsics(&mut self) -> Result<(ObjectId, ObjectId), RuntimeError> {
        if let Some(intrinsics) = self.string_intrinsics {
            return Ok(intrinsics);
        }
        let object_prototype = self.object_prototype;
        let function_prototype = self.with_roots(|heap| {
            heap.alloc_native_function(NativeFunction::Empty, "", object_prototype)
        })?;
        let constructor = self.with_roots(|heap| {
            heap.alloc_native_function(NativeFunction::String, "String", function_prototype)
        })?;
        let root = self.heap.root(constructor)?;
        let result = (|| {
            let prototype = self.with_roots(|heap| {
                heap.alloc_string(JsString::default(), Some(object_prototype))
            })?;
            self.define_data(
                constructor,
                "prototype",
                Value::Object(prototype),
                false,
                false,
                false,
            )?;
            self.define_data(
                prototype,
                "constructor",
                Value::Object(constructor),
                true,
                false,
                true,
            )?;
            self.define_data(
                constructor,
                "name",
                Value::String("String".into()),
                false,
                false,
                true,
            )?;
            self.define_data(
                constructor,
                "length",
                Value::Number(1.0),
                false,
                false,
                true,
            )?;
            self.define_data(
                function_prototype,
                "length",
                Value::Number(0.0),
                false,
                false,
                true,
            )?;
            self.define_data(
                function_prototype,
                "name",
                Value::String(JsString::default()),
                false,
                false,
                true,
            )?;
            self.install_native(
                function_prototype,
                function_prototype,
                "call",
                1,
                NativeFunction::Call,
            )?;
            self.install_native(
                function_prototype,
                function_prototype,
                "apply",
                2,
                NativeFunction::Apply,
            )?;
            self.install_native(
                function_prototype,
                function_prototype,
                "bind",
                1,
                NativeFunction::Bind,
            )?;
            self.install_symbol_native(
                function_prototype,
                function_prototype,
                "hasInstance",
                1,
                NativeFunction::HasInstance,
            )?;
            // The decorator-metadata proposal: a class nothing decorated has
            // `null` metadata, inherited from here.
            self.define_data(
                function_prototype,
                JsSymbol::well_known("metadata"),
                Value::Null,
                false,
                false,
                false,
            )?;
            self.install_native(
                function_prototype,
                function_prototype,
                "toString",
                0,
                NativeFunction::FunctionToString,
            )?;
            self.install_native(
                constructor,
                function_prototype,
                "fromCharCode",
                1,
                NativeFunction::FromCharCode,
            )?;
            self.install_native(
                constructor,
                function_prototype,
                "fromCodePoint",
                1,
                NativeFunction::FromCodePoint,
            )?;
            self.install_native(
                constructor,
                function_prototype,
                "raw",
                1,
                NativeFunction::Raw,
            )?;
            self.install_native(
                prototype,
                function_prototype,
                "split",
                2,
                NativeFunction::Split,
            )?;
            self.install_native(
                prototype,
                function_prototype,
                "replace",
                2,
                NativeFunction::Replace,
            )?;
            self.install_native(
                prototype,
                function_prototype,
                "replaceAll",
                2,
                NativeFunction::ReplaceAll,
            )?;
            for &(name, length, method) in native::STRING_METHODS {
                self.install_native(
                    prototype,
                    function_prototype,
                    name,
                    length,
                    NativeFunction::StringMethod(method),
                )?;
            }
            for (name, length, method) in [
                ("toLocaleLowerCase", 0, NativeFunction::ToLocaleLowerCase),
                ("toLocaleUpperCase", 0, NativeFunction::ToLocaleUpperCase),
                ("localeCompare", 1, NativeFunction::LocaleCompare),
            ] {
                self.install_native(prototype, function_prototype, name, length, method)?;
            }
            for (alias, original) in [("trimLeft", "trimStart"), ("trimRight", "trimEnd")] {
                let function = self.heap.get(prototype, original)?;
                self.define_data(prototype, alias, function, true, false, true)?;
            }
            self.install_symbol_native(
                prototype,
                function_prototype,
                "iterator",
                0,
                NativeFunction::StringIterator,
            )?;
            for (name, method) in [
                ("match", native::PatternMethod::Match),
                ("matchAll", native::PatternMethod::MatchAll),
                ("search", native::PatternMethod::Search),
            ] {
                self.install_native(
                    prototype,
                    function_prototype,
                    name,
                    1,
                    NativeFunction::Pattern(method),
                )?;
            }
            self.install_native(
                object_prototype,
                function_prototype,
                "toString",
                0,
                NativeFunction::ObjectToString,
            )?;
            self.install_native(
                object_prototype,
                function_prototype,
                "toLocaleString",
                0,
                NativeFunction::ObjectToLocaleString,
            )?;
            self.install_native(
                object_prototype,
                function_prototype,
                "valueOf",
                0,
                NativeFunction::ObjectValueOf,
            )?;
            self.install_native(
                object_prototype,
                function_prototype,
                "isPrototypeOf",
                1,
                NativeFunction::ObjectIsPrototypeOf,
            )?;
            for (name, length, function) in [
                (
                    "__defineGetter__",
                    2,
                    NativeFunction::ObjectDefineAccessor { getter: true },
                ),
                (
                    "__defineSetter__",
                    2,
                    NativeFunction::ObjectDefineAccessor { getter: false },
                ),
                (
                    "__lookupGetter__",
                    1,
                    NativeFunction::ObjectLookupAccessor { getter: true },
                ),
                (
                    "__lookupSetter__",
                    1,
                    NativeFunction::ObjectLookupAccessor { getter: false },
                ),
            ] {
                self.install_native(object_prototype, function_prototype, name, length, function)?;
            }
            self.install_native_accessor(
                object_prototype,
                function_prototype,
                "__proto__",
                NativeFunction::ObjectPrototypeGetter,
                NativeFunction::ObjectPrototypeSetter,
            )?;
            self.install_native(
                self.array_prototype,
                function_prototype,
                "at",
                1,
                NativeFunction::ArrayAt,
            )?;
            self.install_native(
                self.array_prototype,
                function_prototype,
                "fill",
                1,
                NativeFunction::ArrayFill,
            )?;
            for (name, length, function) in [
                ("copyWithin", 2, NativeFunction::ArrayCopyWithin),
                ("flat", 0, NativeFunction::ArrayFlat),
                ("flatMap", 1, NativeFunction::ArrayFlatMap),
                ("toReversed", 0, NativeFunction::ArrayToReversed),
                ("toSorted", 1, NativeFunction::ArrayToSorted),
                ("toSpliced", 2, NativeFunction::ArrayToSpliced),
                ("with", 2, NativeFunction::ArrayWith),
            ] {
                self.install_native(
                    self.array_prototype,
                    function_prototype,
                    name,
                    length,
                    function,
                )?;
            }
            for (name, kind) in [
                ("entries", ArrayIteratorKind::Entries),
                ("keys", ArrayIteratorKind::Keys),
                ("values", ArrayIteratorKind::Values),
            ] {
                self.install_native(
                    self.array_prototype,
                    function_prototype,
                    name,
                    0,
                    NativeFunction::ArrayIterator(kind),
                )?;
            }
            // Array.prototype[Symbol.iterator] is the same function object
            // as Array.prototype.values.
            let values = self.heap.get(self.array_prototype, "values")?;
            self.define_data(
                self.array_prototype,
                JsSymbol::well_known("iterator"),
                values,
                true,
                false,
                true,
            )?;
            self.install_native(
                self.array_prototype,
                function_prototype,
                "toString",
                0,
                NativeFunction::ArrayToString,
            )?;
            self.install_native(
                self.array_prototype,
                function_prototype,
                "toLocaleString",
                0,
                NativeFunction::ArrayToLocaleString,
            )?;
            self.install_native(
                self.array_prototype,
                function_prototype,
                "concat",
                1,
                NativeFunction::ArrayConcat,
            )?;
            self.install_native(
                self.array_prototype,
                function_prototype,
                "join",
                1,
                NativeFunction::ArrayJoin,
            )?;
            self.install_native(
                self.array_prototype,
                function_prototype,
                "forEach",
                1,
                NativeFunction::ArrayForEach,
            )?;
            self.install_native(
                self.array_prototype,
                function_prototype,
                "filter",
                1,
                NativeFunction::ArrayFilter,
            )?;
            self.install_native(
                self.array_prototype,
                function_prototype,
                "map",
                1,
                NativeFunction::ArrayMap,
            )?;
            for (name, function) in [
                ("find", NativeFunction::ArrayFind),
                ("findIndex", NativeFunction::ArrayFindIndex),
                ("findLast", NativeFunction::ArrayFindLast),
                ("findLastIndex", NativeFunction::ArrayFindLastIndex),
            ] {
                self.install_native(self.array_prototype, function_prototype, name, 1, function)?;
            }
            self.install_native(
                self.array_prototype,
                function_prototype,
                "every",
                1,
                NativeFunction::ArrayEvery,
            )?;
            self.install_native(
                self.array_prototype,
                function_prototype,
                "some",
                1,
                NativeFunction::ArraySome,
            )?;
            self.install_native(
                self.array_prototype,
                function_prototype,
                "includes",
                1,
                NativeFunction::ArrayIncludes,
            )?;
            self.install_native(
                self.array_prototype,
                function_prototype,
                "reduce",
                1,
                NativeFunction::ArrayReduce,
            )?;
            self.install_native(
                self.array_prototype,
                function_prototype,
                "reduceRight",
                1,
                NativeFunction::ArrayReduceRight,
            )?;
            self.install_native(
                self.array_prototype,
                function_prototype,
                "push",
                1,
                NativeFunction::ArrayPush,
            )?;
            for (name, length, function) in [
                ("pop", 0, NativeFunction::ArrayPop),
                ("shift", 0, NativeFunction::ArrayShift),
                ("unshift", 1, NativeFunction::ArrayUnshift),
                ("reverse", 0, NativeFunction::ArrayReverse),
            ] {
                self.install_native(
                    self.array_prototype,
                    function_prototype,
                    name,
                    length,
                    function,
                )?;
            }
            self.install_native(
                self.array_prototype,
                function_prototype,
                "indexOf",
                1,
                NativeFunction::ArrayIndexOf,
            )?;
            self.install_native(
                self.array_prototype,
                function_prototype,
                "lastIndexOf",
                1,
                NativeFunction::ArrayLastIndexOf,
            )?;
            self.install_native(
                self.array_prototype,
                function_prototype,
                "slice",
                2,
                NativeFunction::ArraySlice,
            )?;
            self.install_native(
                self.array_prototype,
                function_prototype,
                "splice",
                2,
                NativeFunction::ArraySplice,
            )?;
            self.install_native(
                self.array_prototype,
                function_prototype,
                "sort",
                1,
                NativeFunction::ArraySort,
            )?;
            self.install_array_unscopables()?;
            Ok((constructor, prototype))
        })();
        match result {
            Ok(intrinsics) => {
                self.string_intrinsics = Some(intrinsics);
                Ok(intrinsics)
            }
            Err(error) => {
                // The only pre-existing owners modified by bootstrap are
                // these two empty prototypes. Roll back published edges too.
                for (owner, key) in [
                    (object_prototype, PropertyName::from("toString")),
                    (object_prototype, "toLocaleString".into()),
                    (object_prototype, "valueOf".into()),
                    (object_prototype, "isPrototypeOf".into()),
                    (object_prototype, "__defineGetter__".into()),
                    (object_prototype, "__defineSetter__".into()),
                    (object_prototype, "__lookupGetter__".into()),
                    (object_prototype, "__lookupSetter__".into()),
                    (object_prototype, "__proto__".into()),
                    (object_prototype, "hasOwnProperty".into()),
                    (self.array_prototype, "at".into()),
                    (self.array_prototype, "fill".into()),
                    (self.array_prototype, "copyWithin".into()),
                    (self.array_prototype, "flat".into()),
                    (self.array_prototype, "flatMap".into()),
                    (self.array_prototype, "toReversed".into()),
                    (self.array_prototype, "toSorted".into()),
                    (self.array_prototype, "toSpliced".into()),
                    (self.array_prototype, "with".into()),
                    (
                        self.array_prototype,
                        PropertyName::from(JsSymbol::well_known("unscopables")),
                    ),
                    (self.array_prototype, "entries".into()),
                    (self.array_prototype, "keys".into()),
                    (self.array_prototype, "values".into()),
                    (self.array_prototype, "toString".into()),
                    (self.array_prototype, "concat".into()),
                    (self.array_prototype, "join".into()),
                    (self.array_prototype, "forEach".into()),
                    (self.array_prototype, "filter".into()),
                    (self.array_prototype, "map".into()),
                    (self.array_prototype, "find".into()),
                    (self.array_prototype, "findIndex".into()),
                    (self.array_prototype, "findLast".into()),
                    (self.array_prototype, "findLastIndex".into()),
                    (self.array_prototype, "every".into()),
                    (self.array_prototype, "some".into()),
                    (self.array_prototype, "includes".into()),
                    (self.array_prototype, "reduce".into()),
                    (self.array_prototype, "reduceRight".into()),
                    (self.array_prototype, "push".into()),
                    (self.array_prototype, "indexOf".into()),
                    (self.array_prototype, "lastIndexOf".into()),
                    (self.array_prototype, "slice".into()),
                    (self.array_prototype, "splice".into()),
                    (self.array_prototype, "sort".into()),
                    (
                        self.array_prototype,
                        JsSymbol::well_known("iterator").into(),
                    ),
                ] {
                    self.heap.delete(owner, key)?;
                }
                self.heap.unroot(root)?;
                Err(error)
            }
        }
    }
}
