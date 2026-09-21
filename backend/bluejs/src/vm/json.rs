// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

//! JSON's parse and stringify paths. They preserve reviver/replacer callbacks,
//! ordinary property enumeration, `toJSON`, getters, and JSON's finite-number
//! restriction. Raw JSON objects retain their branded internal slot rather
//! than being approximated by ordinary objects.

use super::*;
use std::collections::HashMap;

type JsonSourceMap = HashMap<(ObjectId, JsString), (Value, JsString)>;

impl Vm {
    pub(super) fn json_global(&mut self) -> Result<Value, RuntimeError> {
        if let Some(&id) = self.globals.get("JSON") {
            return Ok(Value::Object(id));
        }
        let function_prototype = self.function_prototype()?;
        let object_prototype = self.object_prototype;
        let id = self.with_roots(|heap| heap.alloc_object(Some(object_prototype)))?;
        let root = self.heap.root(id)?;
        let result = (|| {
            self.install_native(
                id,
                function_prototype,
                "parse",
                2,
                NativeFunction::JsonParse,
            )?;
            self.install_native(
                id,
                function_prototype,
                "stringify",
                3,
                NativeFunction::JsonStringify,
            )?;
            self.install_native(
                id,
                function_prototype,
                "rawJSON",
                1,
                NativeFunction::JsonRawJson,
            )?;
            self.install_native(
                id,
                function_prototype,
                "isRawJSON",
                1,
                NativeFunction::JsonIsRawJson,
            )?;
            self.define_data(
                id,
                JsSymbol::well_known("toStringTag"),
                Value::String("JSON".into()),
                false,
                false,
                true,
            )?;
            Ok(Value::Object(id))
        })();
        match result {
            Ok(value) => {
                self.globals.insert("JSON".into(), id);
                if let Some(&global) = self.globals.get("globalThis") {
                    self.define_data(global, "JSON", Value::Object(id), true, false, true)?;
                }
                Ok(value)
            }
            Err(error) => {
                self.heap.unroot(root)?;
                Err(error)
            }
        }
    }

    pub(super) fn json_parse(
        &mut self,
        input: &Value,
        reviver: Option<&Value>,
    ) -> Result<Value, RuntimeError> {
        let base = self.stack.len();
        let result = (|| {
            let input = self.coerce_string(input)?;
            let input = input
                .to_utf8()
                .map_err(|_| RuntimeError::SyntaxError("invalid JSON text".into()))?;
            let parsed = JsonSourceParser::new(&input)
                .parse()
                .map_err(|_| RuntimeError::SyntaxError("invalid JSON text".into()))?;
            let mut sources = JsonSourceMap::new();
            let value = self.json_from_node(&parsed, &mut sources)?;
            let reviver = reviver.cloned().unwrap_or(Value::Undefined);
            if !self.is_callable(&reviver)? {
                return Ok(value);
            }
            // The complete parsed graph is now outside the interpreter stack.
            // Root its result before allocating the wrapper below:
            // even a one-object nursery can otherwise collect a top-level
            // Array/Object between the conversion and CreateDataProperty.
            self.stack.push(value.clone());
            self.stack.push(reviver.clone());
            let prototype = self.object_prototype;
            let wrapper = self.with_roots(|heap| heap.alloc_object(Some(prototype)))?;
            self.stack.push(Value::Object(wrapper));
            self.define_data(wrapper, "", value, true, true, true)?;
            if let Some(source) = parsed.primitive_source() {
                let value = self.get_property(&Value::Object(wrapper), &"".into())?;
                sources.insert((wrapper, "".into()), (value, source.clone()));
            }
            self.json_internalize(&Value::Object(wrapper), &"".into(), &reviver, &sources)
        })();
        self.stack.truncate(base);
        result
    }

