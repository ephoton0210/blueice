// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

//! JSON's data-only parse/stringify paths. Replacer/reviver and raw JSON are
//! separate APIs; these operations preserve ordinary property enumeration,
//! getters and JSON's finite-number restriction.

use super::*;

impl Vm {
    pub(super) fn json_global(&mut self) -> Result<Value, RuntimeError> {
        if let Some(&id) = self.globals.get("JSON") {
            return Ok(Value::Object(id));
        }
        let function_prototype = self.string_intrinsics()?.1;
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

    pub(super) fn json_parse(&mut self, input: &Value) -> Result<Value, RuntimeError> {
        let input = self.coerce_string(input)?;
        let input = input
            .to_utf8()
            .map_err(|_| RuntimeError::SyntaxError("invalid JSON text".into()))?;
        let value = serde_json::from_str(&input)
            .map_err(|_| RuntimeError::SyntaxError("invalid JSON text".into()))?;
        self.json_from_serde(value)
    }

    fn json_from_serde(&mut self, value: serde_json::Value) -> Result<Value, RuntimeError> {
        Ok(match value {
            serde_json::Value::Null => Value::Null,
            serde_json::Value::Bool(value) => Value::Bool(value),
            serde_json::Value::Number(value) => Value::Number(value.as_f64().unwrap_or(f64::NAN)),
            serde_json::Value::String(value) => {
                let value = Value::String(value.into());
                self.check_string(&value)?;
                value
            }
            serde_json::Value::Array(values) => {
                let prototype = self.array_prototype;
                let array =
                    self.with_roots(|heap| heap.alloc_array(values.len() as u32, Some(prototype)))?;
                self.stack.push(Value::Object(array));
                let result: Result<Value, RuntimeError> = (|| {
                    for (index, value) in values.into_iter().enumerate() {
                        self.charge_step()?;
                        let value = self.json_from_serde(value)?;
                        self.stack.push(value.clone());
                        self.with_roots(|heap| heap.set(array, index.to_string(), value))?;
                        self.stack.pop();
                    }
                    Ok(Value::Object(array))
                })();
                self.stack.pop();
                result?
            }
            serde_json::Value::Object(values) => {
                let prototype = self.object_prototype;
                let object = self.with_roots(|heap| heap.alloc_object(Some(prototype)))?;
                self.stack.push(Value::Object(object));
                let result: Result<Value, RuntimeError> = (|| {
                    for (key, value) in values {
                        self.charge_step()?;
                        let value = self.json_from_serde(value)?;
                        self.stack.push(value.clone());
                        self.with_roots(|heap| heap.set(object, key, value))?;
                        self.stack.pop();
                    }
                    Ok(Value::Object(object))
                })();
                self.stack.pop();
                result?
            }
        })
    }

    pub(super) fn json_stringify(&mut self, value: &Value) -> Result<Value, RuntimeError> {
        match self.json_serialize(value)? {
            Some(value) => {
                self.check_string(&Value::String(value.clone()))?;
                Ok(Value::String(value))
            }
            None => Ok(Value::Undefined),
        }
    }

    fn json_serialize(&mut self, value: &Value) -> Result<Option<JsString>, RuntimeError> {
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
                if self.is_callable(value)? {
                    return Ok(None);
                }
                if self.joining.contains(object) {
                    return Err(RuntimeError::TypeError("cyclic JSON value".into()));
                }
                self.joining.push(*object);
                let result = if self.heap.is_array(*object)? {
                    self.json_array(*object)
                } else {
                    self.json_object(*object)
                };
                self.joining.pop();
                result.map(Some)
            }
        }
    }

    fn json_array(&mut self, array: ObjectId) -> Result<JsString, RuntimeError> {
        let length = self.get_property(&Value::Object(array), &"length".into())?;
        let length = self.coerce_length(&length)? as u64;
        let mut text = String::from("[");
        for index in 0..length {
            self.charge_step()?;
            if index != 0 {
                text.push(',');
            }
            let value = self.get_property(&Value::Object(array), &index.to_string().into())?;
            let value = self
                .json_serialize(&value)?
                .unwrap_or_else(|| "null".into());
            text.push_str(&value.to_utf8().expect("JSON serialization is well-formed"));
        }
        text.push(']');
        Ok(text.into())
    }

    fn json_object(&mut self, object: ObjectId) -> Result<JsString, RuntimeError> {
        let mut text = String::from("{");
        let mut first = true;
        for key in self.heap.own_property_keys(object)? {
            self.charge_step()?;
            let PropertyName::String(key) = key else {
                continue;
            };
            if self
                .heap
                .get_own_property_descriptor(object, &key)?
                .is_none_or(|descriptor| descriptor.enumerable != Some(true))
            {
                continue;
            }
            let value = self.get_property(&Value::Object(object), &PropertyName::from(&key))?;
            let Some(value) = self.json_serialize(&value)? else {
                continue;
            };
            if !first {
                text.push(',');
            }
            first = false;
            text.push_str(&json_quote(&key));
            text.push(':');
            text.push_str(&value.to_utf8().expect("JSON serialization is well-formed"));
        }
        text.push('}');
        Ok(text.into())
    }
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
