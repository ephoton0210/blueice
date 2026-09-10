// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

use super::*;
use crate::intl;
use icu_collator::options::{AlternateHandling, CaseLevel, CollatorOptions, MaxVariable, Strength};
use icu_collator::preferences::{CollationCaseFirst, CollationNumericOrdering};
use icu_locale_core::{
    extensions::unicode::Value as UnicodeValue,
    locale,
    subtags::{Language, Region, Script, Variant, Variants},
    Locale,
};
use std::rc::Rc;

impl Vm {
    pub(super) fn intl_global(&mut self) -> Result<Value, RuntimeError> {
        if let Some(&id) = self.globals.get("Intl") {
            return Ok(Value::Object(id));
        }
        let string = self.string_intrinsics()?.0;
        let function_prototype = self.heap.prototype(string)?.unwrap();
        let object_prototype = self.object_prototype;
        let namespace = self.with_roots(|heap| heap.alloc_object(Some(object_prototype)))?;
        let root = self.heap.root(namespace)?;
        let mut constructor_root = None;
        let result = (|| {
            self.define_data(
                namespace,
                JsSymbol::well_known("toStringTag"),
                Value::String("Intl".into()),
                false,
                false,
                true,
            )?;
            self.install_native(
                namespace,
                function_prototype,
                "getCanonicalLocales",
                1,
                NativeFunction::CanonicalLocales,
            )?;
            self.install_native(
                namespace,
                function_prototype,
                "Collator",
                0,
                NativeFunction::Collator,
            )?;
            let constructor = self.heap.get(namespace, "Collator")?.object_id().unwrap();
            constructor_root = Some(self.heap.root(constructor)?);
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
                prototype,
                "constructor",
                Value::Object(constructor),
                true,
                false,
                true,
            )?;
            self.define_data(
                prototype,
                JsSymbol::well_known("toStringTag"),
                Value::String("Intl.Collator".into()),
                false,
                false,
                true,
            )?;
            self.install_native(
                constructor,
                function_prototype,
                "supportedLocalesOf",
                1,
                NativeFunction::SupportedLocales,
            )?;
            self.install_native(
                prototype,
                function_prototype,
                "resolvedOptions",
                0,
                NativeFunction::CollatorResolvedOptions,
            )?;
            self.install_getter(
                prototype,
                function_prototype,
                "compare".into(),
                "get compare",
                NativeFunction::CollatorCompareGetter,
            )?;
            self.globals.insert("%Intl.Collator%".into(), constructor);
            self.install_native(
                namespace,
                function_prototype,
                "Locale",
                1,
                NativeFunction::Locale,
            )?;
            let constructor = self.heap.get(namespace, "Locale")?.object_id().unwrap();
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
                prototype,
                "constructor",
                Value::Object(constructor),
                true,
                false,
                true,
            )?;
            self.define_data(
                prototype,
                JsSymbol::well_known("toStringTag"),
                Value::String("Intl.Locale".into()),
                false,
                false,
                true,
            )?;
            self.install_native(
                prototype,
                function_prototype,
                "toString",
                0,
                NativeFunction::LocaleToString,
            )?;
            self.install_native(
                prototype,
                function_prototype,
                "maximize",
                0,
                NativeFunction::LocaleMaximize,
            )?;
            self.install_native(
                prototype,
                function_prototype,
                "minimize",
                0,
                NativeFunction::LocaleMinimize,
            )?;
            for (name, operation) in [
                ("getCalendars", native::LocaleInfo::Calendars),
                ("getCollations", native::LocaleInfo::Collations),
                ("getHourCycles", native::LocaleInfo::HourCycles),
                ("getNumberingSystems", native::LocaleInfo::NumberingSystems),
                ("getTextInfo", native::LocaleInfo::TextInfo),
                ("getTimeZones", native::LocaleInfo::TimeZones),
                ("getWeekInfo", native::LocaleInfo::WeekInfo),
            ] {
                self.install_native(
                    prototype,
                    function_prototype,
                    name,
                    0,
                    NativeFunction::LocaleInfo(operation),
                )?;
            }
            for (name, getter) in [
                ("baseName", native::LocaleGetter::BaseName),
                ("language", native::LocaleGetter::Language),
                ("script", native::LocaleGetter::Script),
                ("region", native::LocaleGetter::Region),
                ("variants", native::LocaleGetter::Variants),
                ("calendar", native::LocaleGetter::Calendar),
                ("collation", native::LocaleGetter::Collation),
                ("hourCycle", native::LocaleGetter::HourCycle),
                ("caseFirst", native::LocaleGetter::CaseFirst),
                ("numeric", native::LocaleGetter::Numeric),
                ("numberingSystem", native::LocaleGetter::NumberingSystem),
                ("firstDayOfWeek", native::LocaleGetter::FirstDayOfWeek),
            ] {
                self.install_getter(
                    prototype,
                    function_prototype,
                    name.into(),
                    &format!("get {name}"),
                    NativeFunction::LocaleGetter(getter),
                )?;
            }
            self.globals.insert("%Intl.Locale%".into(), constructor);
            self.globals.insert("Intl".into(), namespace);
            Ok(Value::Object(namespace))
        })();
        if result.is_err() {
            self.heap.unroot(root)?;
            if let Some(root) = constructor_root {
                self.heap.unroot(root)?;
            }
        }
        result
    }

    pub(super) fn canonical_locales(
        &mut self,
        locales: &Value,
    ) -> Result<Vec<Locale>, RuntimeError> {
        if *locales == Value::Undefined {
            return Ok(Vec::new());
        }
        if let Value::String(string) = locales {
            return Ok(vec![intl::canonicalize(string)?]);
        }
        if let Value::Object(id) = locales {
            if let Some(locale) = self.heap.intl_locale(*id)? {
                return Ok(vec![locale.locale.clone()]);
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
            let mut current = Some(object);
            let mut present = false;
            while let Some(id) = current {
                if self.heap.get_own_property_descriptor(id, &key)?.is_some() {
                    present = true;
                    break;
                }
                current = self.heap.prototype(id)?;
            }
            if !present {
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
                    locale.locale.clone()
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

    fn intl_options(&mut self, value: &Value) -> Result<Value, RuntimeError> {
        let object = if *value == Value::Undefined {
            self.with_roots(|heap| heap.alloc_object(None))?
        } else {
            self.coerce_object(value)?
        };
        let result = Value::Object(object);
        self.stack.push(result.clone());
        Ok(result)
    }

    fn string_option(
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

    pub(super) fn supported_locales(&mut self, args: &[Value]) -> Result<Value, RuntimeError> {
        let locales = self.canonical_locales(native::argument(args, 0))?;
        let options = self.intl_options(native::argument(args, 1))?;
        self.string_option(&options, "localeMatcher", &["lookup", "best fit"])?;
        self.array_from(
            locales
                .iter()
                .filter(|l| intl::supported(l))
                .map(|l| Value::String(l.to_string().into()))
                .collect(),
        )
    }

    pub(super) fn resolve_collator(
        &mut self,
        locales: &Value,
        options: &Value,
    ) -> Result<Rc<intl::Collator>, RuntimeError> {
        let locales = self.canonical_locales(locales)?;
        let options = self.intl_options(options)?;
        let usage = self
            .string_option(&options, "usage", &["sort", "search"])?
            .unwrap_or_else(|| "sort".into());
        self.string_option(&options, "localeMatcher", &["lookup", "best fit"])?;
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
        let selected = locales
            .into_iter()
            .find(intl::supported)
            .unwrap_or(locale!("en-US"));
        let mut resolved = Locale::from(selected.id.clone());
        let mut algorithm_locale = resolved.clone();
        let mut selected_collation = "default".to_string();
        // Retain a supported extension only when an option does not override it.
        for (key, option) in [
            ("co", collation.map(|s| s.to_ascii_lowercase())),
            ("kf", case_first),
            ("kn", numeric.map(|b| b.to_string())),
        ] {
            let extension = intl::keyword(&selected, key).map(|s| {
                if key == "kn" && s.is_empty() {
                    "true".into()
                } else {
                    s
                }
            });
            let valid = |value: &str| match key {
                "co" => usage == "sort" && intl::supports_collation(&selected, value),
                "kf" => ["upper", "lower", "false"].contains(&value),
                _ => ["true", "false"].contains(&value),
            };
            let extension = extension.filter(|s| valid(s));
            let choice = option.filter(|s| valid(s)).or_else(|| extension.clone());
            if let Some(value) = choice {
                if extension.as_ref() == Some(&value) {
                    resolved
                        .extensions
                        .unicode
                        .keywords
                        .set(key.parse().unwrap(), value.parse().unwrap());
                }
                algorithm_locale
                    .extensions
                    .unicode
                    .keywords
                    .set(key.parse().unwrap(), value.parse().unwrap());
                if key == "co" {
                    selected_collation = value;
                }
            }
        }
        let mut preferences = intl::preferences(&algorithm_locale);
        if usage == "search" {
            preferences.collation_type = Some(icu_collator::preferences::CollationType::Search);
        }
        let sensitivity = self
            .string_option(
                &options,
                "sensitivity",
                &["base", "accent", "case", "variant"],
            )?
            .unwrap_or_else(|| "variant".into());
        let punctuation = self.get_property(&options, &"ignorePunctuation".into())?;
        let ignore_punctuation = if punctuation == Value::Undefined {
            selected.id.language.as_str() == "th"
        } else {
            self.to_boolean(&punctuation)?
        };
        let mut options = CollatorOptions::default();
        options.strength = Some(match sensitivity.as_str() {
            "base" | "case" => Strength::Primary,
            "accent" => Strength::Secondary,
            _ => Strength::Tertiary,
        });
        options.case_level = Some(if sensitivity == "case" {
            CaseLevel::On
        } else {
            CaseLevel::Off
        });
        options.alternate_handling = Some(if ignore_punctuation {
            AlternateHandling::Shifted
        } else {
            AlternateHandling::NonIgnorable
        });
        options.max_variable = Some(MaxVariable::Punctuation);
        // All preferences are validated above; the bundled provider includes
        // root fallback plus every advertised tailoring. Missing data here
        // is a broken build invariant, not a user locale RangeError.
        let algorithm = icu_collator::Collator::try_new(preferences, options)
            .expect("bundled ICU collation data includes validated preferences");
        Ok(Rc::new(intl::Collator {
            algorithm,
            locale: resolved.to_string(),
            usage,
            sensitivity,
            ignore_punctuation,
            collation: selected_collation,
        }))
    }

    pub(super) fn create_collator(
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

    pub(super) fn collator_data(&self, value: &Value) -> Result<Rc<intl::Collator>, RuntimeError> {
        if let Value::Object(id) = value {
            if let Some(data) = self.heap.collator(*id)? {
                return Ok(data);
            }
        }
        Err(RuntimeError::TypeError(
            "receiver is not an Intl.Collator".into(),
        ))
    }

    pub(super) fn collator_compare_getter(
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
        self.define_data(
            function,
            "name",
            Value::String("".into()),
            false,
            false,
            true,
        )?;
        self.define_data(function, "length", Value::Number(2.0), false, false, true)?;
        self.heap.set_collator_compare(id, function);
        Ok(Value::Object(function))
    }

    pub(super) fn collator_resolved_options(
        &mut self,
        receiver: &Value,
    ) -> Result<Value, RuntimeError> {
        let data = self.collator_data(receiver)?;
        let resolved = data.algorithm.resolved_options();
        let prototype = self.object_prototype;
        let result = self.with_roots(|heap| heap.alloc_object(Some(prototype)))?;
        self.stack.push(Value::Object(result));
        for (key, value) in [
            ("locale", Value::String(data.locale.as_str().into())),
            ("usage", Value::String(data.usage.as_str().into())),
            (
                "sensitivity",
                Value::String(data.sensitivity.as_str().into()),
            ),
            ("ignorePunctuation", Value::Bool(data.ignore_punctuation)),
            ("collation", Value::String(data.collation.as_str().into())),
            (
                "numeric",
                Value::Bool(resolved.numeric == CollationNumericOrdering::True),
            ),
            (
                "caseFirst",
                Value::String(
                    match resolved.case_first {
                        CollationCaseFirst::Upper => "upper",
                        CollationCaseFirst::Lower => "lower",
                        _ => "false",
                    }
                    .into(),
                ),
            ),
        ] {
            self.define_data(result, key, value, true, true, true)?;
        }
        Ok(Value::Object(result))
    }

    fn locale_unicode_option(
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

    fn locale_set_keyword(
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

    fn locale_first_day_of_week(
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

    fn locale_instance(
        &mut self,
        locale: Locale,
        prototype: ObjectId,
    ) -> Result<Value, RuntimeError> {
        self.with_roots(|heap| heap.alloc_intl_locale(Rc::new(intl::Locale { locale }), prototype))
            .map(Value::Object)
    }

    pub(super) fn create_locale(
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
        let mut locale = match tag {
            Value::String(string) => intl::canonicalize(string)?,
            Value::Object(id) => {
                if let Some(locale) = self.heap.intl_locale(*id)? {
                    locale.locale.clone()
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
        locale = intl::canonicalize(&JsString::from(locale.to_string()))?;
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

    pub(super) fn locale_data(&self, receiver: &Value) -> Result<Rc<intl::Locale>, RuntimeError> {
        let id = receiver.object_id().ok_or(RuntimeError::TypeError(
            "receiver is not an Intl.Locale".into(),
        ))?;
        self.heap.intl_locale(id)?.ok_or(RuntimeError::TypeError(
            "receiver is not an Intl.Locale".into(),
        ))
    }

    pub(super) fn locale_to_string(&self, receiver: &Value) -> Result<Value, RuntimeError> {
        Ok(Value::String(
            self.locale_data(receiver)?.locale.to_string().into(),
        ))
    }

    pub(super) fn locale_transform(
        &mut self,
        receiver: &Value,
        maximize: bool,
    ) -> Result<Value, RuntimeError> {
        let mut locale = self.locale_data(receiver)?.locale.clone();
        let expander = icu_locale::LocaleExpander::new_extended();
        if maximize {
            expander.maximize(&mut locale.id);
        } else {
            expander.minimize(&mut locale.id);
        }
        self.intl_global()?;
        let constructor = self.globals["%Intl.Locale%"];
        let prototype = self
            .heap
            .get(constructor, "prototype")?
            .object_id()
            .unwrap();
        self.locale_instance(locale, prototype)
    }

    pub(super) fn locale_getter(
        &self,
        receiver: &Value,
        name: native::LocaleGetter,
    ) -> Result<Value, RuntimeError> {
        let locale = &self.locale_data(receiver)?.locale;
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

    fn locale_info_array(&mut self, values: Vec<String>) -> Result<Value, RuntimeError> {
        self.array_from(
            values
                .into_iter()
                .map(|value| Value::String(value.into()))
                .collect(),
        )
    }

    pub(super) fn locale_info(
        &mut self,
        receiver: &Value,
        operation: native::LocaleInfo,
    ) -> Result<Value, RuntimeError> {
        let locale = self.locale_data(receiver)?.locale.clone();
        match operation {
            native::LocaleInfo::Calendars => {
                let calendar = intl::keyword(&locale, "ca").unwrap_or_else(|| "gregory".into());
                self.locale_info_array(vec![calendar])
            }
            native::LocaleInfo::Collations => {
                let collation = intl::keyword(&locale, "co")
                    .filter(|value| value != "standard" && value != "search")
                    .unwrap_or_else(|| "emoji".into());
                self.locale_info_array(vec![collation])
            }
            native::LocaleInfo::HourCycles => {
                let hour_cycle = intl::keyword(&locale, "hc").unwrap_or_else(|| {
                    if locale.id.language.as_str() == "en" {
                        "h12".into()
                    } else {
                        "h23".into()
                    }
                });
                self.locale_info_array(vec![hour_cycle])
            }
            native::LocaleInfo::NumberingSystems => {
                let numbering_system = intl::keyword(&locale, "nu").unwrap_or_else(|| {
                    if locale.id.language.as_str() == "ar" {
                        "arab".into()
                    } else {
                        "latn".into()
                    }
                });
                self.locale_info_array(vec![numbering_system])
            }
            native::LocaleInfo::TextInfo => {
                let script = locale.id.script.map(|script| script.to_string());
                let rtl = script.as_deref().is_some_and(|script| {
                    matches!(
                        script,
                        "Arab" | "Hebr" | "Syrc" | "Thaa" | "Nkoo" | "Adlm" | "Rohg"
                    )
                }) || matches!(
                    locale.id.language.as_str(),
                    "ar" | "arc"
                        | "ckb"
                        | "dv"
                        | "fa"
                        | "he"
                        | "ks"
                        | "ku"
                        | "nqo"
                        | "ps"
                        | "sd"
                        | "syr"
                        | "ug"
                        | "ur"
                        | "yi"
                );
                let prototype = self.object_prototype;
                let result = self.with_roots(|heap| heap.alloc_object(Some(prototype)))?;
                self.define_data(
                    result,
                    "direction",
                    Value::String(if rtl { "rtl" } else { "ltr" }.into()),
                    true,
                    true,
                    true,
                )?;
                Ok(Value::Object(result))
            }
            native::LocaleInfo::TimeZones => {
                let Some(region) = locale.id.region else {
                    return Ok(Value::Undefined);
                };
                let zones = match region.as_str() {
                    "US" => vec![
                        "America/Adak",
                        "America/Anchorage",
                        "America/Boise",
                        "America/Chicago",
                        "America/Denver",
                        "America/Detroit",
                        "America/Indiana/Indianapolis",
                        "America/Los_Angeles",
                        "America/New_York",
                        "Pacific/Honolulu",
                    ],
                    "GB" => vec!["Europe/London"],
                    "JP" => vec!["Asia/Tokyo"],
                    "TW" => vec!["Asia/Taipei"],
                    _ => vec!["Etc/UTC"],
                };
                self.locale_info_array(zones.into_iter().map(String::from).collect())
            }
            native::LocaleInfo::WeekInfo => {
                let first_day =
                    match intl::keyword(&locale, "fw").as_deref() {
                        Some("mon") => 1.0,
                        Some("tue") => 2.0,
                        Some("wed") => 3.0,
                        Some("thu") => 4.0,
                        Some("fri") => 5.0,
                        Some("sat") => 6.0,
                        Some("sun") => 7.0,
                        _ if locale.id.region.as_ref().is_some_and(|region| {
                            matches!(region.as_str(), "US" | "CA" | "JP")
                        }) =>
                        {
                            7.0
                        }
                        _ => 1.0,
                    };
                let prototype = self.object_prototype;
                let result = self.with_roots(|heap| heap.alloc_object(Some(prototype)))?;
                self.stack.push(Value::Object(result));
                self.define_data(
                    result,
                    "firstDay",
                    Value::Number(first_day),
                    true,
                    true,
                    true,
                )?;
                let weekend = self.array_from(vec![Value::Number(6.0), Value::Number(7.0)])?;
                self.define_data(result, "weekend", weekend, true, true, true)?;
                self.stack.pop();
                Ok(Value::Object(result))
            }
        }
    }
}