    /// ECMAScript §25.5.3 `JSON.rawJSON`. A raw JSON fragment is limited to
    /// primitive JSON values, so embedding it cannot change the enclosing
    /// array/object structure during `JSON.stringify`.
    pub(super) fn json_raw_json(&mut self, text: &Value) -> Result<Value, RuntimeError> {
        let text = self.coerce_string(text)?;
        let utf8 = text
            .to_utf8()
            .map_err(|_| RuntimeError::SyntaxError("invalid raw JSON text".into()))?;
        if utf8.is_empty()
            || matches!(utf8.as_bytes().first(), Some(b'\t' | b'\n' | b'\r' | b' '))
            || matches!(utf8.as_bytes().last(), Some(b'\t' | b'\n' | b'\r' | b' '))
        {
            return Err(RuntimeError::SyntaxError("invalid raw JSON text".into()));
        }
        let parsed = JsonSourceParser::new(&utf8)
            .parse()
            .map_err(|_| RuntimeError::SyntaxError("invalid raw JSON text".into()))?;
        if parsed.is_container() {
            return Err(RuntimeError::SyntaxError("invalid raw JSON text".into()));
        }

        let raw = self.with_roots(|heap| heap.alloc_raw_json())?;
        let base = self.stack.len();
        self.stack.push(Value::Object(raw));
        let result = (|| {
            self.define_data(raw, "rawJSON", Value::String(text), true, true, true)?;
            if !self.object_define_own_property(
                raw,
                "rawJSON".into(),
                PropertyDescriptor {
                    writable: Some(false),
                    configurable: Some(false),
                    ..Default::default()
                },
            )? {
                return Err(RuntimeError::TypeError(
                    "cannot freeze raw JSON text".into(),
                ));
            }
            if !self.object_prevent_extensions(raw)? {
                return Err(RuntimeError::TypeError(
                    "cannot freeze raw JSON object".into(),
                ));
            }
            Ok(Value::Object(raw))
        })();
        self.stack.truncate(base);
        result
    }

    /// ECMAScript §25.5.4 `JSON.isRawJSON` observes the actual internal slot,
    /// not a look-alike object's `rawJSON` property or a Proxy target.
    pub(super) fn json_is_raw_json(&self, value: &Value) -> Result<Value, RuntimeError> {
        let Value::Object(object) = value else {
            return Ok(Value::Bool(false));
        };
        Ok(Value::Bool(self.heap.is_raw_json(*object)?))
    }

    /// `InternalizeJSONProperty` walks the parsed graph post-order, always
    /// using the ordinary internal-method boundary. Reviver callbacks can
    /// replace a child with a Proxy, remove properties, or make the receiver
    /// non-extensible before the required DeletePropertyOrThrow or
    /// CreateDataProperty step.
    fn json_internalize(
        &mut self,
        holder: &Value,
        name: &JsString,
        reviver: &Value,
        sources: &JsonSourceMap,
    ) -> Result<Value, RuntimeError> {
        let base = self.stack.len();
        let result = (|| {
            self.stack.push(holder.clone());
            let value = self.get_property(holder, &PropertyName::from(name))?;
            self.stack.push(value.clone());
            let source = match holder {
                Value::Object(object) => sources
                    .get(&(*object, name.clone()))
                    .filter(|(parsed, _)| crate::heap::same_value(parsed, &value))
                    .map(|(_, source)| source.clone()),
                _ => None,
            };
            if let Value::Object(object) = value {
                if self.is_array(&Value::Object(object))? {
                    let length = self.get_property(&Value::Object(object), &"length".into())?;
                    let length = self.coerce_length(&length)? as u64;
                    for index in 0..length {
                        self.charge_step()?;
                        let key: JsString = index.to_string().into();
                        let replacement =
                            self.json_internalize(&Value::Object(object), &key, reviver, sources)?;
                        self.stack.push(replacement.clone());
                        let property = PropertyName::from(&key);
                        // A `false` from `[[Delete]]` or CreateDataProperty is
                        // ignored; only an abrupt completion propagates.
                        if replacement == Value::Undefined {
                            self.object_delete(object, &property)?;
                        } else {
                            self.object_define_own_property(
                                object,
                                property,
                                PropertyDescriptor::data(replacement, true, true, true),
                            )?;
                        }
                        self.stack.pop();
                    }
                } else {
                    // EnumerableOwnProperties: the keys are collected (and
                    // their enumerability read) up front, so a property the
                    // reviver later deletes is still visited.
                    let mut enumerable = Vec::new();
                    for key in self.object_own_property_keys(object)? {
                        self.charge_step()?;
                        let PropertyName::String(key) = key else {
                            continue;
                        };
                        if self
                            .object_get_own_property(object, &PropertyName::from(&key))?
                            .is_none_or(|descriptor| descriptor.enumerable != Some(true))
                        {
                            continue;
                        }
                        enumerable.push(key);
                    }
                    for key in enumerable {
                        self.charge_step()?;
                        let replacement =
                            self.json_internalize(&Value::Object(object), &key, reviver, sources)?;
                        self.stack.push(replacement.clone());
                        let property = PropertyName::from(&key);
                        // A `false` from `[[Delete]]` or CreateDataProperty is
                        // ignored; only an abrupt completion propagates.
                        if replacement == Value::Undefined {
                            self.object_delete(object, &property)?;
                        } else {
                            self.object_define_own_property(
                                object,
                                property,
                                PropertyDescriptor::data(replacement, true, true, true),
                            )?;
                        }
                        self.stack.pop();
                    }
                }
            }
            let prototype = self.object_prototype;
            let context = self.with_roots(|heap| heap.alloc_object(Some(prototype)))?;
            self.stack.push(Value::Object(context));
            if let Some(source) = source {
                self.define_data(context, "source", Value::String(source), true, true, true)?;
            }
            self.call_native(
                reviver.clone(),
                holder.clone(),
                vec![Value::String(name.clone()), value, Value::Object(context)],
                false,
            )
        })();
        self.stack.truncate(base);
        result
    }

