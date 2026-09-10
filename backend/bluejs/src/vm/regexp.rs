// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

use super::*;
use crate::native::RegExpMethod;
use crate::regexp::{advance, RegExp};
use std::rc::Rc;

impl Vm {
    pub(super) fn regexp_global(&mut self) -> Result<Value, RuntimeError> {
        if let Some(&id) = self.globals.get("RegExp") {
            return Ok(Value::Object(id));
        }
        let string = self.string_intrinsics()?.0;
        let function_prototype = self.heap.prototype(string)?.unwrap();
        let constructor = self.with_roots(|heap| {
            heap.alloc_native_function(NativeFunction::RegExp, "RegExp", function_prototype)
        })?;
        let root = self.heap.root(constructor)?;
        let result = (|| {
            let object_prototype = self.object_prototype;
            let prototype = self.with_roots(|heap| heap.alloc_object(Some(object_prototype)))?;
            self.define_data(
                constructor,
                "prototype",
                Value::Object(prototype),
                false,
                false,
                false,
            )?;
            self.define_data(
                constructor,
                "name",
                Value::String("RegExp".into()),
                false,
                false,
                true,
            )?;
            self.define_data(
                constructor,
                "length",
                Value::Number(2.0),
                false,
                false,
                true,
            )?;
            self.define_data(
                prototype,
                "constructor",
                Value::Object(constructor),
                true,
                false,
                true,
            )?;
            self.install_native(
                constructor,
                function_prototype,
                "escape",
                1,
                NativeFunction::RegExpEscape,
            )?;
            for (name, method) in [
                ("exec", RegExpMethod::Exec),
                ("test", RegExpMethod::Test),
                ("toString", RegExpMethod::ToString),
            ] {
                self.install_native(
                    prototype,
                    function_prototype,
                    name,
                    if method == RegExpMethod::ToString {
                        0
                    } else {
                        1
                    },
                    NativeFunction::RegExpMethod(method),
                )?;
            }
            for (name, length, method) in [
                ("match", 1, RegExpMethod::Match),
                ("matchAll", 1, RegExpMethod::MatchAll),
                ("search", 1, RegExpMethod::Search),
                ("replace", 2, RegExpMethod::Replace),
                ("split", 2, RegExpMethod::Split),
            ] {
                self.install_symbol_native(
                    prototype,
                    function_prototype,
                    name,
                    length,
                    NativeFunction::RegExpMethod(method),
                )?;
            }
            for name in [
                "source",
                "flags",
                "global",
                "ignoreCase",
                "multiline",
                "dotAll",
                "unicode",
                "unicodeSets",
                "sticky",
                "hasIndices",
            ] {
                self.install_getter(
                    prototype,
                    function_prototype,
                    name.into(),
                    &format!("get {name}"),
                    NativeFunction::RegExpGetter(name),
                )?;
            }
            self.install_getter(
                constructor,
                function_prototype,
                JsSymbol::well_known("species").into(),
                "get [Symbol.species]",
                NativeFunction::RegExpGetter("species"),
            )?;
            Ok(Value::Object(constructor))
        })();
        if result.is_err() {
            self.heap.unroot(root)?;
        } else {
            self.globals.insert("RegExp".into(), constructor);
        }
        result
    }

    pub(super) fn regexp_escape(&mut self, value: &Value) -> Result<Value, RuntimeError> {
        let Value::String(string) = value else {
            return Err(RuntimeError::TypeError(
                "RegExp.escape requires a String".into(),
            ));
        };
        let mut result = JsString::default();
        for (index, scalar) in
            char::decode_utf16(string.as_code_units().iter().copied()).enumerate()
        {
            self.charge_step()?;
            let point = scalar.map_or_else(|e| u32::from(e.unpaired_surrogate()), |c| c as u32);
            let part = crate::regexp::escape_code_point(point, index == 0);
            native::append(&mut result, &part, self.config.max_string_bytes)?;
        }
        Ok(Value::String(result))
    }

