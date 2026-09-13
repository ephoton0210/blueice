// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

use super::*;
use crate::intl;
use icu_locale_core::{
    extensions::unicode::Value as UnicodeValue,
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
            // NumberFormat and DateTimeFormat are mandatory service
            // constructors. NumberFormat delegates its finite decimal slice to
            // blueice-ecma402; DateTimeFormat retains its allocation-only
            // boundary until its own host-neutral formatter is complete.
            for (name, service) in [
                ("NumberFormat", native::IntlService::Number),
                ("DateTimeFormat", native::IntlService::DateTime),
                ("DisplayNames", native::IntlService::DisplayNames),
                ("ListFormat", native::IntlService::List),
                ("PluralRules", native::IntlService::Plural),
                ("RelativeTimeFormat", native::IntlService::RelativeTime),
                ("Segmenter", native::IntlService::Segmenter),
            ] {
                self.install_native(
                    namespace,
                    function_prototype,
                    name,
                    if service == native::IntlService::DisplayNames {
                        2
                    } else {
                        0
                    },
                    NativeFunction::IntlService(service),
                )?;
                let constructor = self.heap.get(namespace, name)?.object_id().unwrap();
                let prototype =
                    self.with_roots(|heap| heap.alloc_object(Some(object_prototype)))?;
                self.stack.push(Value::Object(prototype));
                let result: Result<(), RuntimeError> = (|| {
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
                    if service == native::IntlService::Number {
                        self.define_data(
                            prototype,
                            JsSymbol::well_known("toStringTag"),
                            Value::String("Intl.NumberFormat".into()),
                            false,
                            false,
                            true,
                        )?;
                        self.install_native(
                            constructor,
                            function_prototype,
                            "supportedLocalesOf",
                            1,
                            NativeFunction::NumberFormatSupportedLocales,
                        )?;
                        self.install_native(
                            prototype,
                            function_prototype,
                            "resolvedOptions",
                            0,
                            NativeFunction::NumberFormatResolvedOptions,
                        )?;
                        self.install_getter(
                            prototype,
                            function_prototype,
                            "format".into(),
                            "get format",
                            NativeFunction::NumberFormatFormatGetter,
                        )?;
                        self.globals
                            .insert("%Intl.NumberFormat%".into(), constructor);
                    } else if service == native::IntlService::DisplayNames {
                        self.define_data(
                            prototype,
                            JsSymbol::well_known("toStringTag"),
                            Value::String("Intl.DisplayNames".into()),
                            false,
                            false,
                            true,
                        )?;
                        self.install_native(
                            constructor,
                            function_prototype,
                            "supportedLocalesOf",
                            1,
                            NativeFunction::DisplayNamesSupportedLocales,
                        )?;
                        self.install_native(
                            prototype,
                            function_prototype,
                            "resolvedOptions",
                            0,
                            NativeFunction::DisplayNamesResolvedOptions,
                        )?;
                        self.install_native(
                            prototype,
                            function_prototype,
                            "of",
                            1,
                            NativeFunction::DisplayNamesOf,
                        )?;
                        self.globals
                            .insert("%Intl.DisplayNames%".into(), constructor);
                    } else if service == native::IntlService::List {
                        self.define_data(
                            prototype,
                            JsSymbol::well_known("toStringTag"),
                            Value::String("Intl.ListFormat".into()),
                            false,
                            false,
                            true,
                        )?;
                        self.install_native(
                            constructor,
                            function_prototype,
                            "supportedLocalesOf",
                            1,
                            NativeFunction::ListFormatSupportedLocales,
                        )?;
                        self.install_native(
                            prototype,
                            function_prototype,
                            "resolvedOptions",
                            0,
                            NativeFunction::ListFormatResolvedOptions,
                        )?;
                        self.install_native(
                            prototype,
                            function_prototype,
                            "format",
                            1,
                            NativeFunction::ListFormatFormat,
                        )?;
                        self.install_native(
                            prototype,
                            function_prototype,
                            "formatToParts",
                            1,
                            NativeFunction::ListFormatFormatToParts,
                        )?;
                        self.globals.insert("%Intl.ListFormat%".into(), constructor);
                    } else if service == native::IntlService::Plural {
                        self.define_data(
                            prototype,
                            JsSymbol::well_known("toStringTag"),
                            Value::String("Intl.PluralRules".into()),
                            false,
                            false,
                            true,
                        )?;
                        self.install_native(
                            constructor,
                            function_prototype,
                            "supportedLocalesOf",
                            1,
                            NativeFunction::PluralRulesSupportedLocales,
                        )?;
                        self.install_native(
                            prototype,
                            function_prototype,
                            "resolvedOptions",
                            0,
                            NativeFunction::PluralRulesResolvedOptions,
                        )?;
                        self.install_native(
                            prototype,
                            function_prototype,
                            "select",
                            1,
                            NativeFunction::PluralRulesSelect,
                        )?;
                        self.install_native(
                            prototype,
                            function_prototype,
                            "selectRange",
                            2,
                            NativeFunction::PluralRulesSelectRange,
                        )?;
                        self.globals
                            .insert("%Intl.PluralRules%".into(), constructor);
                    } else if service == native::IntlService::RelativeTime {
                        self.define_data(
                            prototype,
                            JsSymbol::well_known("toStringTag"),
                            Value::String("Intl.RelativeTimeFormat".into()),
                            false,
                            false,
                            true,
                        )?;
                        self.install_native(
                            constructor,
                            function_prototype,
                            "supportedLocalesOf",
                            1,
                            NativeFunction::RelativeTimeFormatSupportedLocales,
                        )?;
                        self.install_native(
                            prototype,
                            function_prototype,
                            "resolvedOptions",
                            0,
                            NativeFunction::RelativeTimeFormatResolvedOptions,
                        )?;
                        self.install_native(
                            prototype,
                            function_prototype,
                            "format",
                            2,
                            NativeFunction::RelativeTimeFormatFormat,
                        )?;
                        self.install_native(
                            prototype,
                            function_prototype,
                            "formatToParts",
                            2,
                            NativeFunction::RelativeTimeFormatFormatToParts,
                        )?;
                        self.globals
                            .insert("%Intl.RelativeTimeFormat%".into(), constructor);
                    } else if service == native::IntlService::Segmenter {
                        self.define_data(
                            prototype,
                            JsSymbol::well_known("toStringTag"),
                            Value::String("Intl.Segmenter".into()),
                            false,
                            false,
                            true,
                        )?;
                        self.install_native(
                            constructor,
                            function_prototype,
                            "supportedLocalesOf",
                            1,
                            NativeFunction::SegmenterSupportedLocales,
                        )?;
                        self.install_native(
                            prototype,
                            function_prototype,
                            "resolvedOptions",
                            0,
                            NativeFunction::SegmenterResolvedOptions,
                        )?;
                        self.install_native(
                            prototype,
                            function_prototype,
                            "segment",
                            1,
                            NativeFunction::SegmenterSegment,
                        )?;
                        self.globals.insert("%Intl.Segmenter%".into(), constructor);
                    }
                    Ok(())
                })();
                self.stack.pop();
                result?;
            }
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
        match result {
            Ok(value) => {
                if let Some(&global) = self.globals.get("globalThis") {
                    self.define_data(global, "Intl", Value::Object(namespace), true, false, true)?;
                }
                Ok(value)
            }
            Err(error) => {
                self.heap.unroot(root)?;
                if let Some(root) = constructor_root {
                    self.heap.unroot(root)?;
                }
                Err(error)
            }
        }
    }

    fn segmenter_internal_prototypes(&mut self) -> Result<(ObjectId, ObjectId), RuntimeError> {
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

    pub(super) fn canonical_locales(
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

    /// Edition 13's constructor initialization uses GetOptionsObject rather
    /// than the ToObject-based option coercion used by supportedLocalesOf.
    /// Keep the two abstract operations distinct: a primitive constructor
    /// options value throws, while a primitive supportedLocalesOf options
    /// value is boxed and can expose observable inherited properties.
    fn intl_constructor_options(&mut self, value: &Value) -> Result<Value, RuntimeError> {
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
        let matcher = self.locale_matcher(&options)?;
        let locales = blueice_ecma402::supported_collation_locales(&locales, matcher);
        self.array_from(
            locales
                .iter()
                .map(|l| Value::String(l.to_string().into()))
                .collect(),
        )
    }

    fn locale_matcher(
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

    pub(super) fn create_intl_service(
        &mut self,
        service: native::IntlService,
        args: &[Value],
        construct: bool,
    ) -> Result<Value, RuntimeError> {
        if service == native::IntlService::Number {
            return self.create_number_format(args, construct);
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

    fn resolve_display_names(
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

    fn create_display_names(
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

    pub(super) fn display_names_supported_locales(
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

    fn display_names_data(&self, value: &Value) -> Result<Rc<intl::DisplayNames>, RuntimeError> {
        if let Value::Object(id) = value {
            if let Some(data) = self.heap.display_names(*id)? {
                return Ok(data);
            }
        }
        Err(RuntimeError::TypeError(
            "receiver is not an Intl.DisplayNames".into(),
        ))
    }

    pub(super) fn display_names_of(
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

    pub(super) fn display_names_resolved_options(
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

    fn relative_time_numbering_system(
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

    fn relative_time_style(
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

    fn relative_time_numeric(
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

    fn resolve_relative_time_format(
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

    fn create_relative_time_format(
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

    pub(super) fn relative_time_format_supported_locales(
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

    fn relative_time_format_data(
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

    fn relative_time_unit(
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

    fn relative_time_parts(
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

    pub(super) fn relative_time_format_format(
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

    pub(super) fn relative_time_format_to_parts(
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

    pub(super) fn relative_time_format_resolved_options(
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

    fn number_grouping(
        &mut self,
        options: &Value,
    ) -> Result<blueice_ecma402::NumberGrouping, RuntimeError> {
        let value = self.get_property(options, &"useGrouping".into())?;
        match value {
            Value::Undefined => Ok(blueice_ecma402::NumberGrouping::Auto),
            Value::Bool(true) => Ok(blueice_ecma402::NumberGrouping::Always),
            Value::Bool(false) => Ok(blueice_ecma402::NumberGrouping::Never),
            value => match self
                .coerce_string(&value)?
                .to_utf8()
                .map_err(|_| RuntimeError::RangeError("invalid useGrouping option".into()))?
                .as_str()
            {
                "auto" => Ok(blueice_ecma402::NumberGrouping::Auto),
                "always" => Ok(blueice_ecma402::NumberGrouping::Always),
                "min2" => Ok(blueice_ecma402::NumberGrouping::Min2),
                "false" | "never" => Ok(blueice_ecma402::NumberGrouping::Never),
                _ => Err(RuntimeError::RangeError(
                    "invalid useGrouping option".into(),
                )),
            },
        }
    }

    fn number_fraction_digits_option(
        &mut self,
        options: &Value,
        name: &str,
    ) -> Result<Option<u8>, RuntimeError> {
        let value = self.get_property(options, &name.into())?;
        if value == Value::Undefined {
            return Ok(None);
        }
        let number = self.coerce_number(&value)?;
        if !number.is_finite() || !(0.0..=100.0).contains(&number) {
            return Err(RuntimeError::RangeError(format!("invalid {name} option")));
        }
        Ok(Some(number.floor() as u8))
    }

    fn resolve_number_format(
        &mut self,
        locales: &Value,
        options: &Value,
    ) -> Result<Rc<intl::NumberFormat>, RuntimeError> {
        let locales = self.canonical_locales(locales)?;
        let options = self.intl_options(options)?;
        let options = blueice_ecma402::NumberFormatOptions {
            locale_matcher: self.locale_matcher(&options)?,
            use_grouping: self.number_grouping(&options)?,
            minimum_fraction_digits: self
                .number_fraction_digits_option(&options, "minimumFractionDigits")?,
            maximum_fraction_digits: self
                .number_fraction_digits_option(&options, "maximumFractionDigits")?,
        };
        blueice_ecma402::NumberFormat::try_new(&locales, options)
            .map(Rc::new)
            .map_err(|error| RuntimeError::RangeError(error.to_string()))
    }

    pub(super) fn number_format_supported_locales(
        &mut self,
        args: &[Value],
    ) -> Result<Value, RuntimeError> {
        let locales = self.canonical_locales(native::argument(args, 0))?;
        let options = self.intl_options(native::argument(args, 1))?;
        let matcher = self.locale_matcher(&options)?;
        let locales = blueice_ecma402::supported_number_format_locales(&locales, matcher);
        self.array_from(
            locales
                .iter()
                .map(|locale| Value::String(locale.to_string().into()))
                .collect(),
        )
    }

    fn create_number_format(
        &mut self,
        args: &[Value],
        construct: bool,
    ) -> Result<Value, RuntimeError> {
        self.intl_global()?;
        let constructor = self.globals["%Intl.NumberFormat%"];
        let default = self
            .heap
            .get(constructor, "prototype")?
            .object_id()
            .expect("Intl.NumberFormat.prototype is an object");
        let prototype = if construct {
            self.constructor_prototype(default)?
        } else {
            default
        };
        self.stack.push(Value::Object(prototype));
        let data =
            self.resolve_number_format(native::argument(args, 0), native::argument(args, 1))?;
        self.with_roots(|heap| heap.alloc_number_format(data, prototype))
            .map(Value::Object)
    }

    fn list_type(&mut self, options: &Value) -> Result<blueice_ecma402::ListType, RuntimeError> {
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

    fn list_style(&mut self, options: &Value) -> Result<blueice_ecma402::ListStyle, RuntimeError> {
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

    fn resolve_list_format(
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

    pub(super) fn list_format_supported_locales(
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

    fn create_list_format(
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

    pub(super) fn list_format_data(
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

    fn list_format_values(&mut self, value: &Value) -> Result<Vec<JsString>, RuntimeError> {
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

    fn list_format_parts(
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

    pub(super) fn list_format_format(
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

    pub(super) fn list_format_format_to_parts(
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

    pub(super) fn list_format_resolved_options(
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

    fn plural_rules_integer_option(
        &mut self,
        options: &Value,
        name: &str,
        minimum: u8,
        maximum: u8,
    ) -> Result<Option<u8>, RuntimeError> {
        let value = self.get_property(options, &name.into())?;
        if value == Value::Undefined {
            return Ok(None);
        }
        let value = self.coerce_number(&value)?;
        if !value.is_finite()
            || value.floor() != value
            || !(f64::from(minimum)..=f64::from(maximum)).contains(&value)
        {
            return Err(RuntimeError::RangeError(format!("invalid {name} option")));
        }
        Ok(Some(value as u8))
    }

    fn plural_rules_rounding_increment(&mut self, options: &Value) -> Result<u16, RuntimeError> {
        let value = self.get_property(options, &"roundingIncrement".into())?;
        if value == Value::Undefined {
            return Ok(1);
        }
        let value = self.coerce_number(&value)?;
        const INCREMENTS: &[u16] = &[
            1, 2, 5, 10, 20, 25, 50, 100, 200, 250, 500, 1000, 2000, 2500, 5000,
        ];
        if !value.is_finite() || value.floor() != value || !INCREMENTS.contains(&(value as u16)) {
            return Err(RuntimeError::RangeError(
                "invalid roundingIncrement option".into(),
            ));
        }
        Ok(value as u16)
    }

    fn resolve_plural_rules(
        &mut self,
        locales: &Value,
        options: &Value,
    ) -> Result<Rc<intl::PluralRules>, RuntimeError> {
        let locales = self.canonical_locales(locales)?;
        let options = self.intl_constructor_options(options)?;

        // ECMA-402's observable GetOption order is intentional. Keep this
        // sequence adjacent to the corresponding Edition 13 initialization
        // steps; property getters may throw or record every access.
        let locale_matcher = self.locale_matcher(&options)?;
        let rule_type = self
            .string_option(&options, "type", &["cardinal", "ordinal"])?
            .unwrap_or_else(|| "cardinal".into());
        let notation = self
            .string_option(
                &options,
                "notation",
                &["standard", "compact", "scientific", "engineering"],
            )?
            .unwrap_or_else(|| "standard".into());
        let compact_display = self.string_option(&options, "compactDisplay", &["short", "long"])?;
        let minimum_integer_digits = self
            .plural_rules_integer_option(&options, "minimumIntegerDigits", 1, 21)?
            .unwrap_or(1);
        let minimum_fraction_digits =
            self.plural_rules_integer_option(&options, "minimumFractionDigits", 0, 20)?;
        let maximum_fraction_digits =
            self.plural_rules_integer_option(&options, "maximumFractionDigits", 0, 20)?;
        let minimum_significant_digits =
            self.plural_rules_integer_option(&options, "minimumSignificantDigits", 1, 21)?;
        let maximum_significant_digits =
            self.plural_rules_integer_option(&options, "maximumSignificantDigits", 1, 21)?;
        let rounding_increment = self.plural_rules_rounding_increment(&options)?;
        let rounding_mode = self
            .string_option(
                &options,
                "roundingMode",
                &[
                    "ceil",
                    "floor",
                    "expand",
                    "trunc",
                    "halfCeil",
                    "halfFloor",
                    "halfExpand",
                    "halfTrunc",
                    "halfEven",
                ],
            )?
            .unwrap_or_else(|| "halfExpand".into());
        let rounding_priority = self
            .string_option(
                &options,
                "roundingPriority",
                &["auto", "morePrecision", "lessPrecision"],
            )?
            .unwrap_or_else(|| "auto".into());
        let trailing_zero_display = self
            .string_option(&options, "trailingZeroDisplay", &["auto", "stripIfInteger"])?
            .unwrap_or_else(|| "auto".into());

        let minimum_fraction_digits = minimum_fraction_digits.unwrap_or(0);
        let maximum_fraction_digits = maximum_fraction_digits.unwrap_or(3);
        if minimum_fraction_digits > maximum_fraction_digits {
            return Err(RuntimeError::RangeError(
                "minimumFractionDigits exceeds maximumFractionDigits".into(),
            ));
        }
        let (minimum_significant_digits, maximum_significant_digits) =
            match (minimum_significant_digits, maximum_significant_digits) {
                (None, None) => (None, None),
                (minimum, maximum) => {
                    let minimum = minimum.unwrap_or(1);
                    let maximum = maximum.unwrap_or(21);
                    if minimum > maximum {
                        return Err(RuntimeError::RangeError(
                            "minimumSignificantDigits exceeds maximumSignificantDigits".into(),
                        ));
                    }
                    (Some(minimum), Some(maximum))
                }
            };
        let compact_display =
            (notation == "compact").then(|| compact_display.unwrap_or_else(|| "short".into()));
        let rule_type = match rule_type.as_str() {
            "cardinal" => blueice_ecma402::PluralRuleType::Cardinal,
            "ordinal" => blueice_ecma402::PluralRuleType::Ordinal,
            _ => unreachable!("string_option validates PluralRules type"),
        };
        let data = blueice_ecma402::PluralRules::try_new(
            &locales,
            blueice_ecma402::PluralRulesOptions {
                locale_matcher,
                rule_type,
            },
        )
        .map_err(|error| RuntimeError::RangeError(error.to_string()))?;
        Ok(Rc::new(intl::PluralRules {
            data,
            rule_type,
            notation,
            compact_display,
            minimum_integer_digits,
            minimum_fraction_digits,
            maximum_fraction_digits,
            minimum_significant_digits,
            maximum_significant_digits,
            rounding_increment,
            rounding_mode,
            rounding_priority,
            trailing_zero_display,
        }))
    }

    pub(super) fn plural_rules_supported_locales(
        &mut self,
        args: &[Value],
    ) -> Result<Value, RuntimeError> {
        let locales = self.canonical_locales(native::argument(args, 0))?;
        let options = self.intl_options(native::argument(args, 1))?;
        let matcher = self.locale_matcher(&options)?;
        let locales = blueice_ecma402::supported_plural_rules_locales(&locales, matcher);
        self.array_from(
            locales
                .iter()
                .map(|locale| Value::String(locale.to_string().into()))
                .collect(),
        )
    }

    fn create_plural_rules(
        &mut self,
        args: &[Value],
        construct: bool,
    ) -> Result<Value, RuntimeError> {
        if !construct {
            return Err(RuntimeError::TypeError(
                "Intl.PluralRules must be called with new".into(),
            ));
        }
        self.intl_global()?;
        let constructor = self.globals["%Intl.PluralRules%"];
        let default = self
            .heap
            .get(constructor, "prototype")?
            .object_id()
            .expect("Intl.PluralRules.prototype is an object");
        let prototype = self.constructor_prototype(default)?;
        self.stack.push(Value::Object(prototype));
        let data =
            self.resolve_plural_rules(native::argument(args, 0), native::argument(args, 1))?;
        self.with_roots(|heap| heap.alloc_plural_rules(data, prototype))
            .map(Value::Object)
    }

    pub(super) fn plural_rules_data(
        &self,
        value: &Value,
    ) -> Result<Rc<intl::PluralRules>, RuntimeError> {
        if let Value::Object(id) = value {
            if let Some(data) = self.heap.plural_rules(*id)? {
                return Ok(data);
            }
        }
        Err(RuntimeError::TypeError(
            "receiver is not an Intl.PluralRules".into(),
        ))
    }

    fn plural_category_name(category: blueice_ecma402::PluralCategory) -> &'static str {
        match category {
            blueice_ecma402::PluralCategory::Zero => "zero",
            blueice_ecma402::PluralCategory::One => "one",
            blueice_ecma402::PluralCategory::Two => "two",
            blueice_ecma402::PluralCategory::Few => "few",
            blueice_ecma402::PluralCategory::Many => "many",
            blueice_ecma402::PluralCategory::Other => "other",
        }
    }

    fn plural_rules_categories(&self, data: &intl::PluralRules) -> Vec<Value> {
        let mut seen = [false; 6];
        let mut record = |value: f64| {
            if let Ok(category) = data.data.select_f64(value) {
                let index = match category {
                    blueice_ecma402::PluralCategory::Zero => 0,
                    blueice_ecma402::PluralCategory::One => 1,
                    blueice_ecma402::PluralCategory::Two => 2,
                    blueice_ecma402::PluralCategory::Few => 3,
                    blueice_ecma402::PluralCategory::Many => 4,
                    blueice_ecma402::PluralCategory::Other => 5,
                };
                seen[index] = true;
            }
        };
        for value in 0..=10_000 {
            record(f64::from(value));
        }
        for value in [0.1, 0.2, 0.5, 1.1, 1.5, 2.1, 10.1, 1_000_000.0] {
            record(value);
        }
        ["zero", "one", "two", "few", "many", "other"]
            .into_iter()
            .enumerate()
            .filter(|&(index, _)| seen[index])
            .map(|(_, name)| Value::String(name.into()))
            .collect()
    }

    pub(super) fn plural_rules_select(
        &mut self,
        receiver: &Value,
        value: &Value,
    ) -> Result<Value, RuntimeError> {
        let data = self.plural_rules_data(receiver)?;
        let value = self.coerce_number(value)?;
        if !value.is_finite() {
            return Ok(Value::String("other".into()));
        }
        let category = if data.notation == "compact" {
            data.data
                .select_compact_f64(value, data.compact_display.as_deref() == Some("long"))
        } else {
            data.data.select_f64(value)
        };
        category
            .map(Self::plural_category_name)
            .map(|category| Value::String(category.into()))
            .map_err(|error| RuntimeError::RangeError(error.to_string()))
    }

    pub(super) fn plural_rules_select_range(
        &mut self,
        receiver: &Value,
        start: &Value,
        end: &Value,
    ) -> Result<Value, RuntimeError> {
        let data = self.plural_rules_data(receiver)?;
        if *start == Value::Undefined || *end == Value::Undefined {
            return Err(RuntimeError::TypeError(
                "Intl.PluralRules selectRange arguments must not be undefined".into(),
            ));
        }
        let start = self.coerce_number(start)?;
        let end = self.coerce_number(end)?;
        if !start.is_finite() || !end.is_finite() {
            return Err(RuntimeError::RangeError(
                "Intl.PluralRules selectRange arguments must be finite".into(),
            ));
        }
        // The host service does not yet expose CLDR plural-range tables. The
        // identity range is exact; the non-identity fallback is the required
        // default for English and remains tracked as a host-service gap.
        if start == end {
            let category = if data.notation == "compact" {
                data.data
                    .select_compact_f64(start, data.compact_display.as_deref() == Some("long"))
            } else {
                data.data.select_f64(start)
            };
            return category
                .map(Self::plural_category_name)
                .map(|category| Value::String(category.into()))
                .map_err(|error| RuntimeError::RangeError(error.to_string()));
        }
        Ok(Value::String("other".into()))
    }

    pub(super) fn plural_rules_resolved_options(
        &mut self,
        receiver: &Value,
    ) -> Result<Value, RuntimeError> {
        let data = self.plural_rules_data(receiver)?;
        let prototype = self.object_prototype;
        let result = self.with_roots(|heap| heap.alloc_object(Some(prototype)))?;
        self.stack.push(Value::Object(result));
        let resolved = data.data.resolved_options();
        let mut properties = vec![
            ("locale", Value::String(resolved.locale.clone().into())),
            (
                "type",
                Value::String(
                    match data.rule_type {
                        blueice_ecma402::PluralRuleType::Cardinal => "cardinal",
                        blueice_ecma402::PluralRuleType::Ordinal => "ordinal",
                    }
                    .into(),
                ),
            ),
            ("notation", Value::String(data.notation.clone().into())),
        ];
        if let Some(compact_display) = &data.compact_display {
            properties.push((
                "compactDisplay",
                Value::String(compact_display.clone().into()),
            ));
        }
        properties.extend([
            (
                "minimumIntegerDigits",
                Value::Number(f64::from(data.minimum_integer_digits)),
            ),
            (
                "minimumFractionDigits",
                Value::Number(f64::from(data.minimum_fraction_digits)),
            ),
            (
                "maximumFractionDigits",
                Value::Number(f64::from(data.maximum_fraction_digits)),
            ),
        ]);
        if let (Some(minimum), Some(maximum)) = (
            data.minimum_significant_digits,
            data.maximum_significant_digits,
        ) {
            properties.extend([
                (
                    "minimumSignificantDigits",
                    Value::Number(f64::from(minimum)),
                ),
                (
                    "maximumSignificantDigits",
                    Value::Number(f64::from(maximum)),
                ),
            ]);
        }
        for (key, value) in properties {
            self.define_data(result, key, value, true, true, true)?;
        }
        let categories = self.array_from(self.plural_rules_categories(&data))?;
        self.define_data(result, "pluralCategories", categories, true, true, true)?;
        for (key, value) in [
            (
                "roundingIncrement",
                Value::Number(f64::from(data.rounding_increment)),
            ),
            (
                "roundingMode",
                Value::String(data.rounding_mode.clone().into()),
            ),
            (
                "roundingPriority",
                Value::String(data.rounding_priority.clone().into()),
            ),
            (
                "trailingZeroDisplay",
                Value::String(data.trailing_zero_display.clone().into()),
            ),
        ] {
            self.define_data(result, key, value, true, true, true)?;
        }
        Ok(Value::Object(result))
    }

    fn segmenter_granularity(
        &mut self,
        options: &Value,
    ) -> Result<blueice_ecma402::SegmenterGranularity, RuntimeError> {
        match self
            .string_option(options, "granularity", &["grapheme", "word", "sentence"])?
            .as_deref()
        {
            None | Some("grapheme") => Ok(blueice_ecma402::SegmenterGranularity::Grapheme),
            Some("word") => Ok(blueice_ecma402::SegmenterGranularity::Word),
            Some("sentence") => Ok(blueice_ecma402::SegmenterGranularity::Sentence),
            Some(_) => unreachable!("string_option validates Segmenter granularity"),
        }
    }

    fn resolve_segmenter(
        &mut self,
        locales: &Value,
        options: &Value,
    ) -> Result<Rc<intl::Segmenter>, RuntimeError> {
        let locales = self.canonical_locales(locales)?;
        let options = self.intl_constructor_options(options)?;
        let data = blueice_ecma402::Segmenter::try_new(
            &locales,
            blueice_ecma402::SegmenterOptions {
                locale_matcher: self.locale_matcher(&options)?,
                granularity: self.segmenter_granularity(&options)?,
            },
        )
        .map_err(|error| RuntimeError::RangeError(error.to_string()))?;
        Ok(Rc::new(data))
    }

    pub(super) fn segmenter_supported_locales(
        &mut self,
        args: &[Value],
    ) -> Result<Value, RuntimeError> {
        let locales = self.canonical_locales(native::argument(args, 0))?;
        let options = self.intl_options(native::argument(args, 1))?;
        let matcher = self.locale_matcher(&options)?;
        let locales = blueice_ecma402::supported_segmenter_locales(&locales, matcher);
        self.array_from(
            locales
                .iter()
                .map(|locale| Value::String(locale.to_string().into()))
                .collect(),
        )
    }

    fn create_segmenter(&mut self, args: &[Value], construct: bool) -> Result<Value, RuntimeError> {
        if !construct {
            return Err(RuntimeError::TypeError(
                "Intl.Segmenter must be called with new".into(),
            ));
        }
        self.intl_global()?;
        let constructor = self.globals["%Intl.Segmenter%"];
        let default = self
            .heap
            .get(constructor, "prototype")?
            .object_id()
            .expect("Intl.Segmenter.prototype is an object");
        let prototype = self.constructor_prototype(default)?;
        self.stack.push(Value::Object(prototype));
        let data = self.resolve_segmenter(native::argument(args, 0), native::argument(args, 1))?;
        self.with_roots(|heap| heap.alloc_segmenter(data, prototype))
            .map(Value::Object)
    }

    pub(super) fn segmenter_data(
        &self,
        value: &Value,
    ) -> Result<Rc<intl::Segmenter>, RuntimeError> {
        if let Value::Object(id) = value {
            if let Some(data) = self.heap.segmenter(*id)? {
                return Ok(data);
            }
        }
        Err(RuntimeError::TypeError(
            "receiver is not an Intl.Segmenter".into(),
        ))
    }

    pub(super) fn segmenter_segment(
        &mut self,
        receiver: &Value,
        input: &Value,
    ) -> Result<Value, RuntimeError> {
        let segmenter = self.segmenter_data(receiver)?;
        let input = self.coerce_string(input)?;
        let (prototype, _) = self.segmenter_internal_prototypes()?;
        let data = Rc::new(intl::Segments::from_segmenter(&segmenter, input));
        self.with_roots(|heap| heap.alloc_segments(data, prototype))
            .map(Value::Object)
    }

    fn segments_data(&self, value: &Value) -> Result<Rc<intl::Segments>, RuntimeError> {
        if let Value::Object(id) = value {
            if let Some(data) = self.heap.segments(*id)? {
                return Ok(data);
            }
        }
        Err(RuntimeError::TypeError(
            "receiver is not an Intl.Segmenter Segments object".into(),
        ))
    }

    fn segment_record(
        &mut self,
        data: &intl::Segments,
        index: usize,
    ) -> Result<Value, RuntimeError> {
        let (segment, start, is_word_like) = data
            .record(index)
            .expect("segment iterator index is bounded by its data");
        let prototype = self.object_prototype;
        let record = self.with_roots(|heap| heap.alloc_object(Some(prototype)))?;
        self.stack.push(Value::Object(record));
        let result = (|| {
            self.define_data(record, "segment", Value::String(segment), true, true, true)?;
            self.define_data(
                record,
                "index",
                Value::Number(start as f64),
                true,
                true,
                true,
            )?;
            self.define_data(
                record,
                "input",
                Value::String(data.input.clone()),
                true,
                true,
                true,
            )?;
            if let Some(is_word_like) = is_word_like {
                self.define_data(
                    record,
                    "isWordLike",
                    Value::Bool(is_word_like),
                    true,
                    true,
                    true,
                )?;
            }
            Ok(Value::Object(record))
        })();
        self.stack.pop();
        result
    }

    pub(super) fn segments_containing(
        &mut self,
        receiver: &Value,
        index: &Value,
    ) -> Result<Value, RuntimeError> {
        let data = self.segments_data(receiver)?;
        let index = self.coerce_number(index)?;
        let index = if index.is_nan() { 0.0 } else { index.trunc() };
        if !index.is_finite() || index < 0.0 || index >= data.input.as_code_units().len() as f64 {
            return Ok(Value::Undefined);
        }
        data.containing(index as usize)
            .map(|record| self.segment_record(&data, record))
            .transpose()?
            .map_or(Ok(Value::Undefined), Ok)
    }

    pub(super) fn segments_iterator(&mut self, receiver: &Value) -> Result<Value, RuntimeError> {
        let data = self.segments_data(receiver)?;
        let (_, prototype) = self.segmenter_internal_prototypes()?;
        self.with_roots(|heap| heap.alloc_segment_iterator(data, prototype))
            .map(Value::Object)
    }

    pub(super) fn segment_iterator_next(
        &mut self,
        receiver: &Value,
    ) -> Result<Value, RuntimeError> {
        let Value::Object(iterator) = receiver else {
            return Err(RuntimeError::TypeError(
                "Segmenter iterator next requires an iterator".into(),
            ));
        };
        let next = self.heap.segment_iterator_next(*iterator)?;
        if next.is_none() && !self.heap.is_segment_iterator(*iterator)? {
            return Err(RuntimeError::TypeError(
                "Segmenter iterator next requires a Segmenter iterator".into(),
            ));
        }
        match next {
            Some((data, index)) => self
                .segment_record(&data, index)
                .and_then(|value| self.iterator_result(value, false)),
            None => self.iterator_result(Value::Undefined, true),
        }
    }

    pub(super) fn segmenter_resolved_options(
        &mut self,
        receiver: &Value,
    ) -> Result<Value, RuntimeError> {
        let data = self.segmenter_data(receiver)?;
        let resolved = data.resolved_options();
        let prototype = self.object_prototype;
        let result = self.with_roots(|heap| heap.alloc_object(Some(prototype)))?;
        self.stack.push(Value::Object(result));
        for (key, value) in [
            ("locale", Value::String(resolved.locale.clone().into())),
            (
                "granularity",
                Value::String(
                    match resolved.granularity {
                        blueice_ecma402::SegmenterGranularity::Grapheme => "grapheme",
                        blueice_ecma402::SegmenterGranularity::Word => "word",
                        blueice_ecma402::SegmenterGranularity::Sentence => "sentence",
                    }
                    .into(),
                ),
            ),
        ] {
            self.define_data(result, key, value, true, true, true)?;
        }
        Ok(Value::Object(result))
    }

    pub(super) fn number_format_data(
        &self,
        value: &Value,
    ) -> Result<Rc<intl::NumberFormat>, RuntimeError> {
        if let Value::Object(id) = value {
            if let Some(data) = self.heap.number_format(*id)? {
                return Ok(data);
            }
        }
        Err(RuntimeError::TypeError(
            "receiver is not an Intl.NumberFormat".into(),
        ))
    }

    pub(super) fn number_format_format_getter(
        &mut self,
        receiver: &Value,
    ) -> Result<Value, RuntimeError> {
        self.number_format_data(receiver)?;
        let id = receiver.object_id().unwrap();
        if let Some(function) = self.heap.number_format_format(id) {
            return Ok(Value::Object(function));
        }
        let constructor = self.string_intrinsics()?.0;
        let prototype = self.heap.prototype(constructor)?.unwrap();
        let target = self.with_roots(|heap| {
            heap.alloc_native_function(NativeFunction::NumberFormatFormat, "", prototype)
        })?;
        let bound = crate::heap::BoundFunction {
            target,
            this: receiver.clone(),
            args: vec![],
            constructible: false,
        };
        let function = self.with_roots(|heap| heap.alloc_bound_function(bound, Some(prototype)))?;
        self.stack.push(Value::Object(function));
        self.define_data(function, "length", Value::Number(1.0), false, false, true)?;
        self.define_data(
            function,
            "name",
            Value::String("".into()),
            false,
            false,
            true,
        )?;
        self.heap.set_number_format_format(id, function);
        Ok(Value::Object(function))
    }

    pub(super) fn number_format_resolved_options(
        &mut self,
        receiver: &Value,
    ) -> Result<Value, RuntimeError> {
        let data = self.number_format_data(receiver)?;
        let resolved = data.resolved_options();
        let prototype = self.object_prototype;
        let result = self.with_roots(|heap| heap.alloc_object(Some(prototype)))?;
        self.stack.push(Value::Object(result));
        for (key, value) in [
            ("locale", Value::String(resolved.locale.as_str().into())),
            (
                "numberingSystem",
                Value::String(resolved.numbering_system.as_str().into()),
            ),
            ("style", Value::String("decimal".into())),
            (
                "useGrouping",
                Value::String(
                    match resolved.use_grouping {
                        blueice_ecma402::NumberGrouping::Auto => "auto",
                        blueice_ecma402::NumberGrouping::Never => "false",
                        blueice_ecma402::NumberGrouping::Always => "always",
                        blueice_ecma402::NumberGrouping::Min2 => "min2",
                    }
                    .into(),
                ),
            ),
            (
                "minimumFractionDigits",
                Value::Number(resolved.minimum_fraction_digits.into()),
            ),
            (
                "maximumFractionDigits",
                Value::Number(resolved.maximum_fraction_digits.into()),
            ),
        ] {
            self.define_data(result, key, value, true, true, true)?;
        }
        Ok(Value::Object(result))
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

    pub(super) fn collator_resolved_options(
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
        locale: intl::CanonicalLocale,
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
            self.locale_data(receiver)?.locale.as_str().into(),
        ))
    }

    pub(super) fn locale_transform(
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
        let mut locale = data.locale.locale().clone();
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
        self.locale_instance(
            intl::canonicalize(&JsString::from(locale.to_string()))?,
            prototype,
        )
    }

    pub(super) fn locale_getter(
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
        let locale = self.locale_data(receiver)?.locale.locale().clone();
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