    fn json_from_node(
        &mut self,
        node: &JsonNode,
        sources: &mut JsonSourceMap,
    ) -> Result<Value, RuntimeError> {
        match node {
            JsonNode::Primitive { value, .. } => {
                self.check_string(value)?;
                Ok(value.clone())
            }
            JsonNode::Array(values) => {
                let prototype = self.array_prototype;
                let array =
                    self.with_roots(|heap| heap.alloc_array(values.len() as u32, Some(prototype)))?;
                self.stack.push(Value::Object(array));
                let result = (|| {
                    for (index, child) in values.iter().enumerate() {
                        self.charge_step()?;
                        let value = self.json_from_node(child, sources)?;
                        self.stack.push(value.clone());
                        let key: JsString = index.to_string().into();
                        self.with_roots(|heap| heap.set(array, key.clone(), value.clone()))?;
                        self.json_record_source(array, key, child, &value, sources);
                        self.stack.pop();
                    }
                    Ok(Value::Object(array))
                })();
                self.stack.pop();
                result
            }
            JsonNode::Object(values) => {
                let prototype = self.object_prototype;
                let object = self.with_roots(|heap| heap.alloc_object(Some(prototype)))?;
                self.stack.push(Value::Object(object));
                let result = (|| {
                    for (key, child) in values {
                        self.charge_step()?;
                        let value = self.json_from_node(child, sources)?;
                        self.stack.push(value.clone());
                        self.with_roots(|heap| heap.set(object, key.clone(), value.clone()))?;
                        self.json_record_source(object, key.clone(), child, &value, sources);
                        self.stack.pop();
                    }
                    Ok(Value::Object(object))
                })();
                self.stack.pop();
                result
            }
        }
    }

    fn json_record_source(
        &self,
        holder: ObjectId,
        key: JsString,
        node: &JsonNode,
        value: &Value,
        sources: &mut JsonSourceMap,
    ) {
        let map_key = (holder, key);
        if let Some(source) = node.primitive_source() {
            sources.insert(map_key, (value.clone(), source.clone()));
        } else {
            sources.remove(&map_key);
        }
    }

