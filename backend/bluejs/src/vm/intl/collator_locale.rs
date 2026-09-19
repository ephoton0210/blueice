// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

use super::*;

impl Vm {
    pub(in super::super) fn collator_data(
        &self,
        value: &Value,
    ) -> Result<Rc<intl::Collator>, RuntimeError> {
        if let Value::Object(id) = value {
            if let Some(data) = self.heap.collator(*id)? {
                return Ok(data);
            }
        }
        Err(RuntimeError::TypeError(
            "receiver is not an Intl.Collator".into(),
        ))
    }

    pub(in super::super) fn collator_compare_getter(
        &mut self,
        receiver: &Value,
    ) -> Result<Value, RuntimeError> {
        self.collator_data(receiver)?;
        let id = receiver.object_id().unwrap();
        if let Some(function) = self.heap.collator_compare(id) {
            return Ok(Value::Object(function));
        }
        let constructor = self.string_intrinsics()?.0;
        let prototype = self.heap.prototype(constructor)?.unwrap();
        let target = self.with_roots(|heap| {
            heap.alloc_native_function(NativeFunction::CollatorCompare, "", prototype)
        })?;
        let bound = crate::heap::BoundFunction {
            target,
            this: receiver.clone(),
            args: vec![],
            constructible: false,
        };
        let function = self.with_roots(|heap| heap.alloc_bound_function(bound, Some(prototype)))?;
        self.stack.push(Value::Object(function));
        self.define_data(function, "length", Value::Number(2.0), false, false, true)?;
        self.define_data(
            function,
            "name",
            Value::String("".into()),
            false,
            false,
            true,
        )?;
        self.heap.set_collator_compare(id, function);
        Ok(Value::Object(function))
    }

    pub(in super::super) fn collator_resolved_options(
        &mut self,
        receiver: &Value,
    ) -> Result<Value, RuntimeError> {
        let data = self.collator_data(receiver)?;
        let resolved = data.resolved_options();
        let prototype = self.object_prototype;
        let result = self.with_roots(|heap| heap.alloc_object(Some(prototype)))?;
        self.stack.push(Value::Object(result));
        for (key, value) in [
            ("locale", Value::String(resolved.locale.as_str().into())),
            (
                "usage",
                Value::String(
                    match resolved.usage {
                        blueice_ecma402::CollatorUsage::Sort => "sort",
                        blueice_ecma402::CollatorUsage::Search => "search",
                    }
                    .into(),
                ),
            ),
            (
                "sensitivity",
                Value::String(
                    match resolved.sensitivity {
                        blueice_ecma402::Sensitivity::Base => "base",
                        blueice_ecma402::Sensitivity::Accent => "accent",
                        blueice_ecma402::Sensitivity::Case => "case",
                        blueice_ecma402::Sensitivity::Variant => "variant",
                    }
                    .into(),
                ),
            ),
            (
                "ignorePunctuation",
                Value::Bool(resolved.ignore_punctuation),
            ),
            (
                "collation",
                Value::String(resolved.collation.as_str().into()),
            ),
            ("numeric", Value::Bool(resolved.numeric)),
            (
                "caseFirst",
                Value::String(
                    match resolved.case_first {
                        blueice_ecma402::CaseFirst::Upper => "upper",
                        blueice_ecma402::CaseFirst::Lower => "lower",
                        blueice_ecma402::CaseFirst::False => "false",
                    }
                    .into(),
                ),
            ),
        ] {
            self.define_data(result, key, value, true, true, true)?;
        }
        Ok(Value::Object(result))
    }

    pub(in super::super) fn locale_unicode_option(
        &mut self,
        options: &Value,
        name: &str,
    ) -> Result<Option<UnicodeValue>, RuntimeError> {
        let Some(value) = self.string_option(options, name, &[])? else {
            return Ok(None);
        };
        if value.is_empty()
            || !value.split('-').all(|part| {
                (3..=8).contains(&part.len())
                    && part.bytes().all(|byte| byte.is_ascii_alphanumeric())
            })
        {
            return Err(RuntimeError::RangeError(format!("invalid {name} option")));
        }
        Ok(Some(UnicodeValue::try_from_str(&value).unwrap()))
    }

