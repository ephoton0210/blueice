// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

use super::*;

impl Vm {
    pub(in super::super) fn segmenter_internal_prototypes(
        &mut self,
    ) -> Result<(ObjectId, ObjectId), RuntimeError> {
        if let (Some(&segments), Some(&iterator)) = (
            self.globals.get("%Intl.SegmentsPrototype%"),
            self.globals.get("%Intl.SegmentIteratorPrototype%"),
        ) {
            return Ok((segments, iterator));
        }
        let string = self.string_intrinsics()?.0;
        let function_prototype = self.heap.prototype(string)?.unwrap();
        let object_prototype = self.object_prototype;
        let segments = self.with_roots(|heap| heap.alloc_object(Some(object_prototype)))?;
        let segments_root = self.heap.root(segments)?;
        let iterator_base = self.base_iterator_prototype()?;
        let iterator = self.with_roots(|heap| heap.alloc_object(Some(iterator_base)))?;
        let iterator_root = self.heap.root(iterator)?;
        let result = (|| {
            self.install_native(
                segments,
                function_prototype,
                "containing",
                1,
                NativeFunction::SegmentsContaining,
            )?;
            self.install_symbol_native(
                segments,
                function_prototype,
                "iterator",
                0,
                NativeFunction::SegmentsIterator,
            )?;
            self.install_native(
                iterator,
                function_prototype,
                "next",
                0,
                NativeFunction::SegmentIteratorNext,
            )?;
            self.define_data(
                iterator,
                JsSymbol::well_known("toStringTag"),
                Value::String("Segmenter String Iterator".into()),
                false,
                false,
                true,
            )?;
            self.globals
                .insert("%Intl.SegmentsPrototype%".into(), segments);
            self.globals
                .insert("%Intl.SegmentIteratorPrototype%".into(), iterator);
            Ok((segments, iterator))
        })();
        if result.is_err() {
            self.heap.unroot(segments_root)?;
            self.heap.unroot(iterator_root)?;
        }
        result
    }

    pub(in super::super) fn canonical_locales(
        &mut self,
        locales: &Value,
    ) -> Result<Vec<intl::CanonicalLocale>, RuntimeError> {
        if *locales == Value::Undefined {
            return Ok(Vec::new());
        }
        if let Value::String(string) = locales {
            return Ok(vec![intl::canonicalize(string)?]);
        }
        if let Value::Object(id) = locales {
            if let Some(locale) = self.heap.intl_locale(*id)? {
                return Ok(vec![intl::CanonicalLocale::from(locale.as_ref())]);
            }
        }
        let object = self.coerce_object(locales)?;
        self.stack.push(Value::Object(object));
        let length = self.get_property(&Value::Object(object), &"length".into())?;
        let length = self.coerce_length(&length)? as u64;
        let mut result = Vec::new();
        for index in 0..length {
            self.charge_step()?;
            let key: PropertyName = index.to_string().into();
            if !self.has_property(object, &key)? {
                continue;
            }
            let value = self.get_property(&Value::Object(object), &key)?;
            if !matches!(value, Value::Object(_) | Value::String(_)) {
                return Err(RuntimeError::TypeError(
                    "locale must be a String or Object".into(),
                ));
            }
            let locale = if let Value::Object(id) = value {
                if let Some(locale) = self.heap.intl_locale(id)? {
                    intl::CanonicalLocale::from(locale.as_ref())
                } else {
                    intl::canonicalize(&self.coerce_string(&Value::Object(id))?)?
                }
            } else {
                intl::canonicalize(&self.coerce_string(&value)?)?
            };
            if !result.contains(&locale) {
                result.push(locale);
            }
        }
        Ok(result)
    }

    pub(in super::super) fn intl_options(&mut self, value: &Value) -> Result<Value, RuntimeError> {
        let object = if *value == Value::Undefined {
            self.with_roots(|heap| heap.alloc_object(None))?
        } else {
            self.coerce_object(value)?
        };
        let result = Value::Object(object);
        self.stack.push(result.clone());
        Ok(result)
    }