    /// ECMAScript §25.5.2 `JSON.stringify`.
    ///
    /// The native dispatcher provides the complete argument list here: unlike
    /// ordinary data serialization, the replacer is observable before the
    /// first value is read. In particular, an Array Proxy must be recognised
    /// by `IsArray` before its `length` getter can revoke it.
    pub(super) fn json_stringify(&mut self, args: &[Value]) -> Result<Value, RuntimeError> {
        let base = self.stack.len();
        let result = (|| {
            let replacer = native::argument(args, 1).clone();
            self.stack.push(replacer.clone());
            let space = native::argument(args, 2).clone();
            self.stack.push(space.clone());
            let state = self.json_stringify_state(&replacer, &space)?;

            let prototype = self.object_prototype;
            let wrapper = self.with_roots(|heap| heap.alloc_object(Some(prototype)))?;
            self.stack.push(Value::Object(wrapper));
            self.define_data(
                wrapper,
                "",
                native::argument(args, 0).clone(),
                true,
                true,
                true,
            )?;
            match self.json_str(
                &Value::Object(wrapper),
                &"".into(),
                &state,
                &JsString::default(),
            )? {
                Some(value) => {
                    self.check_string(&Value::String(value.clone()))?;
                    Ok(Value::String(value))
                }
                None => Ok(Value::Undefined),
            }
        })();
        self.stack.truncate(base);
        result
    }

    fn json_stringify_state(
        &mut self,
        replacer: &Value,
        space: &Value,
    ) -> Result<JsonStringifyState, RuntimeError> {
        if self.is_callable(replacer)? {
            return Ok(JsonStringifyState {
                replacer: Some(replacer.clone()),
                property_list: None,
                gap: self.json_gap(space)?,
            });
        }
        if !self.is_array(replacer)? {
            return Ok(JsonStringifyState {
                gap: self.json_gap(space)?,
                ..JsonStringifyState::default()
            });
        }

        // `IsArray` above deliberately does not access Proxy traps. The
        // following Get(length) is the first observable access, as required
        // by SerializeJSONProperty's PropertyList construction.
        let length = self.get_property(replacer, &"length".into())?;
        let length = self.coerce_length(&length)? as u64;
        let mut property_list = Vec::new();
        for index in 0..length {
            self.charge_step()?;
            let item = self.get_property(replacer, &index.to_string().into())?;
            self.stack.push(item.clone());
            let key = self.json_property_list_key(&item)?;
            self.stack.pop();
            if let Some(key) = key.filter(|key| !property_list.contains(key)) {
                property_list.push(key);
            }
        }
        Ok(JsonStringifyState {
            replacer: None,
            property_list: Some(property_list),
            gap: self.json_gap(space)?,
        })
    }

    /// JSON's `gap` construction only observes String/Number values and their
    /// boxed forms. Other objects, including objects with a custom
    /// `toString`, are ignored without coercion.
    fn json_gap(&mut self, space: &Value) -> Result<JsString, RuntimeError> {
        let primitive = if let Value::Object(object) = space {
            if self.heap.boxed_string(*object)?.is_some() {
                Value::String(self.coerce_string(space)?)
            } else if matches!(self.heap.boxed_primitive(*object)?, Some(Value::Number(_))) {
                Value::Number(self.coerce_number(space)?)
            } else {
                Value::Undefined
            }
        } else {
            space.clone()
        };
        match primitive {
            Value::Number(value) => {
                let width = if value.is_nan() || value <= 0.0 {
                    0
                } else if !value.is_finite() || value >= 10.0 {
                    10
                } else {
                    value.trunc() as usize
                };
                Ok(" ".repeat(width).into())
            }
            Value::String(value) => Ok(JsString::from_code_units(
                value.as_code_units()[..value.len().min(10)].to_vec(),
            )),
            _ => Ok(JsString::default()),
        }
    }