    pub(super) fn install_getter(
        &mut self,
        owner: ObjectId,
        prototype: ObjectId,
        key: PropertyName,
        name: &str,
        native: NativeFunction,
    ) -> Result<(), RuntimeError> {
        let id = self.with_roots(|heap| heap.alloc_native_function(native, name, prototype))?;
        self.stack.push(Value::Object(id));
        self.define_data(id, "name", Value::String(name.into()), false, false, true)?;
        self.define_data(id, "length", Value::Number(0.0), false, false, true)?;
        self.with_roots(|heap| {
            heap.define_own_property(
                owner,
                key,
                PropertyDescriptor {
                    get: Some(Value::Object(id)),
                    set: Some(Value::Undefined),
                    enumerable: Some(false),
                    configurable: Some(true),
                    ..Default::default()
                },
            )
        })?;
        self.stack.pop();
        Ok(())
    }

    pub(super) fn regexp_create(
        &mut self,
        pattern: &Value,
        flags: &Value,
    ) -> Result<Value, RuntimeError> {
        let existing = if let Value::Object(id) = pattern {
            self.heap.regexp(*id)?
        } else {
            None
        };
        let (source, flags) = if let Some(regexp) = existing {
            (
                regexp.source.clone(),
                if *flags == Value::Undefined {
                    regexp.flags.clone().into()
                } else {
                    self.coerce_string(flags)?
                },
            )
        } else if self.is_regexp(pattern)? {
            let source = self.get_property(pattern, &"source".into())?;
            self.stack.push(source.clone());
            let flags = if *flags == Value::Undefined {
                self.get_property(pattern, &"flags".into())?
            } else {
                flags.clone()
            };
            (self.coerce_string(&source)?, self.coerce_string(&flags)?)
        } else {
            (
                if *pattern == Value::Undefined {
                    JsString::default()
                } else {
                    self.coerce_string(pattern)?
                },
                if *flags == Value::Undefined {
                    JsString::default()
                } else {
                    self.coerce_string(flags)?
                },
            )
        };
        self.check_string(&Value::String(source.clone()))?;
        let regexp = Rc::new(RegExp::compile_with_timeout(
            source,
            &flags,
            self.config.regex_timeout,
        )?);
        let constructor = self.regexp_global()?;
        let prototype = self
            .get_property(&constructor, &"prototype".into())?
            .object_id()
            .unwrap();
        let prototype = if self.new_target != Value::Undefined {
            self.constructor_prototype(prototype)?
        } else {
            prototype
        };
        let object = self.with_roots(|heap| heap.alloc_regexp(regexp, prototype))?;
        self.stack.push(Value::Object(object));
        self.define_data(object, "lastIndex", Value::Number(0.0), true, false, false)?;
        self.stack.pop();
        Ok(Value::Object(object))
    }

    pub(super) fn regexp_getter(
        &mut self,
        name: &str,
        receiver: &Value,
    ) -> Result<Value, RuntimeError> {
        if name == "species" {
            return Ok(receiver.clone());
        }
        let Value::Object(id) = receiver else {
            return Err(RuntimeError::TypeError(
                "RegExp getter requires an object".into(),
            ));
        };
        if name == "flags" {
            let mut flags = String::new();
            for (property, flag) in [
                ("hasIndices", 'd'),
                ("global", 'g'),
                ("ignoreCase", 'i'),
                ("multiline", 'm'),
                ("dotAll", 's'),
                ("unicode", 'u'),
                ("unicodeSets", 'v'),
                ("sticky", 'y'),
            ] {
                if primitive::truthy(&self.get_property(receiver, &property.into())?) {
                    flags.push(flag);
                }
            }
            return Ok(Value::String(flags.into()));
        }
        let Some(regexp) = self.heap.regexp(*id)? else {
            let constructor = self.globals["RegExp"];
            if self.heap.get(constructor, "prototype")? == *receiver {
                return Ok(if name == "source" {
                    Value::String("(?:)".into())
                } else {
                    Value::Undefined
                });
            }
            return Err(RuntimeError::TypeError(
                "RegExp getter requires a RegExp".into(),
            ));
        };
        if name == "source" {
            if regexp.source.is_empty() {
                return Ok(Value::String("(?:)".into()));
            }
            let mut source = JsString::default();
            let mut escaped = false;
            for &unit in regexp.source.as_code_units() {
                let part = match unit {
                    0x2f if !escaped => "\\/".into(),
                    0x0a => "\\n".into(),
                    0x0d => "\\r".into(),
                    0x2028 => "\\u2028".into(),
                    0x2029 => "\\u2029".into(),
                    _ => JsString::from_code_units(vec![unit]),
                };
                native::append(&mut source, &part, self.config.max_string_bytes)?;
                escaped = unit == 0x5c && !escaped;
            }
            return Ok(Value::String(source));
        }
        let flag = match name {
            "global" => 'g',
            "ignoreCase" => 'i',
            "multiline" => 'm',
            "dotAll" => 's',
            "unicode" => 'u',
            "unicodeSets" => 'v',
            "sticky" => 'y',
            _ => 'd',
        };
        Ok(Value::Bool(regexp.flags.contains(flag)))
    }