    /// The legacy constructor algorithms use `GetOptionsObject`: only an
    /// ordinary object is accepted when an options value is supplied.
    pub(in super::super) fn intl_constructor_options(
        &mut self,
        value: &Value,
    ) -> Result<Value, RuntimeError> {
        let object = match value {
            Value::Undefined => self.with_roots(|heap| heap.alloc_object(None))?,
            Value::Object(object) => *object,
            _ => {
                return Err(RuntimeError::TypeError(
                    "Intl constructor options must be an object".into(),
                ));
            }
        };
        let result = Value::Object(object);
        self.stack.push(result.clone());
        Ok(result)
    }

    /// `InitializeNumberFormat` uses current ECMA-402's
    /// `CoerceOptionsToObject`, so primitives are boxed and can expose
    /// observable inherited properties. `null` still fails `ToObject`.
    pub(in super::super) fn number_format_constructor_options(
        &mut self,
        value: &Value,
    ) -> Result<Value, RuntimeError> {
        self.intl_options(value)
    }

    pub(in super::super) fn string_option(
        &mut self,
        options: &Value,
        name: &str,
        allowed: &[&str],
    ) -> Result<Option<String>, RuntimeError> {
        let value = self.get_property(options, &name.into())?;
        if value == Value::Undefined {
            return Ok(None);
        }
        let string = self.coerce_string(&value)?;
        let string = string
            .to_utf8()
            .map_err(|_| RuntimeError::RangeError(format!("invalid {name} option")))?;
        if !allowed.is_empty() && !allowed.contains(&string.as_str()) {
            return Err(RuntimeError::RangeError(format!("invalid {name} option")));
        }
        Ok(Some(string))
    }

    pub(in super::super) fn supported_locales(
        &mut self,
        args: &[Value],
    ) -> Result<Value, RuntimeError> {
        let locales = self.canonical_locales(native::argument(args, 0))?;
        let options = self.intl_options(native::argument(args, 1))?;
        let matcher = self.locale_matcher(&options)?;
        let locales = blueice_ecma402::supported_collation_locales(&locales, matcher);
        self.array_from(
            locales
                .iter()
                .map(|l| Value::String(l.to_string().into()))
                .collect(),
        )
    }

    pub(in super::super) fn supported_values_of(
        &mut self,
        key: &Value,
    ) -> Result<Value, RuntimeError> {
        let key = self.coerce_string(key)?;
        let key = key
            .to_utf8()
            .map_err(|_| RuntimeError::RangeError("invalid Intl.supportedValuesOf key".into()))?;
        let values = blueice_ecma402::supported_values_of(&key)
            .map_err(|error| RuntimeError::RangeError(error.to_string()))?;
        self.array_from(
            values
                .iter()
                .map(|value| Value::String(value.as_str().into()))
                .collect(),
        )
    }

    pub(in super::super) fn locale_matcher(
        &mut self,
        options: &Value,
    ) -> Result<blueice_ecma402::LocaleMatcher, RuntimeError> {
        Ok(
            match self
                .string_option(options, "localeMatcher", &["lookup", "best fit"])?
                .as_deref()
            {
                Some("best fit") => blueice_ecma402::LocaleMatcher::BestFit,
                Some("lookup") | None => blueice_ecma402::LocaleMatcher::Lookup,
                Some(_) => unreachable!("string_option validates localeMatcher"),
            },
        )
    }