    fn json_property_list_key(&mut self, item: &Value) -> Result<Option<JsString>, RuntimeError> {
        match item {
            Value::String(_) | Value::Number(_) => Ok(Some(self.coerce_string(item)?)),
            Value::Object(object) if self.heap.boxed_string(*object)?.is_some() => {
                Ok(Some(self.coerce_string(item)?))
            }
            Value::Object(object)
                if matches!(
                    self.heap.boxed_primitive(*object)?,
                    Some(Value::String(_) | Value::Number(_))
                ) =>
            {
                Ok(Some(self.coerce_string(item)?))
            }
            _ => Ok(None),
        }
    }

    /// ECMAScript `IsArray`, including the transparent Proxy recursion.
    pub(super) fn is_array(&self, value: &Value) -> Result<bool, RuntimeError> {
        let Value::Object(mut object) = value else {
            return Ok(false);
        };
        loop {
            if self.heap.is_array(object)? {
                return Ok(true);
            }
            let Some((target, _)) = self.heap.proxy(object)? else {
                return Ok(false);
            };
            object = target;
        }
    }

    /// JSON's `Str` abstract operation. Keeping the holder and key explicit
    /// makes `toJSON` and a function replacer observe exactly the receiver
    /// and key used by the specification.
    fn json_str(
        &mut self,
        holder: &Value,
        key: &JsString,
        state: &JsonStringifyState,
        indent: &JsString,
    ) -> Result<Option<JsString>, RuntimeError> {
        let base = self.stack.len();
        let result = (|| {
            self.stack.push(holder.clone());
            let mut value = self.get_property(holder, &PropertyName::from(key))?;
            self.stack.push(value.clone());
            if matches!(value, Value::Object(_) | Value::BigInt(_)) {
                let to_json = self.get_property(&value, &"toJSON".into())?;
                if self.is_callable(&to_json)? {
                    value = self.call_native(
                        to_json,
                        value.clone(),
                        vec![Value::String(key.clone())],
                        false,
                    )?;
                    self.stack.push(value.clone());
                }
            }
            if let Some(replacer) = &state.replacer {
                value = self.call_native(
                    replacer.clone(),
                    holder.clone(),
                    vec![Value::String(key.clone()), value],
                    false,
                )?;
                self.stack.push(value.clone());
            }
            self.json_serialize(&value, state, indent)
        })();
        self.stack.truncate(base);
        result
    }

    fn json_serialize(
        &mut self,
        value: &Value,
        state: &JsonStringifyState,
        indent: &JsString,
    ) -> Result<Option<JsString>, RuntimeError> {
        match value {
            Value::Undefined | Value::Symbol(_) => Ok(None),
            Value::Null => Ok(Some("null".into())),
            Value::Bool(value) => Ok(Some(if *value { "true" } else { "false" }.into())),
            Value::Number(value) => Ok(Some(if value.is_finite() {
                primitive::string(&Value::Number(*value))?
            } else {
                "null".into()
            })),
            Value::BigInt(_) => Err(RuntimeError::TypeError(
                "cannot serialize a BigInt value as JSON".into(),
            )),
            Value::String(value) => Ok(Some(json_quote(value).into())),
            Value::Object(object) => {
                if self.heap.is_raw_json(*object)? {
                    let text = self.get_property(value, &"rawJSON".into())?;
                    let Value::String(text) = text else {
                        unreachable!("a branded raw JSON object retains its frozen text");
                    };
                    return Ok(Some(text));
                }
                if self.heap.boxed_string(*object)?.is_some() {
                    let string = Value::String(self.coerce_string(value)?);
                    return self.json_serialize(&string, state, indent);
                }
                if let Some(primitive) = self
                    .heap
                    .boxed_primitive(*object)?
                    .or(self.test262_foreign_boxed_primitive(*object)?)
                {
                    return match primitive {
                        Value::Number(_) => {
                            let number = Value::Number(self.coerce_number(value)?);
                            self.json_serialize(&number, state, indent)
                        }
                        Value::String(_) => {
                            let string = Value::String(self.coerce_string(value)?);
                            self.json_serialize(&string, state, indent)
                        }
                        primitive => self.json_serialize(&primitive, state, indent),
                    };
                }
                if self.is_callable(value)? {
                    return Ok(None);
                }
                if self.joining.contains(object) {
                    return Err(RuntimeError::TypeError("cyclic JSON value".into()));
                }
                self.joining.push(*object);
                let result = if self.is_array(value)? {
                    self.json_array(*object, state, indent)
                } else {
                    self.json_object(*object, state, indent)
                };
                self.joining.pop();
                result.map(Some)
            }
        }
    }
    fn json_array(
        &mut self,
        array: ObjectId,
        state: &JsonStringifyState,
        indent: &JsString,
    ) -> Result<JsString, RuntimeError> {
        let length = self.get_property(&Value::Object(array), &"length".into())?;
        let length = self.coerce_length(&length)? as u64;
        let mut text: JsString = "[".into();
        let mut next_indent = indent.clone();
        next_indent.push_str(&state.gap);
        let mut values = Vec::new();
        for index in 0..length {
            self.charge_step()?;
            let value = self
                .json_str(
                    &Value::Object(array),
                    &index.to_string().into(),
                    state,
                    &next_indent,
                )?
                .unwrap_or_else(|| "null".into());
            values.push(value);
        }
        if !state.gap.is_empty() && !values.is_empty() {
            text.push_str(&"\n".into());
            text.push_str(&next_indent);
        }
        for (index, value) in values.into_iter().enumerate() {
            if index != 0 {
                let separator: JsString = if state.gap.is_empty() { "," } else { ",\n" }.into();
                text.push_str(&separator);
                if !state.gap.is_empty() {
                    text.push_str(&next_indent);
                }
            }
            text.push_str(&value);
        }
        if !state.gap.is_empty() && length != 0 {
            text.push_str(&"\n".into());
            text.push_str(indent);
        }
        text.push_str(&"]".into());
        Ok(text)
    }