    pub(super) fn set_required(
        &mut self,
        receiver: &Value,
        key: &str,
        value: Value,
    ) -> Result<(), RuntimeError> {
        let strict = std::mem::replace(&mut self.strict, true);
        let result = self.set_property(receiver, &key.into(), &value);
        self.strict = strict;
        result
    }

    pub(super) fn require_global_pattern(&mut self, pattern: &Value) -> Result<(), RuntimeError> {
        if self.is_regexp(pattern)? {
            let flags = self.get_property(pattern, &"flags".into())?;
            if matches!(flags, Value::Null | Value::Undefined) {
                return Err(RuntimeError::TypeError(
                    "RegExp flags must not be nullish".into(),
                ));
            }
            if !self
                .coerce_string(&flags)?
                .as_code_units()
                .contains(&u16::from(b'g'))
            {
                return Err(RuntimeError::TypeError(
                    "String method requires a global RegExp".into(),
                ));
            }
        }
        Ok(())
    }

    fn regexp_exec(
        &mut self,
        receiver: &Value,
        string: &JsString,
        builtin: bool,
    ) -> Result<Value, RuntimeError> {
        if !builtin {
            let exec = self.get_property(receiver, &"exec".into())?;
            if self.is_callable(&exec)? {
                let result = self.call_native(
                    exec,
                    receiver.clone(),
                    vec![Value::String(string.clone())],
                    false,
                )?;
                return if matches!(result, Value::Null | Value::Object(_)) {
                    Ok(result)
                } else {
                    Err(RuntimeError::TypeError(
                        "RegExp exec must return object or null".into(),
                    ))
                };
            }
        }
        let Value::Object(id) = receiver else {
            return Err(RuntimeError::TypeError(
                "RegExp exec requires a RegExp".into(),
            ));
        };
        let Some(regexp) = self.heap.regexp(*id)? else {
            return Err(RuntimeError::TypeError(
                "RegExp exec requires a RegExp".into(),
            ));
        };
        let last_index = self.get_property(receiver, &"lastIndex".into())?;
        let last_index = self.coerce_length(&last_index)?;
        let stateful = regexp.flags.contains(['g', 'y']);
        let start = if stateful { last_index as usize } else { 0 };
        self.charge_step()?;
        let matched = if start > string.len() {
            None
        } else {
            regexp
                .find(string, start, self.config.regex_timeout)?
                .filter(|m| !regexp.flags.contains('y') || m.start() == start)
        };
        let Some(matched) = matched else {
            if stateful {
                self.set_required(receiver, "lastIndex", Value::Number(0.0))?;
            }
            return Ok(Value::Null);
        };
        if stateful {
            self.set_required(receiver, "lastIndex", Value::Number(matched.end() as f64))?;
        }
        let values = matched
            .groups()
            .map(|range| {
                range.map_or(Value::Undefined, |range| {
                    Value::String(JsString::from_code_units(
                        string.as_code_units()[range].to_vec(),
                    ))
                })
            })
            .collect();
        let array = self.array_from(values)?;
        self.stack.push(array.clone());
        let array_id = array.object_id().unwrap();
        self.with_roots(|heap| heap.set(array_id, "index", Value::Number(matched.start() as f64)))?;
        self.with_roots(|heap| heap.set(array_id, "input", Value::String(string.clone())))?;
        let named: Vec<_> = matched
            .named_groups()
            .map(|(name, range)| (name.to_string(), range))
            .collect();
        let groups = if named.is_empty() {
            Value::Undefined
        } else {
            let object = self.with_roots(|heap| heap.alloc_object(None))?;
            self.stack.push(Value::Object(object));
            for (name, range) in &named {
                let value = range.as_ref().map_or(Value::Undefined, |r| {
                    Value::String(JsString::from_code_units(
                        string.as_code_units()[r.clone()].to_vec(),
                    ))
                });
                self.with_roots(|heap| heap.set(object, name.as_str(), value))?;
            }
            self.stack.pop();
            Value::Object(object)
        };
        self.with_roots(|heap| heap.set(array_id, "groups", groups))?;
        if regexp.flags.contains('d') {
            let mut pairs = Vec::new();
            for range in matched.groups() {
                let pair = if let Some(range) = range {
                    self.array_from(vec![
                        Value::Number(range.start as f64),
                        Value::Number(range.end as f64),
                    ])?
                } else {
                    Value::Undefined
                };
                self.stack.push(pair.clone());
                pairs.push(pair);
            }
            let indices = self.array_from(pairs)?;
            self.stack.push(indices.clone());
            let groups = if named.is_empty() {
                Value::Undefined
            } else {
                let object = self.with_roots(|heap| heap.alloc_object(None))?;
                self.stack.push(Value::Object(object));
                for (name, _) in named {
                    let capture = regexp.capture_names.iter().find(|(candidate, index)| {
                        candidate == &name && matched.group(*index).is_some()
                    });
                    let pair = if let Some((_, index)) = capture {
                        self.heap
                            .get(indices.object_id().unwrap(), index.to_string())?
                    } else {
                        Value::Undefined
                    };
                    self.with_roots(|heap| heap.set(object, name, pair))?;
                }
                Value::Object(object)
            };
            self.with_roots(|heap| heap.set(indices.object_id().unwrap(), "groups", groups))?;
            self.with_roots(|heap| heap.set(array_id, "indices", indices))?;
        }
        Ok(array)
    }