    pub(in super::super) fn resolve_collator(
        &mut self,
        locales: &Value,
        options: &Value,
    ) -> Result<Rc<intl::Collator>, RuntimeError> {
        let locales = self.canonical_locales(locales)?;
        let options = self.intl_options(options)?;
        let usage = self
            .string_option(&options, "usage", &["sort", "search"])?
            .unwrap_or_else(|| "sort".into());
        let locale_matcher = self.locale_matcher(&options)?;
        let collation = self.string_option(&options, "collation", &[])?;
        if let Some(collation) = &collation {
            if !collation.split('-').all(|part| {
                (3..=8).contains(&part.len()) && part.bytes().all(|b| b.is_ascii_alphanumeric())
            }) {
                return Err(RuntimeError::RangeError("invalid collation option".into()));
            }
        }
        let numeric = self.get_property(&options, &"numeric".into())?;
        let numeric = if numeric == Value::Undefined {
            None
        } else {
            Some(self.to_boolean(&numeric)?)
        };
        let case_first = self.string_option(&options, "caseFirst", &["upper", "lower", "false"])?;
        let sensitivity = self
            .string_option(
                &options,
                "sensitivity",
                &["base", "accent", "case", "variant"],
            )?
            .unwrap_or_else(|| "variant".into());
        let punctuation = self.get_property(&options, &"ignorePunctuation".into())?;
        let ignore_punctuation = if punctuation == Value::Undefined {
            None
        } else {
            Some(self.to_boolean(&punctuation)?)
        };
        let options = blueice_ecma402::CollatorOptions {
            locale_matcher,
            usage: match usage.as_str() {
                "sort" => blueice_ecma402::CollatorUsage::Sort,
                "search" => blueice_ecma402::CollatorUsage::Search,
                _ => unreachable!("string_option validates usage"),
            },
            collation,
            numeric,
            case_first: case_first.map(|value| match value.as_str() {
                "upper" => blueice_ecma402::CaseFirst::Upper,
                "lower" => blueice_ecma402::CaseFirst::Lower,
                "false" => blueice_ecma402::CaseFirst::False,
                _ => unreachable!("string_option validates caseFirst"),
            }),
            sensitivity: match sensitivity.as_str() {
                "base" => blueice_ecma402::Sensitivity::Base,
                "accent" => blueice_ecma402::Sensitivity::Accent,
                "case" => blueice_ecma402::Sensitivity::Case,
                "variant" => blueice_ecma402::Sensitivity::Variant,
                _ => unreachable!("string_option validates sensitivity"),
            },
            ignore_punctuation,
        };
        blueice_ecma402::Collator::try_new(&locales, options)
            .map(Rc::new)
            .map_err(|_| RuntimeError::Unsupported("unavailable ICU collation data"))
    }

    pub(in super::super) fn create_collator(
        &mut self,
        args: &[Value],
        construct: bool,
    ) -> Result<Value, RuntimeError> {
        self.intl_global()?;
        let constructor = self.globals["%Intl.Collator%"];
        let default = self
            .heap
            .get(constructor, "prototype")?
            .object_id()
            .unwrap();
        let prototype = if construct {
            self.constructor_prototype(default)?
        } else {
            default
        };
        self.stack.push(Value::Object(prototype));
        let data = self.resolve_collator(native::argument(args, 0), native::argument(args, 1))?;
        self.with_roots(|heap| heap.alloc_collator(data, prototype))
            .map(Value::Object)
    }

    pub(in super::super) fn create_intl_service(
        &mut self,
        service: native::IntlService,
        receiver: &Value,
        args: &[Value],
        construct: bool,
    ) -> Result<Value, RuntimeError> {
        if service == native::IntlService::Number {
            return self.create_number_format(receiver, args, construct);
        }
        if service == native::IntlService::DateTime {
            return self.create_date_time_format(receiver, args, construct);
        }
        if service == native::IntlService::List {
            return self.create_list_format(args, construct);
        }
        if service == native::IntlService::Plural {
            return self.create_plural_rules(args, construct);
        }
        if service == native::IntlService::DisplayNames {
            return self.create_display_names(args, construct);
        }
        if service == native::IntlService::Duration {
            return self.create_duration_format(args, construct);
        }
        if service == native::IntlService::RelativeTime {
            return self.create_relative_time_format(args, construct);
        }
        if service == native::IntlService::Segmenter {
            return self.create_segmenter(args, construct);
        }
        self.intl_global()?;
        let name = match service {
            native::IntlService::Number => "NumberFormat",
            native::IntlService::DateTime => "DateTimeFormat",
            native::IntlService::DisplayNames => "DisplayNames",
            native::IntlService::Duration => "DurationFormat",
            native::IntlService::List => "ListFormat",
            native::IntlService::Plural => "PluralRules",
            native::IntlService::RelativeTime => "RelativeTimeFormat",
            native::IntlService::Segmenter => "Segmenter",
        };
        let namespace = self.globals["Intl"];
        let constructor = self.heap.get(namespace, name)?.object_id().unwrap();
        let default = self
            .heap
            .get(constructor, "prototype")?
            .object_id()
            .expect("Intl service prototype is an object");
        let prototype = if construct {
            self.constructor_prototype(default)?
        } else {
            default
        };
        self.with_roots(|heap| heap.alloc_object(Some(prototype)))
            .map(Value::Object)
    }