    fn json_object(
        &mut self,
        object: ObjectId,
        state: &JsonStringifyState,
        indent: &JsString,
    ) -> Result<JsString, RuntimeError> {
        let base = self.stack.len();
        self.stack.push(Value::Object(object));
        let result = (|| {
            let mut text: JsString = "{".into();
            let mut next_indent = indent.clone();
            next_indent.push_str(&state.gap);
            let mut values = Vec::new();
            let keys = if let Some(property_list) = &state.property_list {
                property_list.clone()
            } else {
                // EnumerableOwnProperties: keys and their enumerability are
                // read before any property is serialized, so a getter or a
                // replacer that deletes a later property does not remove it
                // from the output (it is serialized as `undefined`).
                let mut enumerable = Vec::new();
                for key in self.object_own_property_keys(object)? {
                    let PropertyName::String(key) = key else {
                        continue;
                    };
                    self.charge_step()?;
                    if self
                        .object_get_own_property(object, &PropertyName::from(&key))?
                        .is_some_and(|descriptor| descriptor.enumerable == Some(true))
                    {
                        enumerable.push(key);
                    }
                }
                enumerable
            };
            for key in keys {
                self.charge_step()?;
                let Some(value) =
                    self.json_str(&Value::Object(object), &key, state, &next_indent)?
                else {
                    continue;
                };
                values.push((key, value));
            }
            if !state.gap.is_empty() && !values.is_empty() {
                text.push_str(&"\n".into());
                text.push_str(&next_indent);
            }
            let has_values = !values.is_empty();
            for (index, (key, value)) in values.into_iter().enumerate() {
                if index != 0 {
                    let separator: JsString = if state.gap.is_empty() { "," } else { ",\n" }.into();
                    text.push_str(&separator);
                    if !state.gap.is_empty() {
                        text.push_str(&next_indent);
                    }
                }
                text.push_str(&JsString::from(json_quote(&key)));
                let separator: JsString = if state.gap.is_empty() { ":" } else { ": " }.into();
                text.push_str(&separator);
                text.push_str(&value);
            }
            if !state.gap.is_empty() && has_values {
                text.push_str(&"\n".into());
                text.push_str(indent);
            }
            text.push_str(&"}".into());
            Ok(text)
        })();
        self.stack.truncate(base);
        result
    }
}