    pub(super) fn regexp_method(
        &mut self,
        method: RegExpMethod,
        receiver: &Value,
        args: &[Value],
    ) -> Result<Value, RuntimeError> {
        use RegExpMethod::*;
        if !matches!(receiver, Value::Object(_)) {
            return Err(RuntimeError::TypeError(
                "RegExp method requires an object".into(),
            ));
        }
        if method == Exec && self.heap.regexp(receiver.object_id().unwrap())?.is_none() {
            return Err(RuntimeError::TypeError(
                "RegExp exec requires a RegExp".into(),
            ));
        }
        let string = if method == ToString {
            JsString::default()
        } else {
            self.coerce_string(native::argument(args, 0))?
        };
        match method {
            Exec => self.regexp_exec(receiver, &string, true),
            Test => Ok(Value::Bool(
                self.regexp_exec(receiver, &string, false)? != Value::Null,
            )),
            Search => {
                let previous = self.get_property(receiver, &"lastIndex".into())?;
                self.stack.push(previous.clone());
                if !crate::heap::same_value(&previous, &Value::Number(0.0)) {
                    self.set_required(receiver, "lastIndex", Value::Number(0.0))?;
                }
                let result = self.regexp_exec(receiver, &string, false)?;
                self.stack.push(result.clone());
                let current = self.get_property(receiver, &"lastIndex".into())?;
                if !crate::heap::same_value(&previous, &current) {
                    self.set_required(receiver, "lastIndex", previous)?;
                }
                if result == Value::Null {
                    Ok(Value::Number(-1.0))
                } else {
                    self.get_property(&result, &"index".into())
                }
            }
            Match => {
                let flags = self.get_property(receiver, &"flags".into())?;
                let flags = self.coerce_string(&flags)?;
                if !flags.as_code_units().contains(&u16::from(b'g')) {
                    return self.regexp_exec(receiver, &string, false);
                }
                let unicode = flags
                    .as_code_units()
                    .iter()
                    .any(|&c| c == u16::from(b'u') || c == u16::from(b'v'));
                self.set_required(receiver, "lastIndex", Value::Number(0.0))?;
                let mut results = Vec::new();
                loop {
                    self.charge_step()?;
                    let result = self.regexp_exec(receiver, &string, false)?;
                    if result == Value::Null {
                        break;
                    }
                    self.stack.push(result.clone());
                    let matched = self.get_property(&result, &"0".into())?;
                    let matched = self.coerce_string(&matched)?;
                    if matched.is_empty() {
                        self.advance_last_index(receiver, &string, unicode)?;
                    }
                    results.push(Value::String(matched));
                }
                if results.is_empty() {
                    Ok(Value::Null)
                } else {
                    self.array_from(results)
                }
            }
            MatchAll => {
                let constructor = self.regexp_species_constructor(receiver)?;
                self.stack.push(constructor.clone());
                let flags = self.get_property(receiver, &"flags".into())?;
                let flags = self.coerce_string(&flags)?;
                let matcher = self.call_native(
                    constructor,
                    Value::Undefined,
                    vec![receiver.clone(), Value::String(flags.clone())],
                    true,
                )?;
                self.stack.push(matcher.clone());
                let last_index = self.get_property(receiver, &"lastIndex".into())?;
                let last_index = self.coerce_length(&last_index)?;
                self.set_required(&matcher, "lastIndex", Value::Number(last_index))?;
                let global = flags.as_code_units().contains(&u16::from(b'g'));
                let unicode = flags
                    .as_code_units()
                    .iter()
                    .any(|&c| c == u16::from(b'u') || c == u16::from(b'v'));
                let prototype = self.regexp_iterator_prototype()?;
                Ok(Value::Object(self.with_roots(|heap| {
                    heap.alloc_regexp_iterator(
                        matcher.object_id().unwrap(),
                        string,
                        global,
                        unicode,
                        prototype,
                    )
                })?))
            }
            Split => self.regexp_split(receiver, &string, native::argument(args, 1)),
            Replace => self.regexp_replace(receiver, &string, native::argument(args, 1)),
            ToString => {
                let source = self.get_property(receiver, &"source".into())?;
                let source = self.coerce_string(&source)?;
                let flags = self.get_property(receiver, &"flags".into())?;
                let flags = self.coerce_string(&flags)?;
                let mut result = JsString::from("/");
                for part in [source, "/".into(), flags] {
                    native::append(&mut result, &part, self.config.max_string_bytes)?;
                }
                Ok(Value::String(result))
            }
        }
    }