    pub(in super::super) fn locale_set_keyword(
        &mut self,
        locale: &mut Locale,
        options: &Value,
        name: &str,
        key: &str,
    ) -> Result<(), RuntimeError> {
        if let Some(value) = self.locale_unicode_option(options, name)? {
            locale
                .extensions
                .unicode
                .keywords
                .set(key.parse().unwrap(), value);
        }
        Ok(())
    }

    pub(in super::super) fn locale_first_day_of_week(
        &mut self,
        options: &Value,
    ) -> Result<Option<UnicodeValue>, RuntimeError> {
        let Some(value) = self.string_option(options, "firstDayOfWeek", &[])? else {
            return Ok(None);
        };
        let value = match value.as_str() {
            "0" | "7" => "sun",
            "1" => "mon",
            "2" => "tue",
            "3" => "wed",
            "4" => "thu",
            "5" => "fri",
            "6" => "sat",
            value => value,
        };
        if value.is_empty()
            || !value.split('-').all(|part| {
                (3..=8).contains(&part.len())
                    && part.bytes().all(|byte| byte.is_ascii_alphanumeric())
            })
        {
            return Err(RuntimeError::RangeError(
                "invalid firstDayOfWeek option".into(),
            ));
        }
        Ok(Some(UnicodeValue::try_from_str(value).unwrap()))
    }

    pub(in super::super) fn locale_instance(
        &mut self,
        locale: intl::CanonicalLocale,
        prototype: ObjectId,
    ) -> Result<Value, RuntimeError> {
        self.with_roots(|heap| heap.alloc_intl_locale(Rc::new(intl::Locale { locale }), prototype))
            .map(Value::Object)
    }

    pub(in super::super) fn create_locale(
        &mut self,
        args: &[Value],
        construct: bool,
    ) -> Result<Value, RuntimeError> {
        if !construct {
            return Err(RuntimeError::TypeError(
                "Intl.Locale must be called with new".into(),
            ));
        }
        self.intl_global()?;
        let tag = native::argument(args, 0);
        let initial_locale = match tag {
            Value::String(string) => intl::canonicalize(string)?,
            Value::Object(id) => {
                if let Some(locale) = self.heap.intl_locale(*id)? {
                    intl::CanonicalLocale::from(locale.as_ref())
                } else {
                    intl::canonicalize(&self.coerce_string(tag)?)?
                }
            }
            _ => {
                return Err(RuntimeError::TypeError(
                    "locale tag must be a String or Object".into(),
                ))
            }
        };
        let (mut locale, initial_name) = initial_locale.into_parts();
        let options = self.intl_options(native::argument(args, 1))?;

        if let Some(language) = self.string_option(&options, "language", &[])? {
            locale.id.language = language
                .parse::<Language>()
                .map_err(|_| RuntimeError::RangeError("invalid language option".into()))?;
        }
        if let Some(script) = self.string_option(&options, "script", &[])? {
            locale.id.script = Some(
                script
                    .parse::<Script>()
                    .map_err(|_| RuntimeError::RangeError("invalid script option".into()))?,
            );
        }
        if let Some(region) = self.string_option(&options, "region", &[])? {
            locale.id.region = Some(
                region
                    .parse::<Region>()
                    .map_err(|_| RuntimeError::RangeError("invalid region option".into()))?,
            );
        }
        if let Some(variants) = self.string_option(&options, "variants", &[])? {
            if variants.is_empty() {
                return Err(RuntimeError::RangeError("invalid variants option".into()));
            }
            let mut parsed = variants
                .split('-')
                .map(|value| {
                    value
                        .parse::<Variant>()
                        .map_err(|_| RuntimeError::RangeError("invalid variants option".into()))
                })
                .collect::<Result<Vec<_>, _>>()?;
            parsed.sort();
            if parsed.windows(2).any(|values| values[0] == values[1]) {
                return Err(RuntimeError::RangeError("duplicate variant option".into()));
            }
            locale.id.variants = Variants::from_vec_unchecked(parsed);
        }
        self.locale_set_keyword(&mut locale, &options, "calendar", "ca")?;
        self.locale_set_keyword(&mut locale, &options, "collation", "co")?;
        if let Some(hour_cycle) =
            self.string_option(&options, "hourCycle", &["h11", "h12", "h23", "h24"])?
        {
            locale.extensions.unicode.keywords.set(
                "hc".parse().unwrap(),
                UnicodeValue::try_from_str(&hour_cycle).unwrap(),
            );
        }
        if let Some(case_first) =
            self.string_option(&options, "caseFirst", &["upper", "lower", "false"])?
        {
            locale.extensions.unicode.keywords.set(
                "kf".parse().unwrap(),
                UnicodeValue::try_from_str(&case_first).unwrap(),
            );
        }
        let numeric = self.get_property(&options, &"numeric".into())?;
        if numeric != Value::Undefined {
            let value = if self.to_boolean(&numeric)? {
                UnicodeValue::default()
            } else {
                UnicodeValue::try_from_str("false").unwrap()
            };
            locale
                .extensions
                .unicode
                .keywords
                .set("kn".parse().unwrap(), value);
        }
        self.locale_set_keyword(&mut locale, &options, "numberingSystem", "nu")?;
        if let Some(value) = self.locale_first_day_of_week(&options)? {
            locale
                .extensions
                .unicode
                .keywords
                .set("fw".parse().unwrap(), value);
        }

        // ApplyOptionsToTag canonicalizes once after language-id updates and
        // once after Unicode keyword insertion, so aliases in either source
        // have the same observable result.
        let serialized = locale.to_string();
        let locale = if initial_name == "posix" && serialized == "und-posix" {
            intl::CanonicalLocale::from_parts(locale, initial_name)
        } else {
            intl::canonicalize(&JsString::from(serialized))?
        };
        let constructor = self.globals["%Intl.Locale%"];
        let default = self
            .heap
            .get(constructor, "prototype")?
            .object_id()
            .unwrap();
        let prototype = self.constructor_prototype(default)?;
        self.stack.push(Value::Object(prototype));
        let result = self.locale_instance(locale, prototype);
        self.stack.pop();
        result
    }