/// A parsed JSON value retaining each primitive's original JSON source text.
/// `JSON.parse` needs this record only while it invokes a reviver; the public
/// result remains ordinary ECMAScript values and objects.
enum JsonNode {
    Primitive { value: Value, source: JsString },
    Array(Vec<JsonNode>),
    Object(Vec<(JsString, JsonNode)>),
}

impl JsonNode {
    fn primitive_source(&self) -> Option<&JsString> {
        match self {
            Self::Primitive { source, .. } => Some(source),
            Self::Array(_) | Self::Object(_) => None,
        }
    }

    fn is_container(&self) -> bool {
        matches!(self, Self::Array(_) | Self::Object(_))
    }
}

/// Small JSON grammar reader used instead of a data-only deserializer so the
/// reviver can receive its normative source-text context. It deliberately
/// records duplicate object names in source order: ordinary property creation
/// retains the first key position while the final occurrence supplies both the
/// final value and source text.
struct JsonSourceParser<'a> {
    input: &'a str,
    index: usize,
}

impl<'a> JsonSourceParser<'a> {
    fn new(input: &'a str) -> Self {
        Self { input, index: 0 }
    }

    fn parse(mut self) -> Result<JsonNode, ()> {
        self.skip_whitespace();
        let value = self.value()?;
        self.skip_whitespace();
        (self.index == self.input.len()).then_some(value).ok_or(())
    }

    fn value(&mut self) -> Result<JsonNode, ()> {
        self.skip_whitespace();
        let start = self.index;
        match self.peek() {
            Some(b'{') => self.object(),
            Some(b'[') => self.array(),
            Some(b'"') => {
                let value = Value::String(self.string()?);
                Ok(JsonNode::Primitive {
                    value,
                    source: self.input[start..self.index].into(),
                })
            }
            Some(b't') => self.keyword("true", Value::Bool(true), start),
            Some(b'f') => self.keyword("false", Value::Bool(false), start),
            Some(b'n') => self.keyword("null", Value::Null, start),
            Some(b'-' | b'0'..=b'9') => self.number(start),
            _ => Err(()),
        }
    }

    fn array(&mut self) -> Result<JsonNode, ()> {
        self.expect(b'[')?;
        self.skip_whitespace();
        if self.take(b']') {
            return Ok(JsonNode::Array(Vec::new()));
        }
        let mut values = Vec::new();
        loop {
            values.push(self.value()?);
            self.skip_whitespace();
            if self.take(b']') {
                return Ok(JsonNode::Array(values));
            }
            self.expect(b',')?;
            self.skip_whitespace();
        }
    }

    fn object(&mut self) -> Result<JsonNode, ()> {
        self.expect(b'{')?;
        self.skip_whitespace();
        if self.take(b'}') {
            return Ok(JsonNode::Object(Vec::new()));
        }
        let mut values = Vec::new();
        loop {
            self.skip_whitespace();
            let key = self.string()?;
            self.skip_whitespace();
            self.expect(b':')?;
            let value = self.value()?;
            values.push((key, value));
            self.skip_whitespace();
            if self.take(b'}') {
                return Ok(JsonNode::Object(values));
            }
            self.expect(b',')?;
            self.skip_whitespace();
        }
    }

    fn keyword(&mut self, keyword: &str, value: Value, start: usize) -> Result<JsonNode, ()> {
        if !self.input[self.index..].starts_with(keyword) {
            return Err(());
        }
        self.index += keyword.len();
        Ok(JsonNode::Primitive {
            value,
            source: self.input[start..self.index].into(),
        })
    }