    fn advance_last_index(
        &mut self,
        receiver: &Value,
        string: &JsString,
        unicode: bool,
    ) -> Result<(), RuntimeError> {
        let index = self.get_property(receiver, &"lastIndex".into())?;
        let index = self.coerce_length(&index)? as usize;
        self.set_required(
            receiver,
            "lastIndex",
            Value::Number(advance(string, index, unicode) as f64),
        )
    }

    fn regexp_species_constructor(&mut self, receiver: &Value) -> Result<Value, RuntimeError> {
        let constructor = self.get_property(receiver, &"constructor".into())?;
        if constructor != Value::Undefined {
            if !matches!(constructor, Value::Object(_)) {
                return Err(RuntimeError::TypeError(
                    "RegExp constructor must be an object".into(),
                ));
            }
            let species =
                self.get_property(&constructor, &JsSymbol::well_known("species").into())?;
            if !matches!(species, Value::Undefined | Value::Null) {
                if !self.is_constructor(&species)? {
                    return Err(RuntimeError::TypeError(
                        "RegExp species must be a constructor".into(),
                    ));
                }
                return Ok(species);
            }
        }
        self.regexp_global()
    }

    fn regexp_iterator_prototype(&mut self) -> Result<ObjectId, RuntimeError> {
        if let Some(prototype) = self.regexp_iterator_prototype {
            return Ok(prototype);
        }
        let constructor = self.string_intrinsics()?.0;
        let function_prototype = self.heap.prototype(constructor)?.unwrap();
        let iterator_base = self.base_iterator_prototype()?;
        let prototype = self.with_roots(|heap| heap.alloc_object(Some(iterator_base)))?;
        let root = self.heap.root(prototype)?;
        let result = (|| {
            self.install_native(
                prototype,
                function_prototype,
                "next",
                0,
                NativeFunction::RegExpIteratorNext,
            )?;
            self.define_data(
                prototype,
                JsSymbol::well_known("toStringTag"),
                Value::String("RegExp String Iterator".into()),
                false,
                false,
                true,
            )?;
            Ok(prototype)
        })();
        if result.is_err() {
            self.heap.unroot(root)?;
        } else {
            self.regexp_iterator_prototype = Some(prototype);
        }
        result
    }

    pub(super) fn regexp_iterator_next(&mut self, receiver: &Value) -> Result<Value, RuntimeError> {
        let Value::Object(id) = receiver else {
            return Err(RuntimeError::TypeError(
                "RegExp iterator next requires an iterator".into(),
            ));
        };
        let Some((matcher, string, global, unicode, done)) = self.heap.regexp_iterator(*id)? else {
            return Err(RuntimeError::TypeError(
                "RegExp iterator next requires an iterator".into(),
            ));
        };
        if done {
            return self.iterator_result(Value::Undefined, true);
        }
        let matcher = Value::Object(matcher);
        let result = self.regexp_exec(&matcher, &string, false)?;
        self.stack.push(result.clone());
        if result == Value::Null {
            self.heap.finish_regexp_iterator(*id);
            return self.iterator_result(Value::Undefined, true);
        }
        if !global {
            self.heap.finish_regexp_iterator(*id);
        } else {
            let matched = self.get_property(&result, &"0".into())?;
            if self.coerce_string(&matched)?.is_empty() {
                self.advance_last_index(&matcher, &string, unicode)?;
            }
        }
        self.iterator_result(result, false)
    }