    pub(in super::super) fn locale_data(
        &self,
        receiver: &Value,
    ) -> Result<Rc<intl::Locale>, RuntimeError> {
        let id = receiver.object_id().ok_or(RuntimeError::TypeError(
            "receiver is not an Intl.Locale".into(),
        ))?;
        self.heap.intl_locale(id)?.ok_or(RuntimeError::TypeError(
            "receiver is not an Intl.Locale".into(),
        ))
    }

    pub(in super::super) fn locale_to_string(
        &self,
        receiver: &Value,
    ) -> Result<Value, RuntimeError> {
        Ok(Value::String(
            self.locale_data(receiver)?.locale.as_str().into(),
        ))
    }

    pub(in super::super) fn locale_transform(
        &mut self,
        receiver: &Value,
        maximize: bool,
    ) -> Result<Value, RuntimeError> {
        let data = self.locale_data(receiver)?;
        if data.locale.as_str() == "posix" {
            self.intl_global()?;
            let constructor = self.globals["%Intl.Locale%"];
            let prototype = self
                .heap
                .get(constructor, "prototype")?
                .object_id()
                .unwrap();
            return self.locale_instance(intl::CanonicalLocale::from(data.as_ref()), prototype);
        }
        let locale = if maximize {
            blueice_ecma402::maximize_locale(&data.locale)
        } else {
            blueice_ecma402::minimize_locale(&data.locale)
        };
        self.intl_global()?;
        let constructor = self.globals["%Intl.Locale%"];
        let prototype = self
            .heap
            .get(constructor, "prototype")?
            .object_id()
            .unwrap();
        self.locale_instance(locale, prototype)
    }