    fn number(&mut self, start: usize) -> Result<JsonNode, ()> {
        self.take(b'-');
        match self.peek() {
            Some(b'0') => self.index += 1,
            Some(b'1'..=b'9') => {
                self.index += 1;
                while matches!(self.peek(), Some(b'0'..=b'9')) {
                    self.index += 1;
                }
            }
            _ => return Err(()),
        }
        if self.take(b'.') {
            let decimal_start = self.index;
            while matches!(self.peek(), Some(b'0'..=b'9')) {
                self.index += 1;
            }
            if self.index == decimal_start {
                return Err(());
            }
        }
        if matches!(self.peek(), Some(b'e' | b'E')) {
            self.index += 1;
            self.take(b'+');
            self.take(b'-');
            let exponent_start = self.index;
            while matches!(self.peek(), Some(b'0'..=b'9')) {
                self.index += 1;
            }
            if self.index == exponent_start {
                return Err(());
            }
        }
        let source: JsString = self.input[start..self.index].into();
        let value = self.input[start..self.index]
            .parse::<f64>()
            .map_err(|_| ())?;
        Ok(JsonNode::Primitive {
            value: Value::Number(value),
            source,
        })
    }

    fn string(&mut self) -> Result<JsString, ()> {
        let start = self.index;
        self.expect(b'"')?;
        loop {
            match self.peek() {
                Some(b'"') => {
                    self.index += 1;
                    let decoded = serde_json::from_str::<String>(&self.input[start..self.index])
                        .map_err(|_| ())?;
                    return Ok(decoded.into());
                }
                Some(b'\\') => {
                    self.index += 1;
                    match self.peek() {
                        Some(b'"' | b'\\' | b'/' | b'b' | b'f' | b'n' | b'r' | b't') => {
                            self.index += 1;
                        }
                        Some(b'u') => {
                            self.index += 1;
                            for _ in 0..4 {
                                if !matches!(
                                    self.peek(),
                                    Some(b'0'..=b'9' | b'a'..=b'f' | b'A'..=b'F')
                                ) {
                                    return Err(());
                                }
                                self.index += 1;
                            }
                        }
                        _ => return Err(()),
                    }
                }
                Some(byte) if byte < 0x20 => return Err(()),
                Some(_) => {
                    let character = self.input[self.index..].chars().next().ok_or(())?;
                    self.index += character.len_utf8();
                }
                None => return Err(()),
            }
        }
    }

    fn skip_whitespace(&mut self) {
        while matches!(self.peek(), Some(b' ' | b'\n' | b'\r' | b'\t')) {
            self.index += 1;
        }
    }

    fn expect(&mut self, byte: u8) -> Result<(), ()> {
        self.take(byte).then_some(()).ok_or(())
    }

    fn take(&mut self, byte: u8) -> bool {
        if self.peek() == Some(byte) {
            self.index += 1;
            true
        } else {
            false
        }
    }

    fn peek(&self) -> Option<u8> {
        self.input.as_bytes().get(self.index).copied()
    }
}

#[derive(Default)]
struct JsonStringifyState {
    replacer: Option<Value>,
    property_list: Option<Vec<JsString>>,
    gap: JsString,
}

fn json_quote(value: &JsString) -> String {
    let mut text = String::from("\"");
    let units = value.as_code_units();
    let mut index = 0;
    while let Some(&unit) = units.get(index) {
        match unit {
            0x08 => text.push_str("\\b"),
            0x09 => text.push_str("\\t"),
            0x0a => text.push_str("\\n"),
            0x0c => text.push_str("\\f"),
            0x0d => text.push_str("\\r"),
            0x22 => text.push_str("\\\""),
            0x5c => text.push_str("\\\\"),
            0x00..=0x1f => text.push_str(&format!("\\u{unit:04x}")),
            0xd800..=0xdbff
                if units
                    .get(index + 1)
                    .is_some_and(|next| (0xdc00..=0xdfff).contains(next)) =>
            {
                text.push(
                    char::from_u32(
                        0x10000
                            + ((u32::from(unit) - 0xd800) << 10)
                            + (u32::from(units[index + 1]) - 0xdc00),
                    )
                    .unwrap(),
                );
                index += 1;
            }
            0xd800..=0xdfff => text.push_str(&format!("\\u{unit:04x}")),
            _ => text.push(char::from_u32(u32::from(unit)).unwrap()),
        }
        index += 1;
    }
    text.push('\"');
    text
}