    fn regexp_split(
        &mut self,
        receiver: &Value,
        string: &JsString,
        limit: &Value,
    ) -> Result<Value, RuntimeError> {
        let constructor = self.regexp_species_constructor(receiver)?;
        self.stack.push(constructor.clone());
        let flags = self.get_property(receiver, &"flags".into())?;
        let mut flags = self.coerce_string(&flags)?;
        let unicode = flags
            .as_code_units()
            .iter()
            .any(|&c| c == u16::from(b'u') || c == u16::from(b'v'));
        if !flags.as_code_units().contains(&u16::from(b'y')) {
            flags.push_str(&"y".into());
        }
        let splitter = self.call_native(
            constructor,
            Value::Undefined,
            vec![receiver.clone(), Value::String(flags)],
            true,
        )?;
        self.stack.push(splitter.clone());
        let limit = if *limit == Value::Undefined {
            u32::MAX
        } else {
            native::uint32(&Value::Number(self.coerce_number(limit)?))?
        } as usize;
        let mut values = Vec::new();
        if limit == 0 {
            return self.array_from(values);
        }
        if string.is_empty() {
            if self.regexp_exec(&splitter, string, false)? == Value::Null {
                values.push(Value::String(string.clone()));
            }
            return self.array_from(values);
        }
        let mut start = 0;
        let mut position = 0;
        while position < string.len() {
            self.charge_step()?;
            self.set_required(&splitter, "lastIndex", Value::Number(position as f64))?;
            let result = self.regexp_exec(&splitter, string, false)?;
            if result == Value::Null {
                position = advance(string, position, unicode);
                continue;
            }
            self.stack.push(result.clone());
            let end = self.get_property(&splitter, &"lastIndex".into())?;
            let end = (self.coerce_length(&end)? as usize).min(string.len());
            if end == start {
                position = advance(string, position, unicode);
                continue;
            }
            values.push(Value::String(JsString::from_code_units(
                string.as_code_units()[start..position].to_vec(),
            )));
            if values.len() == limit {
                return self.array_from(values);
            }
            start = end;
            let length = self.get_property(&result, &"length".into())?;
            let length = self.coerce_length(&length)? as u64;
            for index in 1..length {
                self.charge_step()?;
                let value = self.get_property(&result, &index.to_string().into())?;
                self.stack.push(value.clone());
                values.push(value);
                if values.len() == limit {
                    return self.array_from(values);
                }
            }
            position = start;
        }
        values.push(Value::String(JsString::from_code_units(
            string.as_code_units()[start..].to_vec(),
        )));
        self.array_from(values)
    }

