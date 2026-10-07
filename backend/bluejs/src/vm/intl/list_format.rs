// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

use super::*;

impl Vm {
    pub(in super::super) fn list_type(
        &mut self,
        options: &Value,
    ) -> Result<blueice_ecma402::ListType, RuntimeError> {
        let value = self.get_property(options, &"type".into())?;
        if value == Value::Undefined {
            return Ok(blueice_ecma402::ListType::Conjunction);
        }
        match self
            .coerce_string(&value)?
            .to_utf8()
            .map_err(|_| RuntimeError::RangeError("invalid type option".into()))?
            .as_str()
        {
            "conjunction" => Ok(blueice_ecma402::ListType::Conjunction),
            "disjunction" => Ok(blueice_ecma402::ListType::Disjunction),
            "unit" => Ok(blueice_ecma402::ListType::Unit),
            _ => Err(RuntimeError::RangeError("invalid type option".into())),
        }
    }

    pub(in super::super) fn list_style(
        &mut self,
        options: &Value,
    ) -> Result<blueice_ecma402::ListStyle, RuntimeError> {
        let value = self.get_property(options, &"style".into())?;
        if value == Value::Undefined {
            return Ok(blueice_ecma402::ListStyle::Wide);
        }
        match self
            .coerce_string(&value)?
            .to_utf8()
            .map_err(|_| RuntimeError::RangeError("invalid style option".into()))?
            .as_str()
        {
            "long" => Ok(blueice_ecma402::ListStyle::Wide),
            "short" => Ok(blueice_ecma402::ListStyle::Short),
            "narrow" => Ok(blueice_ecma402::ListStyle::Narrow),
            _ => Err(RuntimeError::RangeError("invalid style option".into())),
        }
    }

    pub(in super::super) fn resolve_list_format(
        &mut self,
        locales: &Value,
        options: &Value,
    ) -> Result<Rc<intl::ListFormat>, RuntimeError> {
        let locales = self.canonical_locales(locales)?;
        let options = self.intl_constructor_options(options)?;
        let options = blueice_ecma402::ListFormatOptions {
            locale_matcher: self.locale_matcher(&options)?,
            list_type: self.list_type(&options)?,
            style: self.list_style(&options)?,
        };
        blueice_ecma402::ListFormat::try_new(&locales, options)
            .map(Rc::new)
            .map_err(|error| RuntimeError::RangeError(error.to_string()))
    }

    pub(in super::super) fn list_format_supported_locales(
        &mut self,
        args: &[Value],
    ) -> Result<Value, RuntimeError> {
        let locales = self.canonical_locales(native::argument(args, 0))?;
        let options = self.intl_options(native::argument(args, 1))?;
        let matcher = self.locale_matcher(&options)?;
        let locales = blueice_ecma402::supported_list_format_locales(&locales, matcher);
        self.array_from(
            locales
                .iter()
                .map(|locale| Value::String(locale.to_string().into()))
                .collect(),
        )
    }

    pub(in super::super) fn create_list_format(
        &mut self,
        args: &[Value],
        construct: bool,
    ) -> Result<Value, RuntimeError> {
        if !construct {
            return Err(RuntimeError::TypeError(
                "Intl.ListFormat must be called with new".into(),
            ));
        }
        self.intl_global()?;
        let constructor = self.globals["%Intl.ListFormat%"];
        let default = self
            .heap
            .get(constructor, "prototype")?
            .object_id()
            .expect("Intl.ListFormat.prototype is an object");
        let prototype = self.constructor_prototype(default)?;
        self.stack.push(Value::Object(prototype));
        let data =
            self.resolve_list_format(native::argument(args, 0), native::argument(args, 1))?;
        self.with_roots(|heap| heap.alloc_list_format(data, prototype))
            .map(Value::Object)
    }

    pub(in super::super) fn list_format_data(
        &self,
        value: &Value,
    ) -> Result<Rc<intl::ListFormat>, RuntimeError> {
        if let Value::Object(id) = value {
            if let Some(data) = self.heap.list_format(*id)? {
                return Ok(data);
            }
        }
        Err(RuntimeError::TypeError(
            "receiver is not an Intl.ListFormat".into(),
        ))
    }

    pub(in super::super) fn list_format_values(
        &mut self,
        value: &Value,
    ) -> Result<Vec<JsString>, RuntimeError> {
        if *value == Value::Undefined {
            return Ok(Vec::new());
        }
        let base = self.stack.len();
        self.stack.push(value.clone());
        let result = (|| {
            let record = self.get_iterator(value)?;
            self.stack.push(record.clone());
            let outcome = (|| {
                let mut values = Vec::new();
                while let Some(value) = self.iterator_step(&record, true)? {
                    let Value::String(value) = value else {
                        return Err(RuntimeError::TypeError(
                            "Intl.ListFormat list elements must be strings".into(),
                        ));
                    };
                    values.push(value);
                }
                Ok(values)
            })();
            if outcome.is_err() {
                // StringListFromIterable closes the live iterator, preserving
                // the original abrupt completion if close itself fails.
                let _ = self.iterator_close(&record);
            }
            outcome
        })();
        self.stack.truncate(base);
        result
    }