    pub(in super::super) fn locale_getter(
        &self,
        receiver: &Value,
        name: native::LocaleGetter,
    ) -> Result<Value, RuntimeError> {
        let data = self.locale_data(receiver)?;
        let locale = data.locale.locale();
        let value = match name {
            native::LocaleGetter::BaseName => Value::String(locale.id.to_string().into()),
            native::LocaleGetter::Language => Value::String(locale.id.language.to_string().into()),
            native::LocaleGetter::Script => locale
                .id
                .script
                .map(|value| Value::String(value.to_string().into()))
                .unwrap_or(Value::Undefined),
            native::LocaleGetter::Region => locale
                .id
                .region
                .map(|value| Value::String(value.to_string().into()))
                .unwrap_or(Value::Undefined),
            native::LocaleGetter::Variants => {
                let variants = locale.id.variants.to_string();
                if variants.is_empty() {
                    Value::Undefined
                } else {
                    Value::String(variants.into())
                }
            }
            native::LocaleGetter::Calendar => intl::keyword(locale, "ca")
                .map(|value| Value::String(value.into()))
                .unwrap_or(Value::Undefined),
            native::LocaleGetter::Collation => intl::keyword(locale, "co")
                .map(|value| Value::String(value.into()))
                .unwrap_or(Value::Undefined),
            native::LocaleGetter::HourCycle => intl::keyword(locale, "hc")
                .map(|value| Value::String(value.into()))
                .unwrap_or(Value::Undefined),
            native::LocaleGetter::CaseFirst => intl::keyword(locale, "kf")
                .map(|value| Value::String(value.into()))
                .unwrap_or(Value::Undefined),
            native::LocaleGetter::Numeric => {
                Value::Bool(intl::keyword(locale, "kn").is_some_and(|value| value != "false"))
            }
            native::LocaleGetter::NumberingSystem => intl::keyword(locale, "nu")
                .map(|value| Value::String(value.into()))
                .unwrap_or(Value::Undefined),
            native::LocaleGetter::FirstDayOfWeek => intl::keyword(locale, "fw")
                .map(|value| Value::String(value.into()))
                .unwrap_or(Value::Undefined),
        };
        Ok(value)
    }

    pub(in super::super) fn locale_info_array(
        &mut self,
        values: Vec<String>,
    ) -> Result<Value, RuntimeError> {
        self.array_from(
            values
                .into_iter()
                .map(|value| Value::String(value.into()))
                .collect(),
        )
    }

    pub(in super::super) fn locale_info(
        &mut self,
        receiver: &Value,
        operation: native::LocaleInfo,
    ) -> Result<Value, RuntimeError> {
        let locale = self.locale_data(receiver)?.locale.clone();
        let information = blueice_ecma402::locale_information(&locale);
        match operation {
            native::LocaleInfo::Calendars => self.locale_info_array(information.calendars),
            native::LocaleInfo::Collations => self.locale_info_array(information.collations),
            native::LocaleInfo::HourCycles => self.locale_info_array(information.hour_cycles),
            native::LocaleInfo::NumberingSystems => {
                self.locale_info_array(information.numbering_systems)
            }
            native::LocaleInfo::TextInfo => {
                let prototype = self.object_prototype;
                let result = self.with_roots(|heap| heap.alloc_object(Some(prototype)))?;
                self.define_data(
                    result,
                    "direction",
                    Value::String(
                        match information.text_direction {
                            blueice_ecma402::TextDirection::LeftToRight => "ltr",
                            blueice_ecma402::TextDirection::RightToLeft => "rtl",
                        }
                        .into(),
                    ),
                    true,
                    true,
                    true,
                )?;
                Ok(Value::Object(result))
            }
            native::LocaleInfo::TimeZones => {
                let Some(time_zones) = information.time_zones else {
                    return Ok(Value::Undefined);
                };
                self.locale_info_array(time_zones)
            }
            native::LocaleInfo::WeekInfo => {
                let prototype = self.object_prototype;
                let result = self.with_roots(|heap| heap.alloc_object(Some(prototype)))?;
                self.stack.push(Value::Object(result));
                self.define_data(
                    result,
                    "firstDay",
                    Value::Number(f64::from(information.week_info.first_day)),
                    true,
                    true,
                    true,
                )?;
                let weekend = self.array_from(
                    information
                        .week_info
                        .weekend
                        .into_iter()
                        .map(|day| Value::Number(f64::from(day)))
                        .collect(),
                )?;
                self.define_data(result, "weekend", weekend, true, true, true)?;
                self.stack.pop();
                Ok(Value::Object(result))
            }
        }
    }
}