    pub(in super::super) fn resolve_display_names(
        &mut self,
        locales: &Value,
        options: &Value,
    ) -> Result<Rc<intl::DisplayNames>, RuntimeError> {
        let locales = self.canonical_locales(locales)?;
        let options = self.intl_constructor_options(options)?;
        let locale_matcher = self.locale_matcher(&options)?;
        let style = match self
            .string_option(&options, "style", &["narrow", "short", "long"])?
            .as_deref()
        {
            None | Some("long") => blueice_ecma402::DisplayNamesStyle::Long,
            Some("short") => blueice_ecma402::DisplayNamesStyle::Short,
            Some("narrow") => blueice_ecma402::DisplayNamesStyle::Narrow,
            Some(_) => unreachable!("string_option validates DisplayNames style"),
        };
        let display_type = match self
            .string_option(
                &options,
                "type",
                &[
                    "language",
                    "region",
                    "script",
                    "currency",
                    "calendar",
                    "dateTimeField",
                ],
            )?
            .as_deref()
        {
            Some("language") => blueice_ecma402::DisplayNamesType::Language,
            Some("region") => blueice_ecma402::DisplayNamesType::Region,
            Some("script") => blueice_ecma402::DisplayNamesType::Script,
            Some("currency") => blueice_ecma402::DisplayNamesType::Currency,
            Some("calendar") => blueice_ecma402::DisplayNamesType::Calendar,
            Some("dateTimeField") => blueice_ecma402::DisplayNamesType::DateTimeField,
            None => {
                return Err(RuntimeError::TypeError(
                    "Intl.DisplayNames requires a type option".into(),
                ));
            }
            Some(_) => unreachable!("string_option validates DisplayNames type"),
        };
        let fallback = match self
            .string_option(&options, "fallback", &["code", "none"])?
            .as_deref()
        {
            None | Some("code") => blueice_ecma402::DisplayNamesFallback::Code,
            Some("none") => blueice_ecma402::DisplayNamesFallback::None,
            Some(_) => unreachable!("string_option validates DisplayNames fallback"),
        };
        let language_display = match self
            .string_option(&options, "languageDisplay", &["dialect", "standard"])?
            .as_deref()
        {
            None | Some("dialect") => blueice_ecma402::DisplayNamesLanguageDisplay::Dialect,
            Some("standard") => blueice_ecma402::DisplayNamesLanguageDisplay::Standard,
            Some(_) => unreachable!("string_option validates DisplayNames languageDisplay"),
        };
        blueice_ecma402::DisplayNames::try_new(
            &locales,
            blueice_ecma402::DisplayNamesOptions {
                locale_matcher,
                display_type,
                style,
                fallback,
                language_display,
            },
        )
        .map(Rc::new)
        .map_err(|error| RuntimeError::RangeError(error.to_string()))
    }

    pub(in super::super) fn create_display_names(
        &mut self,
        args: &[Value],
        construct: bool,
    ) -> Result<Value, RuntimeError> {
        if !construct {
            return Err(RuntimeError::TypeError(
                "Intl.DisplayNames must be called with new".into(),
            ));
        }
        self.intl_global()?;
        let constructor = self.globals["%Intl.DisplayNames%"];
        let default = self
            .heap
            .get(constructor, "prototype")?
            .object_id()
            .expect("Intl.DisplayNames.prototype is an object");
        let prototype = self.constructor_prototype(default)?;
        self.stack.push(Value::Object(prototype));
        let data =
            self.resolve_display_names(native::argument(args, 0), native::argument(args, 1))?;
        self.with_roots(|heap| heap.alloc_display_names(data, prototype))
            .map(Value::Object)
    }

    pub(in super::super) fn display_names_supported_locales(
        &mut self,
        args: &[Value],
    ) -> Result<Value, RuntimeError> {
        let locales = self.canonical_locales(native::argument(args, 0))?;
        let options = self.intl_options(native::argument(args, 1))?;
        let matcher = self.locale_matcher(&options)?;
        let locales = blueice_ecma402::supported_display_names_locales(&locales, matcher);
        self.array_from(
            locales
                .iter()
                .map(|locale| Value::String(locale.to_string().into()))
                .collect(),
        )
    }