    fn regexp_replace(
        &mut self,
        receiver: &Value,
        string: &JsString,
        replacement: &Value,
    ) -> Result<Value, RuntimeError> {
        let callable = self.is_callable(replacement)?;
        let template = if callable {
            JsString::default()
        } else {
            self.coerce_string(replacement)?
        };
        let flags = self.get_property(receiver, &"flags".into())?;
        let flags = self.coerce_string(&flags)?;
        let global = flags.as_code_units().contains(&u16::from(b'g'));
        let unicode = flags
            .as_code_units()
            .iter()
            .any(|&c| c == u16::from(b'u') || c == u16::from(b'v'));
        if global {
            self.set_required(receiver, "lastIndex", Value::Number(0.0))?;
        }
        let mut results = Vec::new();
        loop {
            self.charge_step()?;
            let result = self.regexp_exec(receiver, string, false)?;
            if result == Value::Null {
                break;
            }
            self.stack.push(result.clone());
            results.push(result.clone());
            if !global {
                break;
            }
            let matched = self.get_property(&result, &"0".into())?;
            if self.coerce_string(&matched)?.is_empty() {
                self.advance_last_index(receiver, string, unicode)?;
            }
        }
        let mut output = JsString::default();
        let mut next = 0;
        for result in results {
            let length = self.get_property(&result, &"length".into())?;
            let length = self.coerce_length(&length)? as usize;
            let matched = self.get_property(&result, &"0".into())?;
            let matched = self.coerce_string(&matched)?;
            let position = self.get_property(&result, &"index".into())?;
            let position = native::integer(&Value::Number(self.coerce_number(&position)?))?
                .clamp(0.0, string.len() as f64) as usize;
            let mut captures = Vec::new();
            for index in 1..length {
                self.charge_step()?;
                let capture = self.get_property(&result, &index.to_string().into())?;
                captures.push(if capture == Value::Undefined {
                    Value::Undefined
                } else {
                    Value::String(self.coerce_string(&capture)?)
                });
            }
            let groups = self.get_property(&result, &"groups".into())?;
            self.stack.push(groups.clone());
            let replacement = if callable {
                let mut args = vec![Value::String(matched.clone())];
                args.extend(captures.iter().cloned());
                args.extend([
                    Value::Number(position as f64),
                    Value::String(string.clone()),
                ]);
                if groups != Value::Undefined {
                    args.push(groups);
                }
                let result =
                    self.call_native(replacement.clone(), Value::Undefined, args, false)?;
                self.coerce_string(&result)?
            } else {
                self.capture_substitution(
                    string, &matched, position, &captures, &groups, &template,
                )?
            };
            if position >= next {
                native::append(
                    &mut output,
                    &JsString::from_code_units(string.as_code_units()[next..position].to_vec()),
                    self.config.max_string_bytes,
                )?;
                native::append(&mut output, &replacement, self.config.max_string_bytes)?;
                next = (position + matched.len()).min(string.len());
            }
        }
        native::append(
            &mut output,
            &JsString::from_code_units(string.as_code_units()[next..].to_vec()),
            self.config.max_string_bytes,
        )?;
        Ok(Value::String(output))
    }

    fn capture_substitution(
        &mut self,
        string: &JsString,
        matched: &JsString,
        position: usize,
        captures: &[Value],
        groups: &Value,
        template: &JsString,
    ) -> Result<JsString, RuntimeError> {
        if *groups != Value::Undefined {
            self.coerce_object(groups)?;
        }
        let units = template.as_code_units();
        let mut index = 0;
        let mut result = JsString::default();
        while index < units.len() {
            self.charge_step()?;
            let mut consumed = 1;
            let mut part = JsString::from_code_units(vec![units[index]]);
            if units[index] == u16::from(b'$') {
                match units.get(index + 1).copied() {
                    Some(0x24) => {
                        consumed = 2;
                    }
                    Some(0x26) => {
                        part = matched.clone();
                        consumed = 2;
                    }
                    Some(0x60) => {
                        part =
                            JsString::from_code_units(string.as_code_units()[..position].to_vec());
                        consumed = 2;
                    }
                    Some(0x27) => {
                        part = JsString::from_code_units(
                            string.as_code_units()[(position + matched.len()).min(string.len())..]
                                .to_vec(),
                        );
                        consumed = 2;
                    }
                    Some(digit @ 0x30..=0x39) => {
                        let mut number = (digit - 0x30) as usize;
                        if let Some(second @ 0x30..=0x39) = units.get(index + 2).copied() {
                            let candidate = number * 10 + (second - 0x30) as usize;
                            if candidate > 0 && candidate <= captures.len() {
                                number = candidate;
                                consumed = 3;
                            }
                        }
                        if number > 0 && number <= captures.len() {
                            consumed = consumed.max(2);
                            part = if let Value::String(capture) = &captures[number - 1] {
                                capture.clone()
                            } else {
                                JsString::default()
                            };
                        }
                    }
                    Some(0x3c) if *groups != Value::Undefined => {
                        if let Some(end) = units[index + 2..].iter().position(|&u| u == 0x3e) {
                            let key = JsString::from_code_units(
                                units[index + 2..index + 2 + end].to_vec(),
                            );
                            let value = self.get_property(groups, &key.into())?;
                            part = if value == Value::Undefined {
                                JsString::default()
                            } else {
                                self.coerce_string(&value)?
                            };
                            consumed = end + 3;
                        }
                    }
                    _ => {}
                }
            }
            native::append(&mut result, &part, self.config.max_string_bytes)?;
            index += consumed;
        }
        Ok(result)
    }
}