    pub(in super::super) fn list_format_parts(
        &mut self,
        receiver: &Value,
        list: &Value,
    ) -> Result<Vec<(blueice_ecma402::ListPartKind, JsString)>, RuntimeError> {
        let data = self.list_format_data(receiver)?;
        let values = self.list_format_values(list)?;
        // ICU4X operates on Unicode scalar strings, while ECMAScript strings
        // retain lone UTF-16 surrogates. Use a scalar projection only for
        // CLDR pattern selection and put the original values back into every
        // element part, preserving the observable ECMAScript code units.
        let projected = values
            .iter()
            .map(|value| {
                char::decode_utf16(value.as_code_units().iter().copied())
                    .map(|scalar| scalar.unwrap_or(char::REPLACEMENT_CHARACTER))
                    .collect::<String>()
            })
            .collect::<Vec<_>>();
        let parts = data
            .format_to_parts(projected.iter())
            .map_err(|error| RuntimeError::RangeError(error.to_string()))?;
        let mut next_element = 0usize;
        parts
            .into_iter()
            .map(|part| match part.kind {
                blueice_ecma402::ListPartKind::Element => {
                    let value = values.get(next_element).cloned().ok_or_else(|| {
                        RuntimeError::RangeError("invalid ListFormat element data".into())
                    })?;
                    next_element += 1;
                    Ok((part.kind, value))
                }
                blueice_ecma402::ListPartKind::Literal => Ok((part.kind, part.value.into())),
            })
            .collect()
    }

    pub(in super::super) fn list_format_format(
        &mut self,
        receiver: &Value,
        list: &Value,
    ) -> Result<Value, RuntimeError> {
        let parts = self.list_format_parts(receiver, list)?;
        let mut result = JsString::default();
        for (_, part) in parts {
            native::append(&mut result, &part, self.config.max_string_bytes)?;
        }
        Ok(Value::String(result))
    }

    pub(in super::super) fn list_format_format_to_parts(
        &mut self,
        receiver: &Value,
        list: &Value,
    ) -> Result<Value, RuntimeError> {
        let parts = self.list_format_parts(receiver, list)?;
        let prototype = self.object_prototype;
        let base = self.stack.len();
        let result = (|| {
            for (kind, value) in parts {
                let part = self.with_roots(|heap| heap.alloc_object(Some(prototype)))?;
                // Keep every completed part rooted until the result Array has
                // taken ownership. A later part allocation may collect the
                // nursery, so a Rust Vec<Value> alone is insufficient.
                self.stack.push(Value::Object(part));
                self.define_data(
                    part,
                    "type",
                    Value::String(
                        match kind {
                            blueice_ecma402::ListPartKind::Element => "element",
                            blueice_ecma402::ListPartKind::Literal => "literal",
                        }
                        .into(),
                    ),
                    true,
                    true,
                    true,
                )?;
                self.define_data(part, "value", Value::String(value), true, true, true)?;
            }
            self.array_from(self.stack[base..].to_vec())
        })();
        self.stack.truncate(base);
        result
    }

    pub(in super::super) fn list_format_resolved_options(
        &mut self,
        receiver: &Value,
    ) -> Result<Value, RuntimeError> {
        let data = self.list_format_data(receiver)?;
        let resolved = data.resolved_options();
        let prototype = self.object_prototype;
        let result = self.with_roots(|heap| heap.alloc_object(Some(prototype)))?;
        self.stack.push(Value::Object(result));
        for (key, value) in [
            ("locale", Value::String(resolved.locale.as_str().into())),
            (
                "type",
                Value::String(
                    match resolved.list_type {
                        blueice_ecma402::ListType::Conjunction => "conjunction",
                        blueice_ecma402::ListType::Disjunction => "disjunction",
                        blueice_ecma402::ListType::Unit => "unit",
                    }
                    .into(),
                ),
            ),
            (
                "style",
                Value::String(
                    match resolved.style {
                        blueice_ecma402::ListStyle::Wide => "long",
                        blueice_ecma402::ListStyle::Short => "short",
                        blueice_ecma402::ListStyle::Narrow => "narrow",
                    }
                    .into(),
                ),
            ),
        ] {
            self.define_data(result, key, value, true, true, true)?;
        }
        Ok(Value::Object(result))
    }
}