    pub(in super::super) fn display_names_data(
        &self,
        value: &Value,
    ) -> Result<Rc<intl::DisplayNames>, RuntimeError> {
        if let Value::Object(id) = value {
            if let Some(data) = self.heap.display_names(*id)? {
                return Ok(data);
            }
        }
        Err(RuntimeError::TypeError(
            "receiver is not an Intl.DisplayNames".into(),
        ))
    }

    pub(in super::super) fn display_names_of(
        &mut self,
        receiver: &Value,
        code: &Value,
    ) -> Result<Value, RuntimeError> {
        let data = self.display_names_data(receiver)?;
        let code = self.coerce_string(code)?;
        let code = code
            .to_utf8()
            .map_err(|_| RuntimeError::RangeError("invalid display-name code".into()))?;
        data.of(&code)
            .map(|name| name.map_or(Value::Undefined, |name| Value::String(name.into())))
            .map_err(|error| RuntimeError::RangeError(error.to_string()))
    }

    pub(in super::super) fn display_names_resolved_options(
        &mut self,
        receiver: &Value,
    ) -> Result<Value, RuntimeError> {
        let data = self.display_names_data(receiver)?;
        let resolved = data.resolved_options();
        let prototype = self.object_prototype;
        let result = self.with_roots(|heap| heap.alloc_object(Some(prototype)))?;
        self.stack.push(Value::Object(result));
        let style = match resolved.style {
            blueice_ecma402::DisplayNamesStyle::Long => "long",
            blueice_ecma402::DisplayNamesStyle::Short => "short",
            blueice_ecma402::DisplayNamesStyle::Narrow => "narrow",
        };
        let display_type = match resolved.display_type {
            blueice_ecma402::DisplayNamesType::Language => "language",
            blueice_ecma402::DisplayNamesType::Region => "region",
            blueice_ecma402::DisplayNamesType::Script => "script",
            blueice_ecma402::DisplayNamesType::Currency => "currency",
            blueice_ecma402::DisplayNamesType::Calendar => "calendar",
            blueice_ecma402::DisplayNamesType::DateTimeField => "dateTimeField",
        };
        let fallback = match resolved.fallback {
            blueice_ecma402::DisplayNamesFallback::Code => "code",
            blueice_ecma402::DisplayNamesFallback::None => "none",
        };
        for (key, value) in [
            ("locale", Value::String(resolved.locale.clone().into())),
            ("style", Value::String(style.into())),
            ("type", Value::String(display_type.into())),
            ("fallback", Value::String(fallback.into())),
        ] {
            self.define_data(result, key, value, true, true, true)?;
        }
        if let Some(language_display) = resolved.language_display {
            self.define_data(
                result,
                "languageDisplay",
                Value::String(
                    match language_display {
                        blueice_ecma402::DisplayNamesLanguageDisplay::Dialect => "dialect",
                        blueice_ecma402::DisplayNamesLanguageDisplay::Standard => "standard",
                    }
                    .into(),
                ),
                true,
                true,
                true,
            )?;
        }
        Ok(Value::Object(result))
    }

    pub(in super::super) fn relative_time_numbering_system(
        &mut self,
        options: &Value,
    ) -> Result<Option<String>, RuntimeError> {
        let value = self.string_option(options, "numberingSystem", &[])?;
        if value.as_ref().is_some_and(|value| {
            value.split('-').any(|part| {
                !(3..=8).contains(&part.len())
                    || !part.bytes().all(|byte| byte.is_ascii_alphanumeric())
            })
        }) {
            return Err(RuntimeError::RangeError(
                "invalid numberingSystem option".into(),
            ));
        }
        Ok(value.map(|value| value.to_ascii_lowercase()))
    }

    pub(in super::super) fn relative_time_style(
        &mut self,
        options: &Value,
    ) -> Result<blueice_ecma402::RelativeTimeStyle, RuntimeError> {
        match self
            .string_option(options, "style", &["long", "short", "narrow"])?
            .as_deref()
        {
            None | Some("long") => Ok(blueice_ecma402::RelativeTimeStyle::Long),
            Some("short") => Ok(blueice_ecma402::RelativeTimeStyle::Short),
            Some("narrow") => Ok(blueice_ecma402::RelativeTimeStyle::Narrow),
            Some(_) => unreachable!("string_option validates RelativeTimeFormat style"),
        }
    }

    pub(in super::super) fn relative_time_numeric(
        &mut self,
        options: &Value,
    ) -> Result<blueice_ecma402::RelativeTimeNumeric, RuntimeError> {
        match self
            .string_option(options, "numeric", &["always", "auto"])?
            .as_deref()
        {
            None | Some("always") => Ok(blueice_ecma402::RelativeTimeNumeric::Always),
            Some("auto") => Ok(blueice_ecma402::RelativeTimeNumeric::Auto),
            Some(_) => unreachable!("string_option validates RelativeTimeFormat numeric"),
        }
    }

    pub(in super::super) fn resolve_relative_time_format(
        &mut self,
        locales: &Value,
        options: &Value,
    ) -> Result<Rc<intl::RelativeTimeFormat>, RuntimeError> {
        let locales = self.canonical_locales(locales)?;
        // RelativeTimeFormat's Edition 13 ResolveOptions route starts from
        // CoerceOptionsToObject, unlike the newer GetOptionsObject service
        // constructors. Primitive options therefore expose inherited values.
        let options = self.intl_options(options)?;
        // Edition 13's ResolveOptions reads these properties in this exact
        // order; keep each conversion beside its lookup.
        let locale_matcher = self.locale_matcher(&options)?;
        let numbering_system = self.relative_time_numbering_system(&options)?;
        let style = self.relative_time_style(&options)?;
        let numeric = self.relative_time_numeric(&options)?;
        blueice_ecma402::RelativeTimeFormat::try_new(
            &locales,
            blueice_ecma402::RelativeTimeFormatOptions {
                locale_matcher,
                numbering_system,
                style,
                numeric,
            },
        )
        .map(Rc::new)
        .map_err(|error| RuntimeError::RangeError(error.to_string()))
    }

    pub(in super::super) fn create_relative_time_format(
        &mut self,
        args: &[Value],
        construct: bool,
    ) -> Result<Value, RuntimeError> {
        if !construct {
            return Err(RuntimeError::TypeError(
                "Intl.RelativeTimeFormat must be called with new".into(),
            ));
        }
        self.intl_global()?;
        let constructor = self.globals["%Intl.RelativeTimeFormat%"];
        let default = self
            .heap
            .get(constructor, "prototype")?
            .object_id()
            .expect("Intl.RelativeTimeFormat.prototype is an object");
        let prototype = self.constructor_prototype(default)?;
        self.stack.push(Value::Object(prototype));
        let data = self
            .resolve_relative_time_format(native::argument(args, 0), native::argument(args, 1))?;
        self.with_roots(|heap| heap.alloc_relative_time_format(data, prototype))
            .map(Value::Object)
    }

    pub(in super::super) fn relative_time_format_supported_locales(
        &mut self,
        args: &[Value],
    ) -> Result<Value, RuntimeError> {
        let locales = self.canonical_locales(native::argument(args, 0))?;
        let options = self.intl_options(native::argument(args, 1))?;
        let matcher = self.locale_matcher(&options)?;
        let locales = blueice_ecma402::supported_relative_time_format_locales(&locales, matcher);
        self.array_from(
            locales
                .into_iter()
                .map(|locale| Value::String(locale.to_string().into()))
                .collect(),
        )
    }

    pub(in super::super) fn relative_time_format_data(
        &self,
        value: &Value,
    ) -> Result<Rc<intl::RelativeTimeFormat>, RuntimeError> {
        if let Value::Object(id) = value {
            if let Some(data) = self.heap.relative_time_format(*id)? {
                return Ok(data);
            }
        }
        Err(RuntimeError::TypeError(
            "receiver is not an Intl.RelativeTimeFormat".into(),
        ))
    }

    pub(in super::super) fn relative_time_unit(
        &mut self,
        value: &Value,
    ) -> Result<blueice_ecma402::RelativeTimeUnit, RuntimeError> {
        let value = self.coerce_string(value)?;
        let value = value
            .to_utf8()
            .map_err(|_| RuntimeError::RangeError("invalid relative-time unit".into()))?;
        blueice_ecma402::RelativeTimeUnit::parse(&value)
            .ok_or_else(|| RuntimeError::RangeError("invalid relative-time unit".into()))
    }

    pub(in super::super) fn relative_time_parts(
        &mut self,
        receiver: &Value,
        value: &Value,
        unit: &Value,
    ) -> Result<(Vec<blueice_ecma402::RelativeTimePart>, &'static str), RuntimeError> {
        let data = self.relative_time_format_data(receiver)?;
        let value = self.coerce_number(value)?;
        let unit = self.relative_time_unit(unit)?;
        let parts = data
            .format_to_parts(value, unit)
            .map_err(|error| RuntimeError::RangeError(error.to_string()))?;
        Ok((parts, unit.as_str()))
    }

    pub(in super::super) fn relative_time_format_format(
        &mut self,
        receiver: &Value,
        value: &Value,
        unit: &Value,
    ) -> Result<Value, RuntimeError> {
        let data = self.relative_time_format_data(receiver)?;
        let value = self.coerce_number(value)?;
        let unit = self.relative_time_unit(unit)?;
        let parts = data
            .format_to_parts(value, unit)
            .map_err(|error| RuntimeError::RangeError(error.to_string()))?;
        Ok(Value::String(
            parts
                .into_iter()
                .map(|part| part.value)
                .collect::<String>()
                .into(),
        ))
    }

    pub(in super::super) fn relative_time_format_to_parts(
        &mut self,
        receiver: &Value,
        value: &Value,
        unit: &Value,
    ) -> Result<Value, RuntimeError> {
        let (parts, unit) = self.relative_time_parts(receiver, value, unit)?;
        let base = self.stack.len();
        let prototype = self.object_prototype;
        for part in parts {
            let object = self.with_roots(|heap| heap.alloc_object(Some(prototype)))?;
            self.stack.push(Value::Object(object));
            let kind = match part.kind {
                blueice_ecma402::RelativeTimePartKind::Literal => "literal",
                blueice_ecma402::RelativeTimePartKind::Integer => "integer",
                blueice_ecma402::RelativeTimePartKind::Group => "group",
                blueice_ecma402::RelativeTimePartKind::Decimal => "decimal",
                blueice_ecma402::RelativeTimePartKind::Fraction => "fraction",
            };
            self.define_data(object, "type", Value::String(kind.into()), true, true, true)?;
            self.define_data(
                object,
                "value",
                Value::String(part.value.into()),
                true,
                true,
                true,
            )?;
            if part.kind != blueice_ecma402::RelativeTimePartKind::Literal {
                self.define_data(object, "unit", Value::String(unit.into()), true, true, true)?;
            }
        }
        let result = self.array_from(self.stack[base..].to_vec());
        self.stack.truncate(base);
        result
    }

    pub(in super::super) fn relative_time_format_resolved_options(
        &mut self,
        receiver: &Value,
    ) -> Result<Value, RuntimeError> {
        let data = self.relative_time_format_data(receiver)?;
        let resolved = data.resolved_options();
        let prototype = self.object_prototype;
        let object = self.with_roots(|heap| heap.alloc_object(Some(prototype)))?;
        self.stack.push(Value::Object(object));
        for (key, value) in [
            ("locale", Value::String(resolved.locale.clone().into())),
            (
                "style",
                Value::String(
                    match resolved.style {
                        blueice_ecma402::RelativeTimeStyle::Long => "long",
                        blueice_ecma402::RelativeTimeStyle::Short => "short",
                        blueice_ecma402::RelativeTimeStyle::Narrow => "narrow",
                    }
                    .into(),
                ),
            ),
            (
                "numeric",
                Value::String(
                    match resolved.numeric {
                        blueice_ecma402::RelativeTimeNumeric::Always => "always",
                        blueice_ecma402::RelativeTimeNumeric::Auto => "auto",
                    }
                    .into(),
                ),
            ),
            (
                "numberingSystem",
                Value::String(resolved.numbering_system.clone().into()),
            ),
        ] {
            self.define_data(object, key, value, true, true, true)?;
        }
        Ok(Value::Object(object))
    }
}
