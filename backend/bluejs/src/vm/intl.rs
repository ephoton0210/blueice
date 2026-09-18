// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

use super::*;
use crate::heap::{TemporalKind, TemporalValue};
use crate::intl;
use icu_locale_core::{
    extensions::unicode::Value as UnicodeValue,
    subtags::{Language, Region, Script, Variant, Variants},
    Locale,
};
use num_traits::ToPrimitive;
use std::rc::Rc;

/// The VM-side input to `ToDateTimeFormattable`. It keeps JavaScript coercion
/// and Temporal internal-slot recognition at the Realm boundary before the
/// host-neutral ECMA-402 service receives a typed input.
enum DateTimeFormatValue {
    Number(f64),
    Temporal(TemporalValue),
}

/// The VM-side `ToIntlMathematicalValue` result. This is intentionally kept
/// separate from `ToNumber`: NumberFormat must preserve exact decimal strings
/// and BigInts through `format`, `formatToParts`, and both range operations.
enum NumberFormatValue {
    Decimal(String),
    ScientificDecimal { significand: String, exponent: i16 },
    Number(f64),
}

/// Parses the finite base-10 subset of `StringNumericLiteral` that can be
/// represented exactly by the fixed-decimal provider. It intentionally runs
/// after `ToNumber`, so non-decimal strings retain ordinary Number semantics.
fn exact_decimal_intl_mathematical_value(value: &str) -> Option<NumberFormatValue> {
    let value =
        value.trim_matches(|character: char| character.is_whitespace() || character == '\u{feff}');
    let (significand, exponent) = match value.find(['e', 'E']) {
        Some(index) => {
            let (significand, exponent) = value.split_at(index);
            if exponent[1..].contains(['e', 'E']) {
                return None;
            }
            (significand, Some(exponent[1..].parse::<i16>().ok()?))
        }
        None => (value, None),
    };
    let significand = significand
        .strip_prefix('+')
        .or_else(|| significand.strip_prefix('-'))
        .unwrap_or(significand);
    let mut digits = 0;
    let mut decimal = false;
    for byte in significand.bytes() {
        if byte.is_ascii_digit() {
            digits += 1;
        } else if byte == b'.' && !decimal {
            decimal = true;
        } else {
            return None;
        }
    }
    (digits > 0).then(|| match exponent {
        Some(exponent) => NumberFormatValue::ScientificDecimal {
            significand: value[..value.find(['e', 'E']).unwrap()].into(),
            exponent,
        },
        None => NumberFormatValue::Decimal(value.into()),
    })
}

fn temporal_has_date_components(options: &blueice_ecma402::DateTimeFormatOptions) -> bool {
    options.weekday.is_some()
        || options.era.is_some()
        || options.year.is_some()
        || options.month.is_some()
        || options.day.is_some()
}

fn temporal_has_time_components(options: &blueice_ecma402::DateTimeFormatOptions) -> bool {
    options.day_period.is_some()
        || options.hour.is_some()
        || options.minute.is_some()
        || options.second.is_some()
        || options.fractional_second_digits.is_some()
}

fn clear_temporal_date_components(options: &mut blueice_ecma402::DateTimeFormatOptions) {
    options.weekday = None;
    options.era = None;
    options.year = None;
    options.month = None;
    options.day = None;
    options.date_style = None;
}

fn clear_temporal_time_components(options: &mut blueice_ecma402::DateTimeFormatOptions) {
    options.day_period = None;
    options.hour = None;
    options.minute = None;
    options.second = None;
    options.fractional_second_digits = None;
    options.time_style = None;
}

fn apply_temporal_partial_date_style(
    options: &mut blueice_ecma402::DateTimeFormatOptions,
    style: blueice_ecma402::DateTimeStyle,
    includes_year: bool,
) {
    use blueice_ecma402::DateTimeWidth::{Long, Numeric, Short, TwoDigit};

    // A dateStyle normally expands to YMD. Temporal.PlainMonthDay and
    // Temporal.PlainYearMonth retain the style's field widths while omitting
    // the reference field they do not model. This must happen before ICU4X's
    // semantic skeleton is built; deleting a rendered year afterwards would
    // leave the wrong month width and punctuation.
    options.date_style = None;
    options.month = Some(match style {
        blueice_ecma402::DateTimeStyle::Full | blueice_ecma402::DateTimeStyle::Long => Long,
        blueice_ecma402::DateTimeStyle::Medium => Short,
        blueice_ecma402::DateTimeStyle::Short => Numeric,
    });
    options.day = (!includes_year).then_some(Numeric);
    options.year = includes_year.then_some(if style == blueice_ecma402::DateTimeStyle::Short {
        TwoDigit
    } else {
        Numeric
    });
}

fn temporal_default_components(
    options: &mut blueice_ecma402::DateTimeFormatOptions,
    kind: TemporalKind,
) {
    use blueice_ecma402::DateTimeWidth::Numeric;
    match kind {
        TemporalKind::Duration => unreachable!("Temporal.Duration is not date-time formattable"),
        // Leaving the components absent selects DateTimeFormat's legacy
        // default numeric-date pattern. That matters for interval patterns:
        // en-US repeats both default-date endpoints rather than collapsing a
        // shared year.
        TemporalKind::PlainDate => {}
        TemporalKind::PlainDateTime => {
            options.year = Some(Numeric);
            options.month = Some(Numeric);
            options.day = Some(Numeric);
            options.hour = Some(Numeric);
            options.minute = Some(Numeric);
            options.second = Some(Numeric);
        }
        TemporalKind::PlainMonthDay => {
            options.month = Some(Numeric);
            options.day = Some(Numeric);
        }
        TemporalKind::PlainTime => {
            options.hour = Some(Numeric);
            options.minute = Some(Numeric);
            options.second = Some(Numeric);
        }
        TemporalKind::PlainYearMonth => {
            options.year = Some(Numeric);
            options.month = Some(Numeric);
        }
        TemporalKind::Instant => {
            options.year = Some(Numeric);
            options.month = Some(Numeric);
            options.day = Some(Numeric);
            options.hour = Some(Numeric);
            options.minute = Some(Numeric);
            options.second = Some(Numeric);
        }
        TemporalKind::ZonedDateTime => {}
    }
}

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
                "supportedValuesOf",
                1,
                NativeFunction::SupportedValuesOf,
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
            // constructors. Their locale data and formatting algorithms live
            // in the host-neutral blueice-ecma402 crate.
            for (name, service) in [
                ("NumberFormat", native::IntlService::Number),
                ("DateTimeFormat", native::IntlService::DateTime),
                ("DisplayNames", native::IntlService::DisplayNames),
                ("DurationFormat", native::IntlService::Duration),
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
                        self.install_native(
                            prototype,
                            function_prototype,
                            "formatToParts",
                            1,
                            NativeFunction::NumberFormatFormatToParts,
                        )?;
                        self.install_native(
                            prototype,
                            function_prototype,
                            "formatRange",
                            2,
                            NativeFunction::NumberFormatFormatRange,
                        )?;
                        self.install_native(
                            prototype,
                            function_prototype,
                            "formatRangeToParts",
                            2,
                            NativeFunction::NumberFormatFormatRangeToParts,
                        )?;
                        self.globals
                            .insert("%Intl.NumberFormat%".into(), constructor);
                    } else if service == native::IntlService::DateTime {
                        self.define_data(
                            prototype,
                            JsSymbol::well_known("toStringTag"),
                            Value::String("Intl.DateTimeFormat".into()),
                            false,
                            false,
                            true,
                        )?;
                        self.install_native(
                            constructor,
                            function_prototype,
                            "supportedLocalesOf",
                            1,
                            NativeFunction::DateTimeFormatSupportedLocales,
                        )?;
                        self.install_native(
                            prototype,
                            function_prototype,
                            "resolvedOptions",
                            0,
                            NativeFunction::DateTimeFormatResolvedOptions,
                        )?;
                        self.install_getter(
                            prototype,
                            function_prototype,
                            "format".into(),
                            "get format",
                            NativeFunction::DateTimeFormatFormatGetter,
                        )?;
                        self.install_native(
                            prototype,
                            function_prototype,
                            "formatToParts",
                            1,
                            NativeFunction::DateTimeFormatFormatToParts,
                        )?;
                        self.install_native(
                            prototype,
                            function_prototype,
                            "formatRange",
                            2,
                            NativeFunction::DateTimeFormatFormatRange,
                        )?;
                        self.install_native(
                            prototype,
                            function_prototype,
                            "formatRangeToParts",
                            2,
                            NativeFunction::DateTimeFormatFormatRangeToParts,
                        )?;
                        self.globals
                            .insert("%Intl.DateTimeFormat%".into(), constructor);
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
                    } else if service == native::IntlService::Duration {
                        self.define_data(
                            prototype,
                            JsSymbol::well_known("toStringTag"),
                            Value::String("Intl.DurationFormat".into()),
                            false,
                            false,
                            true,
                        )?;
                        self.install_native(
                            constructor,
                            function_prototype,
                            "supportedLocalesOf",
                            1,
                            NativeFunction::DurationFormatSupportedLocales,
                        )?;
                        self.install_native(
                            prototype,
                            function_prototype,
                            "resolvedOptions",
                            0,
                            NativeFunction::DurationFormatResolvedOptions,
                        )?;
                        self.install_native(
                            prototype,
                            function_prototype,
                            "format",
                            1,
                            NativeFunction::DurationFormatFormat,
                        )?;
                        self.install_native(
                            prototype,
                            function_prototype,
                            "formatToParts",
                            1,
                            NativeFunction::DurationFormatFormatToParts,
                        )?;
                        self.globals
                            .insert("%Intl.DurationFormat%".into(), constructor);
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

    /// The legacy constructor algorithms use `GetOptionsObject`: only an
    /// ordinary object is accepted when an options value is supplied.
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

    /// `InitializeNumberFormat` uses current ECMA-402's
    /// `CoerceOptionsToObject`, so primitives are boxed and can expose
    /// observable inherited properties. `null` still fails `ToObject`.
    fn number_format_constructor_options(&mut self, value: &Value) -> Result<Value, RuntimeError> {
        self.intl_options(value)
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

    pub(super) fn supported_values_of(&mut self, key: &Value) -> Result<Value, RuntimeError> {
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
        notation: blueice_ecma402::NumberNotation,
    ) -> Result<blueice_ecma402::NumberGrouping, RuntimeError> {
        let value = self.get_property(options, &"useGrouping".into())?;
        match value {
            Value::Undefined => Ok(if notation == blueice_ecma402::NumberNotation::Compact {
                blueice_ecma402::NumberGrouping::Min2
            } else {
                blueice_ecma402::NumberGrouping::Auto
            }),
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
                "false" | "true" => Ok(blueice_ecma402::NumberGrouping::Auto),
                "" | "null" | "0" => Ok(blueice_ecma402::NumberGrouping::Never),
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

    fn number_minimum_integer_digits_option(
        &mut self,
        options: &Value,
    ) -> Result<u8, RuntimeError> {
        let value = self.get_property(options, &"minimumIntegerDigits".into())?;
        if value == Value::Undefined {
            return Ok(1);
        }
        let number = self.coerce_number(&value)?;
        if !number.is_finite() || !(1.0..=21.0).contains(&number) {
            return Err(RuntimeError::RangeError(
                "invalid minimumIntegerDigits option".into(),
            ));
        }
        Ok(number.floor() as u8)
    }

    fn number_style(
        &mut self,
        options: &Value,
    ) -> Result<blueice_ecma402::NumberFormatStyle, RuntimeError> {
        match self
            .string_option(
                options,
                "style",
                &["decimal", "percent", "currency", "unit"],
            )?
            .as_deref()
        {
            None | Some("decimal") => Ok(blueice_ecma402::NumberFormatStyle::Decimal),
            Some("percent") => Ok(blueice_ecma402::NumberFormatStyle::Percent),
            Some("currency") => Ok(blueice_ecma402::NumberFormatStyle::Currency),
            Some("unit") => Ok(blueice_ecma402::NumberFormatStyle::Unit),
            Some(_) => unreachable!("string_option validates NumberFormat style"),
        }
    }

    fn number_unit(
        &mut self,
        options: &Value,
        style: blueice_ecma402::NumberFormatStyle,
    ) -> Result<Option<blueice_ecma402::NumberFormatUnit>, RuntimeError> {
        let unit = self
            .string_option(options, "unit", &[])?
            .as_deref()
            .map(|unit| {
                blueice_ecma402::NumberFormatUnit::parse(unit)
                    .ok_or_else(|| RuntimeError::RangeError("invalid unit option".into()))
            })
            .transpose()?;
        match (style, unit) {
            (
                blueice_ecma402::NumberFormatStyle::Decimal
                | blueice_ecma402::NumberFormatStyle::Percent
                | blueice_ecma402::NumberFormatStyle::Currency,
                _,
            ) => Ok(None),
            (blueice_ecma402::NumberFormatStyle::Unit, None) => Err(RuntimeError::TypeError(
                "unit is required when style is unit".into(),
            )),
            (blueice_ecma402::NumberFormatStyle::Unit, Some(unit)) => Ok(Some(unit)),
        }
    }

    fn number_currency(
        &mut self,
        options: &Value,
        style: blueice_ecma402::NumberFormatStyle,
    ) -> Result<Option<blueice_ecma402::NumberCurrencyOptions>, RuntimeError> {
        let code = self.string_option(options, "currency", &[])?;
        let display = match self
            .string_option(
                options,
                "currencyDisplay",
                &["code", "symbol", "narrowSymbol", "name"],
            )?
            .as_deref()
        {
            None | Some("symbol") => blueice_ecma402::NumberCurrencyDisplay::Symbol,
            Some("code") => blueice_ecma402::NumberCurrencyDisplay::Code,
            Some("narrowSymbol") => blueice_ecma402::NumberCurrencyDisplay::NarrowSymbol,
            Some("name") => blueice_ecma402::NumberCurrencyDisplay::Name,
            Some(_) => unreachable!("string_option validates NumberFormat currencyDisplay"),
        };
        let sign = match self
            .string_option(options, "currencySign", &["standard", "accounting"])?
            .as_deref()
        {
            None | Some("standard") => blueice_ecma402::NumberCurrencySign::Standard,
            Some("accounting") => blueice_ecma402::NumberCurrencySign::Accounting,
            Some(_) => unreachable!("string_option validates NumberFormat currencySign"),
        };
        let code = code
            .map(|code| {
                if code.len() != 3 || !code.bytes().all(|byte| byte.is_ascii_alphabetic()) {
                    Err(RuntimeError::RangeError("invalid currency option".into()))
                } else {
                    Ok(code.to_ascii_uppercase())
                }
            })
            .transpose()?;
        if style != blueice_ecma402::NumberFormatStyle::Currency {
            return Ok(None);
        }
        let code = code.ok_or_else(|| {
            RuntimeError::TypeError("currency is required when style is currency".into())
        })?;
        Ok(Some(blueice_ecma402::NumberCurrencyOptions {
            code,
            display,
            sign,
        }))
    }

    fn number_unit_display(
        &mut self,
        options: &Value,
    ) -> Result<blueice_ecma402::NumberUnitDisplay, RuntimeError> {
        match self
            .string_option(options, "unitDisplay", &["short", "narrow", "long"])?
            .as_deref()
        {
            None | Some("short") => Ok(blueice_ecma402::NumberUnitDisplay::Short),
            Some("narrow") => Ok(blueice_ecma402::NumberUnitDisplay::Narrow),
            Some("long") => Ok(blueice_ecma402::NumberUnitDisplay::Long),
            Some(_) => unreachable!("string_option validates NumberFormat unitDisplay"),
        }
    }

    fn number_rounding_mode(
        &mut self,
        options: &Value,
    ) -> Result<blueice_ecma402::NumberRoundingMode, RuntimeError> {
        match self
            .string_option(
                options,
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
            .as_deref()
        {
            None | Some("halfExpand") => Ok(blueice_ecma402::NumberRoundingMode::HalfExpand),
            Some("ceil") => Ok(blueice_ecma402::NumberRoundingMode::Ceil),
            Some("floor") => Ok(blueice_ecma402::NumberRoundingMode::Floor),
            Some("expand") => Ok(blueice_ecma402::NumberRoundingMode::Expand),
            Some("trunc") => Ok(blueice_ecma402::NumberRoundingMode::Trunc),
            Some("halfCeil") => Ok(blueice_ecma402::NumberRoundingMode::HalfCeil),
            Some("halfFloor") => Ok(blueice_ecma402::NumberRoundingMode::HalfFloor),
            Some("halfTrunc") => Ok(blueice_ecma402::NumberRoundingMode::HalfTrunc),
            Some("halfEven") => Ok(blueice_ecma402::NumberRoundingMode::HalfEven),
            Some(_) => unreachable!("string_option validates NumberFormat roundingMode"),
        }
    }

    /// Reads NumberFormat's notation option at its standard observable point.
    /// Formatting extensions consume this resolved value in their own service
    /// slices; retaining it here keeps the constructor boundary spec ordered.
    fn number_notation(
        &mut self,
        options: &Value,
    ) -> Result<blueice_ecma402::NumberNotation, RuntimeError> {
        match self
            .string_option(
                options,
                "notation",
                &["standard", "scientific", "engineering", "compact"],
            )?
            .as_deref()
        {
            None | Some("standard") => Ok(blueice_ecma402::NumberNotation::Standard),
            Some("scientific") => Ok(blueice_ecma402::NumberNotation::Scientific),
            Some("engineering") => Ok(blueice_ecma402::NumberNotation::Engineering),
            Some("compact") => Ok(blueice_ecma402::NumberNotation::Compact),
            Some(_) => unreachable!("string_option validates NumberFormat notation"),
        }
    }

    /// Validates the Unicode type grammar accepted by the `numberingSystem`
    /// option. The host-neutral locale service applies a supported value when
    /// its numbering-system data is selected.
    fn number_numbering_system(&mut self, options: &Value) -> Result<Option<String>, RuntimeError> {
        let Some(value) = self.string_option(options, "numberingSystem", &[])? else {
            return Ok(None);
        };
        if !value.split('-').all(|part| {
            (3..=8).contains(&part.len()) && part.bytes().all(|byte| byte.is_ascii_alphanumeric())
        }) {
            return Err(RuntimeError::RangeError(
                "invalid numberingSystem option".into(),
            ));
        }
        Ok(Some(value.to_ascii_lowercase()))
    }

    /// Reads `compactDisplay` even for the non-compact notation branches, as
    /// required by InitializeNumberFormat's observable option sequence.
    fn number_compact_display(
        &mut self,
        options: &Value,
    ) -> Result<blueice_ecma402::NumberCompactDisplay, RuntimeError> {
        match self
            .string_option(options, "compactDisplay", &["short", "long"])?
            .as_deref()
        {
            None | Some("short") => Ok(blueice_ecma402::NumberCompactDisplay::Short),
            Some("long") => Ok(blueice_ecma402::NumberCompactDisplay::Long),
            Some(_) => unreachable!("string_option validates NumberFormat compactDisplay"),
        }
    }

    fn number_rounding_increment(&mut self, options: &Value) -> Result<u16, RuntimeError> {
        let value = self.get_property(options, &"roundingIncrement".into())?;
        if value == Value::Undefined {
            return Ok(1);
        }
        let value = self.coerce_number(&value)?;
        if !value.is_finite() || value.fract() != 0.0 || !(1.0..=5000.0).contains(&value) {
            return Err(RuntimeError::RangeError(
                "invalid roundingIncrement option".into(),
            ));
        }
        let value = value as u16;
        if !matches!(
            value,
            1 | 2 | 5 | 10 | 20 | 25 | 50 | 100 | 200 | 250 | 500 | 1000 | 2000 | 2500 | 5000
        ) {
            return Err(RuntimeError::RangeError(
                "invalid roundingIncrement option".into(),
            ));
        }
        Ok(value)
    }

    fn number_trailing_zero_display(
        &mut self,
        options: &Value,
    ) -> Result<blueice_ecma402::NumberTrailingZeroDisplay, RuntimeError> {
        match self
            .string_option(options, "trailingZeroDisplay", &["auto", "stripIfInteger"])?
            .as_deref()
        {
            None | Some("auto") => Ok(blueice_ecma402::NumberTrailingZeroDisplay::Auto),
            Some("stripIfInteger") => {
                Ok(blueice_ecma402::NumberTrailingZeroDisplay::StripIfInteger)
            }
            Some(_) => unreachable!("string_option validates NumberFormat trailingZeroDisplay"),
        }
    }

    fn number_significant_digits_option(
        &mut self,
        options: &Value,
        name: &str,
    ) -> Result<Option<u8>, RuntimeError> {
        let value = self.get_property(options, &name.into())?;
        if value == Value::Undefined {
            return Ok(None);
        }
        let value = self.coerce_number(&value)?;
        if !value.is_finite() || value.fract() != 0.0 || !(1.0..=21.0).contains(&value) {
            return Err(RuntimeError::RangeError(format!("invalid {name} option")));
        }
        Ok(Some(value as u8))
    }

    fn number_rounding_priority(
        &mut self,
        options: &Value,
    ) -> Result<blueice_ecma402::NumberRoundingPriority, RuntimeError> {
        match self
            .string_option(
                options,
                "roundingPriority",
                &["auto", "morePrecision", "lessPrecision"],
            )?
            .as_deref()
        {
            None | Some("auto") => Ok(blueice_ecma402::NumberRoundingPriority::Auto),
            Some("morePrecision") => Ok(blueice_ecma402::NumberRoundingPriority::MorePrecision),
            Some("lessPrecision") => Ok(blueice_ecma402::NumberRoundingPriority::LessPrecision),
            Some(_) => unreachable!("string_option validates NumberFormat roundingPriority"),
        }
    }

    fn validate_number_precision_options(
        rounding_increment: u16,
        rounding_priority: blueice_ecma402::NumberRoundingPriority,
        minimum_significant_digits: Option<u8>,
        maximum_significant_digits: Option<u8>,
    ) -> Result<(), RuntimeError> {
        if rounding_increment != 1
            && (rounding_priority != blueice_ecma402::NumberRoundingPriority::Auto
                || minimum_significant_digits.is_some()
                || maximum_significant_digits.is_some())
        {
            return Err(RuntimeError::TypeError(
                "roundingIncrement is incompatible with significant-digit rounding".into(),
            ));
        }
        Ok(())
    }

    fn number_sign_display(
        &mut self,
        options: &Value,
    ) -> Result<blueice_ecma402::NumberSignDisplay, RuntimeError> {
        match self
            .string_option(
                options,
                "signDisplay",
                &["auto", "never", "always", "exceptZero", "negative"],
            )?
            .as_deref()
        {
            None | Some("auto") => Ok(blueice_ecma402::NumberSignDisplay::Auto),
            Some("never") => Ok(blueice_ecma402::NumberSignDisplay::Never),
            Some("always") => Ok(blueice_ecma402::NumberSignDisplay::Always),
            Some("exceptZero") => Ok(blueice_ecma402::NumberSignDisplay::ExceptZero),
            Some("negative") => Ok(blueice_ecma402::NumberSignDisplay::Negative),
            Some(_) => unreachable!("string_option validates NumberFormat signDisplay"),
        }
    }

    pub(super) fn resolve_number_format(
        &mut self,
        locales: &Value,
        options: &Value,
    ) -> Result<Rc<intl::NumberFormat>, RuntimeError> {
        let locales = self.canonical_locales(locales)?;
        let options = self.number_format_constructor_options(options)?;
        let locale_matcher = self.locale_matcher(&options)?;
        let requested_numbering_system = self.number_numbering_system(&options)?;
        let style = self.number_style(&options)?;
        let currency = self.number_currency(&options, style)?;
        let unit = self.number_unit(&options, style)?;
        let unit_display = self.number_unit_display(&options)?;
        let notation = self.number_notation(&options)?;
        let minimum_integer_digits = self.number_minimum_integer_digits_option(&options)?;
        let minimum_fraction_digits =
            self.number_fraction_digits_option(&options, "minimumFractionDigits")?;
        let maximum_fraction_digits =
            self.number_fraction_digits_option(&options, "maximumFractionDigits")?;
        let minimum_significant_digits =
            self.number_significant_digits_option(&options, "minimumSignificantDigits")?;
        let maximum_significant_digits =
            self.number_significant_digits_option(&options, "maximumSignificantDigits")?;
        let rounding_increment = self.number_rounding_increment(&options)?;
        let rounding_mode = self.number_rounding_mode(&options)?;
        let rounding_priority = self.number_rounding_priority(&options)?;
        let trailing_zero_display = self.number_trailing_zero_display(&options)?;
        let compact_display = self.number_compact_display(&options)?;
        let use_grouping = self.number_grouping(&options, notation)?;
        let sign_display = self.number_sign_display(&options)?;
        Self::validate_number_precision_options(
            rounding_increment,
            rounding_priority,
            minimum_significant_digits,
            maximum_significant_digits,
        )?;
        let options = blueice_ecma402::NumberFormatOptions {
            locale_matcher,
            use_grouping,
            style,
            notation,
            compact_display,
            unit,
            unit_display,
            minimum_integer_digits,
            minimum_fraction_digits,
            maximum_fraction_digits,
            rounding_mode,
            rounding_priority,
            sign_display,
        };
        blueice_ecma402::NumberFormat::try_new_with_construction_options(
            &locales,
            options,
            blueice_ecma402::NumberFormatConstructionOptions {
                rounding_increment,
                minimum_significant_digits,
                maximum_significant_digits,
                trailing_zero_display,
                currency,
                numbering_system: requested_numbering_system,
            },
        )
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
        receiver: &Value,
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
        let legacy_receiver =
            (!construct && self.intl_legacy_receiver(receiver, default)?).then(|| receiver.clone());
        let prototype = if construct {
            self.constructor_prototype(default)?
        } else {
            default
        };
        self.stack.push(Value::Object(prototype));
        let data =
            self.resolve_number_format(native::argument(args, 0), native::argument(args, 1))?;
        let number_format = self.with_roots(|heap| heap.alloc_number_format(data, prototype))?;
        let Some(legacy_receiver) = legacy_receiver else {
            return Ok(Value::Object(number_format));
        };

        let fallback_symbol = self.intl_legacy_fallback_symbol();
        let legacy_id = legacy_receiver
            .object_id()
            .expect("legacy NumberFormat receiver is an object");
        self.stack.push(Value::Object(number_format));
        if !self.object_define_own_property(
            legacy_id,
            fallback_symbol.into(),
            PropertyDescriptor::data(Value::Object(number_format), false, false, false),
        )? {
            return Err(RuntimeError::TypeError(
                "cannot define IntlLegacyConstructedSymbol property".into(),
            ));
        }
        Ok(legacy_receiver)
    }

    fn date_time_width(
        &mut self,
        options: &Value,
        name: &str,
        allowed: &[&str],
    ) -> Result<Option<blueice_ecma402::DateTimeWidth>, RuntimeError> {
        self.string_option(options, name, allowed)?
            .map_or(Ok(None), |value| {
                Ok(Some(match value.as_str() {
                    "numeric" => blueice_ecma402::DateTimeWidth::Numeric,
                    "2-digit" => blueice_ecma402::DateTimeWidth::TwoDigit,
                    "short" => blueice_ecma402::DateTimeWidth::Short,
                    "long" => blueice_ecma402::DateTimeWidth::Long,
                    "narrow" => blueice_ecma402::DateTimeWidth::Narrow,
                    _ => unreachable!("string_option validates date-time field widths"),
                }))
            })
    }

    fn date_time_style(
        &mut self,
        options: &Value,
        name: &str,
    ) -> Result<Option<blueice_ecma402::DateTimeStyle>, RuntimeError> {
        self.string_option(options, name, &["full", "long", "medium", "short"])?
            .map_or(Ok(None), |value| {
                Ok(Some(match value.as_str() {
                    "full" => blueice_ecma402::DateTimeStyle::Full,
                    "long" => blueice_ecma402::DateTimeStyle::Long,
                    "medium" => blueice_ecma402::DateTimeStyle::Medium,
                    "short" => blueice_ecma402::DateTimeStyle::Short,
                    _ => unreachable!("string_option validates date-time styles"),
                }))
            })
    }

    fn date_time_fractional_second_digits(
        &mut self,
        options: &Value,
    ) -> Result<Option<u8>, RuntimeError> {
        let value = self.get_property(options, &"fractionalSecondDigits".into())?;
        if value == Value::Undefined {
            return Ok(None);
        }
        let value = self.coerce_number(&value)?;
        // GetNumberOption first validates the numeric value against its
        // inclusive bounds and only then applies floor. Thus 2.9 resolves to
        // 2, whereas 3.000001 remains out of range.
        if !value.is_finite() || !(1.0..=3.0).contains(&value) {
            return Err(RuntimeError::RangeError(
                "invalid fractionalSecondDigits option".into(),
            ));
        }
        Ok(Some(value.floor() as u8))
    }

    fn date_time_format_options(
        &mut self,
        value: &Value,
    ) -> Result<blueice_ecma402::DateTimeFormatOptions, RuntimeError> {
        let options = self.intl_options(value)?;
        let locale_matcher = self.locale_matcher(&options)?;
        let calendar = self.string_option(&options, "calendar", &[])?;
        let numbering_system = self.string_option(&options, "numberingSystem", &[])?;
        if let Some(value) = &numbering_system {
            if !(3..=8).contains(&value.len())
                || !value
                    .bytes()
                    .all(|character| character.is_ascii_alphanumeric())
            {
                return Err(RuntimeError::RangeError(
                    "invalid numberingSystem option".into(),
                ));
            }
        }
        let hour12 = self.get_property(&options, &"hour12".into())?;
        let hour12 = if hour12 == Value::Undefined {
            None
        } else {
            Some(self.to_boolean(&hour12)?)
        };
        let hour_cycle =
            self.string_option(&options, "hourCycle", &["h11", "h12", "h23", "h24"])?;
        let time_zone = self.string_option(&options, "timeZone", &[])?;
        let weekday = self.date_time_width(&options, "weekday", &["short", "long", "narrow"])?;
        let era = self.date_time_width(&options, "era", &["short", "long", "narrow"])?;
        let year = self.date_time_width(&options, "year", &["numeric", "2-digit"])?;
        let month = self.date_time_width(
            &options,
            "month",
            &["numeric", "2-digit", "short", "long", "narrow"],
        )?;
        let day = self.date_time_width(&options, "day", &["numeric", "2-digit"])?;
        let day_period =
            self.date_time_width(&options, "dayPeriod", &["narrow", "short", "long"])?;
        let hour = self.date_time_width(&options, "hour", &["numeric", "2-digit"])?;
        let minute = self.date_time_width(&options, "minute", &["numeric", "2-digit"])?;
        let second = self.date_time_width(&options, "second", &["numeric", "2-digit"])?;
        let fractional_second_digits = self.date_time_fractional_second_digits(&options)?;
        let time_zone_name = self.string_option(
            &options,
            "timeZoneName",
            &[
                "long",
                "short",
                "shortOffset",
                "longOffset",
                "shortGeneric",
                "longGeneric",
            ],
        )?;
        let format_matcher =
            match self.string_option(&options, "formatMatcher", &["basic", "best fit"])? {
                Some(value) if value == "basic" => blueice_ecma402::DateTimeFormatMatcher::Basic,
                Some(value) if value == "best fit" => {
                    blueice_ecma402::DateTimeFormatMatcher::BestFit
                }
                None => blueice_ecma402::DateTimeFormatMatcher::BestFit,
                Some(_) => unreachable!("string_option validates formatMatcher values"),
            };
        let date_style = self.date_time_style(&options, "dateStyle")?;
        let time_style = self.date_time_style(&options, "timeStyle")?;
        let has_components = weekday.is_some()
            || era.is_some()
            || year.is_some()
            || month.is_some()
            || day.is_some()
            || day_period.is_some()
            || hour.is_some()
            || minute.is_some()
            || second.is_some()
            || fractional_second_digits.is_some()
            || time_zone_name.is_some();
        if (date_style.is_some() || time_style.is_some()) && has_components {
            return Err(RuntimeError::TypeError(
                "dateStyle and timeStyle cannot be used with date-time component options".into(),
            ));
        }
        Ok(blueice_ecma402::DateTimeFormatOptions {
            locale_matcher,
            format_matcher,
            calendar,
            numbering_system,
            hour_cycle,
            hour12,
            time_zone,
            weekday,
            era,
            year,
            month,
            day,
            day_period,
            hour,
            minute,
            second,
            fractional_second_digits,
            time_zone_name,
            date_style,
            time_style,
        })
    }

    fn resolve_date_time_format(
        &mut self,
        locales: &Value,
        options: &Value,
    ) -> Result<Rc<intl::DateTimeFormat>, RuntimeError> {
        let locales = self.canonical_locales(locales)?;
        let options = self.date_time_format_options(options)?;
        blueice_ecma402::DateTimeFormat::try_new(&locales, options)
            .map(Rc::new)
            .map_err(|error| RuntimeError::RangeError(error.to_string()))
    }

    /// Shared `Date.prototype.toLocale*` bridge. The legacy Date methods do
    /// not use the fixed English Date string helpers: they run the same
    /// DateTimeFormat resolution as an explicit formatter after
    /// `ToDateTimeOptions` supplies only the defaults required by that
    /// particular method.
    pub(super) fn date_to_locale_string(
        &mut self,
        time: f64,
        args: &[Value],
        default_date: bool,
        default_time: bool,
    ) -> Result<Value, RuntimeError> {
        if !time.is_finite() {
            return Ok(Value::String("Invalid Date".into()));
        }
        let mut options = self.date_time_format_options(native::argument(args, 1))?;
        let has_date = options.weekday.is_some()
            || options.year.is_some()
            || options.month.is_some()
            || options.day.is_some();
        let has_time = options.day_period.is_some()
            || options.hour.is_some()
            || options.minute.is_some()
            || options.second.is_some()
            || options.fractional_second_digits.is_some();
        let has_style = options.date_style.is_some() || options.time_style.is_some();

        // ToDateTimeOptions has three distinct defaulting modes. `any` (the
        // all-locales method) defaults only when neither group was supplied;
        // the date/time methods independently fill their required group while
        // retaining any caller-supplied fields from the other group.
        let need_date = default_date
            && !has_style
            && if default_time {
                !has_date && !has_time
            } else {
                !has_date
            };
        let need_time = default_time
            && !has_style
            && if default_date {
                !has_date && !has_time
            } else {
                !has_time
            };
        if need_date {
            options.year = Some(blueice_ecma402::DateTimeWidth::Numeric);
            options.month = Some(blueice_ecma402::DateTimeWidth::Numeric);
            options.day = Some(blueice_ecma402::DateTimeWidth::Numeric);
        }
        if need_time {
            options.hour = Some(blueice_ecma402::DateTimeWidth::Numeric);
            options.minute = Some(blueice_ecma402::DateTimeWidth::Numeric);
            options.second = Some(blueice_ecma402::DateTimeWidth::Numeric);
        }
        // Date.prototype.toLocale* runs ToDateTimeOptions before it delegates
        // to CreateDateTimeFormat, so an abrupt option getter wins over a
        // later locale coercion just as it does in the specification.
        let locales = self.canonical_locales(native::argument(args, 0))?;
        blueice_ecma402::DateTimeFormat::try_new(&locales, options)
            .and_then(|format| format.format(time))
            .map(|formatted| Value::String(formatted.into()))
            .map_err(|error| RuntimeError::RangeError(error.to_string()))
    }

    pub(super) fn date_time_format_supported_locales(
        &mut self,
        args: &[Value],
    ) -> Result<Value, RuntimeError> {
        let locales = self.canonical_locales(native::argument(args, 0))?;
        let options = self.intl_options(native::argument(args, 1))?;
        let locales = blueice_ecma402::supported_locales(
            blueice_ecma402::IntlService::DateTimeFormat,
            &locales,
            self.locale_matcher(&options)?,
        );
        self.array_from(
            locales
                .into_iter()
                .map(|locale| Value::String(locale.to_string().into()))
                .collect(),
        )
    }

    pub(super) fn create_date_time_format(
        &mut self,
        receiver: &Value,
        args: &[Value],
        construct: bool,
    ) -> Result<Value, RuntimeError> {
        self.intl_global()?;
        let constructor = self.globals["%Intl.DateTimeFormat%"];
        let default = self
            .heap
            .get(constructor, "prototype")?
            .object_id()
            .expect("Intl.DateTimeFormat.prototype is an object");
        let legacy_receiver =
            (!construct && self.intl_legacy_receiver(receiver, default)?).then(|| receiver.clone());
        let prototype = if construct {
            self.constructor_prototype(default)?
        } else {
            default
        };
        self.stack.push(Value::Object(prototype));
        let data =
            self.resolve_date_time_format(native::argument(args, 0), native::argument(args, 1))?;
        let date_time_format =
            self.with_roots(|heap| heap.alloc_date_time_format(data, prototype))?;
        let Some(legacy_receiver) = legacy_receiver else {
            return Ok(Value::Object(date_time_format));
        };

        // ECMA-402's normative-optional ChainDateTimeFormat mode preserves
        // the eligible call receiver, while a real DateTimeFormat object is
        // kept behind a per-realm non-enumerable, non-writable and
        // non-configurable Symbol property. Use the generic internal method
        // here so an eligible Proxy observes [[DefineOwnProperty]].
        let fallback_symbol = self.intl_legacy_fallback_symbol();
        let legacy_id = legacy_receiver
            .object_id()
            .expect("legacy DateTimeFormat receiver is an object");
        self.stack.push(Value::Object(date_time_format));
        if !self.object_define_own_property(
            legacy_id,
            fallback_symbol.into(),
            PropertyDescriptor::data(Value::Object(date_time_format), false, false, false),
        )? {
            return Err(RuntimeError::TypeError(
                "cannot define IntlLegacyConstructedSymbol property".into(),
            ));
        }
        Ok(legacy_receiver)
    }

    /// Returns whether the service prototype occurs strictly in `receiver`'s
    /// prototype chain. This is the receiver predicate for the
    /// normative-optional legacy constructor behavior.
    fn intl_legacy_receiver(
        &mut self,
        receiver: &Value,
        prototype: ObjectId,
    ) -> Result<bool, RuntimeError> {
        let Some(mut object) = receiver.object_id() else {
            return Ok(false);
        };
        let base = self.stack.len();
        self.stack
            .extend([receiver.clone(), Value::Object(prototype)]);
        let result = (|| {
            loop {
                // Proxy [[GetPrototypeOf]] can execute arbitrary JavaScript,
                // so retain the current link across that operation.
                self.stack[base] = Value::Object(object);
                let Some(parent) = self.object_get_prototype(object)? else {
                    return Ok(false);
                };
                if parent == prototype {
                    return Ok(true);
                }
                object = parent;
            }
        })();
        self.stack.truncate(base);
        result
    }

    fn intl_legacy_fallback_symbol(&mut self) -> JsSymbol {
        self.intl_legacy_constructed_symbol
            .get_or_insert_with(|| JsSymbol::new(Some("IntlLegacyConstructedSymbol".into())))
            .clone()
    }

    fn date_time_format_data(
        &self,
        value: &Value,
    ) -> Result<Rc<intl::DateTimeFormat>, RuntimeError> {
        if let Value::Object(id) = value {
            if let Some(data) = self.heap.date_time_format(*id)? {
                return Ok(data);
            }
        }
        Err(RuntimeError::TypeError(
            "receiver is not an Intl.DateTimeFormat".into(),
        ))
    }

    /// Implements `UnwrapDateTimeFormat` for the only legacy-facing methods
    /// that ECMA-402 permits to unwrap a ChainDateTimeFormat receiver. Other
    /// DateTimeFormat methods deliberately continue to call
    /// `date_time_format_data` and require a directly branded receiver.
    fn unwrap_date_time_format(&mut self, value: &Value) -> Result<ObjectId, RuntimeError> {
        if let Some(id) = value.object_id() {
            if self.heap.date_time_format(id)?.is_some() {
                return Ok(id);
            }
        } else {
            return Err(RuntimeError::TypeError(
                "receiver is not an Intl.DateTimeFormat".into(),
            ));
        }

        let fallback_symbol = self.intl_legacy_constructed_symbol.clone().ok_or_else(|| {
            RuntimeError::TypeError("receiver is not an Intl.DateTimeFormat".into())
        })?;
        // `Get` is intentional. In particular, a Proxy around a chained
        // receiver must observe this symbol lookup before the hidden object
        // is brand-checked.
        let fallback_key = PropertyName::from(fallback_symbol);
        let fallback = self.get_property(value, &fallback_key)?;
        let Some(id) = fallback.object_id() else {
            return Err(RuntimeError::TypeError(
                "receiver is not an Intl.DateTimeFormat".into(),
            ));
        };
        self.heap
            .date_time_format(id)?
            .is_some()
            .then_some(id)
            .ok_or_else(|| RuntimeError::TypeError("receiver is not an Intl.DateTimeFormat".into()))
    }

    pub(super) fn date_time_format_format_getter(
        &mut self,
        receiver: &Value,
    ) -> Result<Value, RuntimeError> {
        let id = self.unwrap_date_time_format(receiver)?;
        if let Some(function) = self.heap.date_time_format_format(id) {
            return Ok(Value::Object(function));
        }
        let constructor = self.string_intrinsics()?.0;
        let prototype = self.heap.prototype(constructor)?.unwrap();
        let target = self.with_roots(|heap| {
            heap.alloc_native_function(NativeFunction::DateTimeFormatFormat, "", prototype)
        })?;
        let function = self.with_roots(|heap| {
            heap.alloc_bound_function(
                crate::heap::BoundFunction {
                    target,
                    // The getter may have received a legacy chained object.
                    // Bind the actual branded formatter selected by
                    // UnwrapDateTimeFormat, not that outer receiver.
                    this: Value::Object(id),
                    args: vec![],
                    constructible: false,
                },
                Some(prototype),
            )
        })?;
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
        self.heap.set_date_time_format_format(id, function);
        Ok(Value::Object(function))
    }

    fn date_time_value(&mut self, value: &Value) -> Result<f64, RuntimeError> {
        if *value == Value::Undefined {
            return Ok(Self::current_time());
        }
        let value = self.coerce_number(value)?;
        if !value.is_finite() {
            return Err(RuntimeError::RangeError("invalid time value".into()));
        }
        Ok(value)
    }

    pub(super) fn date_time_format_format(
        &mut self,
        receiver: &Value,
        value: &Value,
    ) -> Result<Value, RuntimeError> {
        let data = self.date_time_format_data(receiver)?;
        let parts = self.date_time_format_parts(&data, value)?;
        Ok(Value::String(
            parts
                .into_iter()
                .map(|part| part.value)
                .collect::<String>()
                .into(),
        ))
    }

    fn date_time_parts_to_value(
        &mut self,
        parts: Vec<blueice_ecma402::DateTimePart>,
        source: Option<&str>,
    ) -> Result<Value, RuntimeError> {
        let prototype = self.object_prototype;
        let base = self.stack.len();
        let result = (|| {
            for part in parts {
                let object = self.with_roots(|heap| heap.alloc_object(Some(prototype)))?;
                self.stack.push(Value::Object(object));
                self.define_data(
                    object,
                    "type",
                    Value::String(part.kind.into()),
                    true,
                    true,
                    true,
                )?;
                self.define_data(
                    object,
                    "value",
                    Value::String(part.value.into()),
                    true,
                    true,
                    true,
                )?;
                if let Some(source) = source {
                    self.define_data(
                        object,
                        "source",
                        Value::String(source.into()),
                        true,
                        true,
                        true,
                    )?;
                }
            }
            self.array_from(self.stack[base..].to_vec())
        })();
        self.stack.truncate(base);
        result
    }

    fn date_time_range_parts_to_value(
        &mut self,
        parts: Vec<blueice_ecma402::DateTimeRangePart>,
    ) -> Result<Value, RuntimeError> {
        let prototype = self.object_prototype;
        let base = self.stack.len();
        let result = (|| {
            for part in parts {
                let object = self.with_roots(|heap| heap.alloc_object(Some(prototype)))?;
                self.stack.push(Value::Object(object));
                self.define_data(
                    object,
                    "type",
                    Value::String(part.kind.into()),
                    true,
                    true,
                    true,
                )?;
                self.define_data(
                    object,
                    "value",
                    Value::String(part.value.into()),
                    true,
                    true,
                    true,
                )?;
                let source = match part.source {
                    blueice_ecma402::DateTimeRangePartSource::Shared => "shared",
                    blueice_ecma402::DateTimeRangePartSource::StartRange => "startRange",
                    blueice_ecma402::DateTimeRangePartSource::EndRange => "endRange",
                };
                self.define_data(
                    object,
                    "source",
                    Value::String(source.into()),
                    true,
                    true,
                    true,
                )?;
            }
            self.array_from(self.stack[base..].to_vec())
        })();
        self.stack.truncate(base);
        result
    }

    pub(super) fn date_time_format_format_to_parts(
        &mut self,
        receiver: &Value,
        value: &Value,
    ) -> Result<Value, RuntimeError> {
        let data = self.date_time_format_data(receiver)?;
        let parts = self.date_time_format_parts(&data, value)?;
        self.date_time_parts_to_value(parts, None)
    }

    fn date_time_format_parts(
        &mut self,
        data: &blueice_ecma402::DateTimeFormat,
        value: &Value,
    ) -> Result<Vec<blueice_ecma402::DateTimePart>, RuntimeError> {
        let value = self.date_time_format_value(value, true)?;
        let input = self.date_time_format_input(data, value)?;
        data.format_input_to_parts(input)
            .map_err(|error| RuntimeError::RangeError(error.to_string()))
    }

    fn date_time_format_value(
        &mut self,
        value: &Value,
        default_to_now: bool,
    ) -> Result<DateTimeFormatValue, RuntimeError> {
        if let Some(object) = value.object_id() {
            if let Some(temporal) = self.heap.temporal_value(object)? {
                return Ok(DateTimeFormatValue::Temporal(temporal));
            }
        }
        let epoch_milliseconds = if default_to_now {
            self.date_time_value(value)?
        } else {
            self.coerce_number(value)?
        };
        Ok(DateTimeFormatValue::Number(epoch_milliseconds))
    }

    fn date_time_format_input(
        &self,
        data: &blueice_ecma402::DateTimeFormat,
        value: DateTimeFormatValue,
    ) -> Result<blueice_ecma402::DateTimeFormatInput, RuntimeError> {
        match value {
            DateTimeFormatValue::Number(epoch_milliseconds) => Ok(
                blueice_ecma402::DateTimeFormatInput::EpochMilliseconds(epoch_milliseconds),
            ),
            DateTimeFormatValue::Temporal(temporal) => {
                if temporal.kind == TemporalKind::ZonedDateTime {
                    return Err(RuntimeError::TypeError(
                        "Intl.DateTimeFormat does not support Temporal.ZonedDateTime".into(),
                    ));
                }
                if temporal.kind == TemporalKind::Duration {
                    return Err(RuntimeError::TypeError(
                        "Intl.DateTimeFormat does not support Temporal.Duration".into(),
                    ));
                }
                let options = self.temporal_format_options(data, temporal.kind)?;
                self.temporal_date_time_format_input(temporal, options)
            }
        }
    }

    fn temporal_date_time_format_input(
        &self,
        temporal: TemporalValue,
        options: blueice_ecma402::DateTimeFormatOptions,
    ) -> Result<blueice_ecma402::DateTimeFormatInput, RuntimeError> {
        match temporal.kind {
            TemporalKind::Duration => Err(RuntimeError::TypeError(
                "Intl.DateTimeFormat does not support Temporal.Duration".into(),
            )),
            TemporalKind::ZonedDateTime => Err(RuntimeError::TypeError(
                "Intl.DateTimeFormat does not support Temporal.ZonedDateTime".into(),
            )),
            TemporalKind::Instant => {
                let milliseconds = (&temporal.epoch_nanoseconds / 1_000_000u32)
                    .to_f64()
                    .ok_or_else(|| RuntimeError::RangeError("invalid Temporal instant".into()))?;
                Ok(blueice_ecma402::DateTimeFormatInput::TemporalInstant {
                    epoch_milliseconds: milliseconds,
                    options,
                })
            }
            _ => Ok(blueice_ecma402::DateTimeFormatInput::TemporalPlain {
                local_epoch_milliseconds: temporal.plain_epoch_milliseconds(),
                options,
            }),
        }
    }

    fn date_time_range_values(
        &mut self,
        start: &Value,
        end: &Value,
    ) -> Result<(DateTimeFormatValue, DateTimeFormatValue), RuntimeError> {
        // Unlike `format` and `formatToParts`, the range methods require both
        // operands. Check that before ToNumber so a missing endpoint takes
        // precedence over observable conversion of the other argument.
        if *start == Value::Undefined || *end == Value::Undefined {
            return Err(RuntimeError::TypeError(
                "date-time range endpoints must not be undefined".into(),
            ));
        }
        // Convert both arguments before checking their kinds. In particular,
        // an ordinary object's `valueOf` remains observable when the other
        // argument is a Temporal object, as required by ToDateTimeFormattable.
        Ok((
            self.date_time_format_value(start, false)?,
            self.date_time_format_value(end, false)?,
        ))
    }

    fn temporal_format_options(
        &self,
        data: &blueice_ecma402::DateTimeFormat,
        kind: TemporalKind,
    ) -> Result<blueice_ecma402::DateTimeFormatOptions, RuntimeError> {
        if kind == TemporalKind::Duration {
            return Err(RuntimeError::TypeError(
                "Intl.DateTimeFormat does not support Temporal.Duration".into(),
            ));
        }
        let original = data.options();
        let has_date = temporal_has_date_components(original);
        let has_time = temporal_has_time_components(original);
        // `era` is an additive display field. It does not suppress the
        // type-specific default components selected by DateTimeFormat.
        let only_default_components = !(original.date_style.is_some()
            || original.time_style.is_some()
            || has_time
            || original.weekday.is_some()
            || original.year.is_some()
            || original.month.is_some()
            || original.day.is_some());
        let mut options = original.clone();
        if kind == TemporalKind::Instant {
            if only_default_components {
                temporal_default_components(&mut options, kind);
            }
            return Ok(options);
        }
        // Plain Temporal values denote local calendar fields, not instants.
        // UTC carries those fields through ICU4X without applying the
        // formatter's requested IANA transition rules.
        options.time_zone = Some("UTC".into());
        options.time_zone_name = None;
        match kind {
            TemporalKind::PlainDate => clear_temporal_time_components(&mut options),
            TemporalKind::PlainDateTime => {}
            TemporalKind::PlainMonthDay => {
                clear_temporal_time_components(&mut options);
                options.weekday = None;
                options.era = None;
                options.year = None;
                if let Some(style) = original.date_style {
                    apply_temporal_partial_date_style(&mut options, style, false);
                }
            }
            TemporalKind::PlainTime => clear_temporal_date_components(&mut options),
            TemporalKind::PlainYearMonth => {
                clear_temporal_time_components(&mut options);
                options.weekday = None;
                options.day = None;
                if let Some(style) = original.date_style {
                    apply_temporal_partial_date_style(&mut options, style, true);
                }
            }
            TemporalKind::Instant | TemporalKind::ZonedDateTime | TemporalKind::Duration => {
                unreachable!()
            }
        }
        // `timeStyle: long/full` normally supplies a time-zone name. Plain
        // values specifically suppress it, so retain the time fields with an
        // otherwise equivalent non-zone style.
        if matches!(
            options.time_style,
            Some(blueice_ecma402::DateTimeStyle::Full | blueice_ecma402::DateTimeStyle::Long)
        ) {
            options.time_style = Some(blueice_ecma402::DateTimeStyle::Medium);
        }
        if only_default_components {
            temporal_default_components(&mut options, kind);
        }
        let visible = only_default_components
            || options.date_style.is_some()
            || options.time_style.is_some()
            || temporal_has_date_components(&options)
            || temporal_has_time_components(&options);
        if !visible {
            return Err(RuntimeError::TypeError(
                "DateTimeFormat options do not overlap the Temporal value".into(),
            ));
        }
        // If a date/time style was present but it did not apply to this
        // Temporal kind, the pruning above left no components. This is the
        // same no-overlap TypeError, including dateStyle with PlainTime.
        if (kind == TemporalKind::PlainTime
            && original.date_style.is_some()
            && original.time_style.is_none()
            && !has_time)
            || (kind == TemporalKind::PlainDate
                && original.time_style.is_some()
                && original.date_style.is_none()
                && !has_date)
        {
            return Err(RuntimeError::TypeError(
                "DateTimeFormat options do not overlap the Temporal value".into(),
            ));
        }
        Ok(options)
    }

    fn temporal_range_parts(
        &self,
        data: &blueice_ecma402::DateTimeFormat,
        start: TemporalValue,
        end: TemporalValue,
    ) -> Result<Vec<blueice_ecma402::DateTimeRangePart>, RuntimeError> {
        if start.kind != end.kind {
            return Err(RuntimeError::TypeError(
                "date-time range endpoints have different Temporal types".into(),
            ));
        }
        if start.calendar != end.calendar {
            return Err(RuntimeError::RangeError(
                "date-time range endpoints have different calendars".into(),
            ));
        }
        if start.kind == TemporalKind::ZonedDateTime {
            return Err(RuntimeError::TypeError(
                "Intl.DateTimeFormat does not support Temporal.ZonedDateTime".into(),
            ));
        }
        // Both endpoints share a Temporal kind and calendar above, so one
        // resolved option record must govern direct and range formatting.
        let options = self.temporal_format_options(data, start.kind)?;
        let start = self.temporal_date_time_format_input(start, options.clone())?;
        let end = self.temporal_date_time_format_input(end, options)?;
        data.format_range_inputs_to_parts(start, end)
            .map_err(|error| RuntimeError::RangeError(error.to_string()))
    }

    fn date_time_range_parts(
        &self,
        data: &blueice_ecma402::DateTimeFormat,
        start: DateTimeFormatValue,
        end: DateTimeFormatValue,
    ) -> Result<Vec<blueice_ecma402::DateTimeRangePart>, RuntimeError> {
        match (start, end) {
            (DateTimeFormatValue::Number(start), DateTimeFormatValue::Number(end)) => data
                .format_range_inputs_to_parts(
                    blueice_ecma402::DateTimeFormatInput::EpochMilliseconds(start),
                    blueice_ecma402::DateTimeFormatInput::EpochMilliseconds(end),
                )
                .map_err(|error| RuntimeError::RangeError(error.to_string())),
            (DateTimeFormatValue::Temporal(start), DateTimeFormatValue::Temporal(end)) => {
                self.temporal_range_parts(data, start, end)
            }
            _ => Err(RuntimeError::TypeError(
                "date-time range endpoints have different kinds".into(),
            )),
        }
    }

    pub(super) fn date_time_format_format_range(
        &mut self,
        receiver: &Value,
        start: &Value,
        end: &Value,
    ) -> Result<Value, RuntimeError> {
        let data = self.date_time_format_data(receiver)?;
        let (start, end) = self.date_time_range_values(start, end)?;
        self.date_time_range_parts(&data, start, end).map(|parts| {
            Value::String(
                parts
                    .into_iter()
                    .map(|part| part.value)
                    .collect::<String>()
                    .into(),
            )
        })
    }

    pub(super) fn date_time_format_format_range_to_parts(
        &mut self,
        receiver: &Value,
        start: &Value,
        end: &Value,
    ) -> Result<Value, RuntimeError> {
        let data = self.date_time_format_data(receiver)?;
        let (start, end) = self.date_time_range_values(start, end)?;
        let parts = self.date_time_range_parts(&data, start, end)?;
        self.date_time_range_parts_to_value(parts)
    }

    pub(super) fn date_time_format_resolved_options(
        &mut self,
        receiver: &Value,
    ) -> Result<Value, RuntimeError> {
        let id = self.unwrap_date_time_format(receiver)?;
        let data = self
            .heap
            .date_time_format(id)?
            .expect("UnwrapDateTimeFormat returns a branded object");
        let options = data.options();
        let prototype = self.object_prototype;
        let result = self.with_roots(|heap| heap.alloc_object(Some(prototype)))?;
        self.stack.push(Value::Object(result));
        for (name, value) in [
            ("locale", Value::String(data.locale().into())),
            ("calendar", Value::String(data.calendar().into())),
            (
                "numberingSystem",
                Value::String(data.numbering_system().into()),
            ),
            ("timeZone", Value::String(data.time_zone().into())),
        ] {
            self.define_data(result, name, value, true, true, true)?;
        }
        let time_requested = options.time_style.is_some()
            || options.hour.is_some()
            || options.minute.is_some()
            || options.second.is_some()
            || options.fractional_second_digits.is_some();
        // InitializeDateTimeFormat supplies the numeric date triple only
        // when *no* date or time component was requested. Do not turn a
        // requested year/month, month/day, or weekday skeleton into YMD in
        // resolvedOptions merely because it has no time fields.
        let default_date = !time_requested
            && options.date_style.is_none()
            && options.weekday.is_none()
            && options.year.is_none()
            && options.month.is_none()
            && options.day.is_none();
        let default_date_width = default_date.then_some(blueice_ecma402::DateTimeWidth::Numeric);
        if time_requested {
            self.define_data(
                result,
                "hourCycle",
                Value::String(data.hour_cycle().into()),
                true,
                true,
                true,
            )?;
            self.define_data(
                result,
                "hour12",
                Value::Bool(matches!(data.hour_cycle(), "h11" | "h12")),
                true,
                true,
                true,
            )?;
        }
        for (name, width) in [
            ("weekday", options.weekday),
            ("era", options.era),
            ("year", options.year.or(default_date_width)),
            ("month", options.month.or(default_date_width)),
            ("day", options.day.or(default_date_width)),
            ("dayPeriod", options.day_period),
            ("hour", options.hour),
            ("minute", options.minute),
            ("second", options.second),
        ] {
            if let Some(width) = width {
                let width = match width {
                    blueice_ecma402::DateTimeWidth::Numeric => "numeric",
                    blueice_ecma402::DateTimeWidth::TwoDigit => "2-digit",
                    blueice_ecma402::DateTimeWidth::Short => "short",
                    blueice_ecma402::DateTimeWidth::Long => "long",
                    blueice_ecma402::DateTimeWidth::Narrow => "narrow",
                };
                self.define_data(result, name, Value::String(width.into()), true, true, true)?;
            }
        }
        if let Some(digits) = options.fractional_second_digits {
            self.define_data(
                result,
                "fractionalSecondDigits",
                Value::Number(digits.into()),
                true,
                true,
                true,
            )?;
        }
        if let Some(name) = &options.time_zone_name {
            self.define_data(
                result,
                "timeZoneName",
                Value::String(name.clone().into()),
                true,
                true,
                true,
            )?;
        }
        if let Some(style) = options.date_style {
            self.define_data(
                result,
                "dateStyle",
                Value::String(
                    match style {
                        blueice_ecma402::DateTimeStyle::Full => "full",
                        blueice_ecma402::DateTimeStyle::Long => "long",
                        blueice_ecma402::DateTimeStyle::Medium => "medium",
                        blueice_ecma402::DateTimeStyle::Short => "short",
                    }
                    .into(),
                ),
                true,
                true,
                true,
            )?;
        }
        if let Some(style) = options.time_style {
            self.define_data(
                result,
                "timeStyle",
                Value::String(
                    match style {
                        blueice_ecma402::DateTimeStyle::Full => "full",
                        blueice_ecma402::DateTimeStyle::Long => "long",
                        blueice_ecma402::DateTimeStyle::Medium => "medium",
                        blueice_ecma402::DateTimeStyle::Short => "short",
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

    fn duration_style(
        &mut self,
        options: &Value,
    ) -> Result<blueice_ecma402::DurationStyle, RuntimeError> {
        match self
            .string_option(options, "style", &["long", "short", "narrow", "digital"])?
            .as_deref()
        {
            None | Some("short") => Ok(blueice_ecma402::DurationStyle::Short),
            Some("long") => Ok(blueice_ecma402::DurationStyle::Long),
            Some("narrow") => Ok(blueice_ecma402::DurationStyle::Narrow),
            Some("digital") => Ok(blueice_ecma402::DurationStyle::Digital),
            Some(_) => unreachable!("string_option validates DurationFormat style"),
        }
    }

    fn duration_unit_options(
        &mut self,
        options: &Value,
        unit: blueice_ecma402::DurationUnit,
    ) -> Result<blueice_ecma402::DurationUnitOptions, RuntimeError> {
        let (name, allows_two_digit) = match unit {
            blueice_ecma402::DurationUnit::Years => ("years", false),
            blueice_ecma402::DurationUnit::Months => ("months", false),
            blueice_ecma402::DurationUnit::Weeks => ("weeks", false),
            blueice_ecma402::DurationUnit::Days => ("days", false),
            blueice_ecma402::DurationUnit::Hours => ("hours", true),
            blueice_ecma402::DurationUnit::Minutes => ("minutes", true),
            blueice_ecma402::DurationUnit::Seconds => ("seconds", true),
            blueice_ecma402::DurationUnit::Milliseconds => ("milliseconds", false),
            blueice_ecma402::DurationUnit::Microseconds => ("microseconds", false),
            blueice_ecma402::DurationUnit::Nanoseconds => ("nanoseconds", false),
        };
        let allowed = if allows_two_digit {
            &["long", "short", "narrow", "numeric", "2-digit"][..]
        } else if matches!(
            unit,
            blueice_ecma402::DurationUnit::Milliseconds
                | blueice_ecma402::DurationUnit::Microseconds
                | blueice_ecma402::DurationUnit::Nanoseconds
        ) {
            &["long", "short", "narrow", "numeric"][..]
        } else {
            &["long", "short", "narrow"][..]
        };
        let style = self
            .string_option(options, name, allowed)?
            .map(|style| match style.as_str() {
                "long" => blueice_ecma402::DurationUnitStyle::Long,
                "short" => blueice_ecma402::DurationUnitStyle::Short,
                "narrow" => blueice_ecma402::DurationUnitStyle::Narrow,
                "numeric" => blueice_ecma402::DurationUnitStyle::Numeric,
                "2-digit" => blueice_ecma402::DurationUnitStyle::TwoDigit,
                _ => unreachable!("string_option validates DurationFormat unit style"),
            });
        let display = self
            .string_option(options, &format!("{name}Display"), &["auto", "always"])?
            .map(|display| match display.as_str() {
                "auto" => blueice_ecma402::DurationUnitDisplay::Auto,
                "always" => blueice_ecma402::DurationUnitDisplay::Always,
                _ => unreachable!("string_option validates DurationFormat display"),
            });
        Ok(blueice_ecma402::DurationUnitOptions { style, display })
    }

    fn duration_fractional_digits(&mut self, options: &Value) -> Result<Option<u8>, RuntimeError> {
        let value = self.get_property(options, &"fractionalDigits".into())?;
        if value == Value::Undefined {
            return Ok(None);
        }
        let value = self.coerce_number(&value)?;
        if !value.is_finite() || value.floor() != value || !(0.0..=9.0).contains(&value) {
            return Err(RuntimeError::RangeError(
                "invalid fractionalDigits option".into(),
            ));
        }
        Ok(Some(value as u8))
    }

    fn resolve_duration_format(
        &mut self,
        locales: &Value,
        options: &Value,
    ) -> Result<Rc<intl::DurationFormat>, RuntimeError> {
        let locales = self.canonical_locales(locales)?;
        let options = self.intl_constructor_options(options)?;
        let locale_matcher = self.locale_matcher(&options)?;
        let requested_numbering_system = self.string_option(&options, "numberingSystem", &[])?;
        if let Some(numbering_system) = requested_numbering_system.as_deref() {
            if !(3..=8).contains(&numbering_system.len())
                || !numbering_system
                    .bytes()
                    .all(|byte| byte.is_ascii_alphanumeric())
            {
                return Err(RuntimeError::RangeError(
                    "invalid numberingSystem option".into(),
                ));
            }
        }
        let locales = locales
            .into_iter()
            .map(|locale| {
                blueice_ecma402::resolve_numbering_system_locale(
                    &locale,
                    requested_numbering_system.as_deref(),
                )
            })
            .collect::<Vec<_>>();
        let style = self.duration_style(&options)?;
        let mut units = [blueice_ecma402::DurationUnitOptions::default(); 10];
        for unit in blueice_ecma402::DurationUnit::ALL {
            units[unit as usize] = self.duration_unit_options(&options, unit)?;
        }
        let options = blueice_ecma402::DurationFormatOptions {
            locale_matcher,
            style,
            units,
            fractional_digits: self.duration_fractional_digits(&options)?,
        };
        blueice_ecma402::DurationFormat::try_new(&locales, options)
            .map(Rc::new)
            .map_err(|error| RuntimeError::RangeError(error.to_string()))
    }

    pub(super) fn duration_format_supported_locales(
        &mut self,
        args: &[Value],
    ) -> Result<Value, RuntimeError> {
        let locales = self.canonical_locales(native::argument(args, 0))?;
        let options = self.intl_options(native::argument(args, 1))?;
        let locales = blueice_ecma402::supported_duration_format_locales(
            &locales,
            self.locale_matcher(&options)?,
        );
        self.array_from(
            locales
                .iter()
                .map(|locale| Value::String(locale.to_string().into()))
                .collect(),
        )
    }

    fn create_duration_format(
        &mut self,
        args: &[Value],
        construct: bool,
    ) -> Result<Value, RuntimeError> {
        if !construct {
            return Err(RuntimeError::TypeError(
                "Intl.DurationFormat must be called with new".into(),
            ));
        }
        self.intl_global()?;
        let constructor = self.globals["%Intl.DurationFormat%"];
        let default = self
            .heap
            .get(constructor, "prototype")?
            .object_id()
            .expect("Intl.DurationFormat.prototype is an object");
        let prototype = self.constructor_prototype(default)?;
        self.stack.push(Value::Object(prototype));
        let data =
            self.resolve_duration_format(native::argument(args, 0), native::argument(args, 1))?;
        self.with_roots(|heap| heap.alloc_duration_format(data, prototype))
            .map(Value::Object)
    }

    fn duration_format_data(
        &self,
        value: &Value,
    ) -> Result<Rc<intl::DurationFormat>, RuntimeError> {
        if let Value::Object(id) = value {
            if let Some(data) = self.heap.duration_format(*id)? {
                return Ok(data);
            }
        }
        Err(RuntimeError::TypeError(
            "receiver is not an Intl.DurationFormat".into(),
        ))
    }

    fn duration_record(
        &mut self,
        value: &Value,
    ) -> Result<blueice_ecma402::DurationRecord, RuntimeError> {
        if let Some(object) = value.object_id() {
            if let Some(temporal) = self.heap.temporal_value(object)? {
                if temporal.kind == TemporalKind::Duration {
                    return Ok(*temporal
                        .duration
                        .as_deref()
                        .expect("Temporal.Duration values retain a duration record"));
                }
            }
        }
        if matches!(value, Value::String(_)) {
            let source = self.coerce_string(value)?.to_utf8().map_err(|_| {
                RuntimeError::RangeError("invalid Intl.DurationFormat duration string".into())
            })?;
            return self
                .temporal_value_from_string(TemporalKind::Duration, &source)
                .map(|temporal| {
                    *temporal
                        .duration
                        .as_deref()
                        .expect("Temporal.Duration values retain a duration record")
                });
        }
        if !matches!(value, Value::Object(_)) {
            return Err(RuntimeError::TypeError(
                "Intl.DurationFormat duration must be an object".into(),
            ));
        }
        let mut values = [0.0; 10];
        let mut has_duration_field = false;
        for (index, name) in [
            "years",
            "months",
            "weeks",
            "days",
            "hours",
            "minutes",
            "seconds",
            "milliseconds",
            "microseconds",
            "nanoseconds",
        ]
        .into_iter()
        .enumerate()
        {
            let field = self.get_property(value, &name.into())?;
            if field != Value::Undefined {
                has_duration_field = true;
                values[index] = self.coerce_number(&field)?;
            }
        }
        if !has_duration_field {
            return Err(RuntimeError::TypeError(
                "Intl.DurationFormat duration has no fields".into(),
            ));
        }
        blueice_ecma402::DurationRecord::try_from_f64(
            values[0], values[1], values[2], values[3], values[4], values[5], values[6], values[7],
            values[8], values[9],
        )
        .map_err(|error| RuntimeError::RangeError(error.to_string()))
    }

    pub(super) fn duration_format_format(
        &mut self,
        receiver: &Value,
        duration: &Value,
    ) -> Result<Value, RuntimeError> {
        let data = self.duration_format_data(receiver)?;
        let duration = self.duration_record(duration)?;
        data.format(duration)
            .map(|value| Value::String(value.into()))
            .map_err(|error| RuntimeError::RangeError(error.to_string()))
    }

    pub(super) fn duration_format_format_to_parts(
        &mut self,
        receiver: &Value,
        duration: &Value,
    ) -> Result<Value, RuntimeError> {
        let data = self.duration_format_data(receiver)?;
        let duration = self.duration_record(duration)?;
        let parts = data
            .format_to_parts(duration)
            .map_err(|error| RuntimeError::RangeError(error.to_string()))?;
        let prototype = self.object_prototype;
        let base = self.stack.len();
        let result = (|| {
            for source in parts {
                let part = self.with_roots(|heap| heap.alloc_object(Some(prototype)))?;
                self.stack.push(Value::Object(part));
                let kind = match source.kind {
                    blueice_ecma402::DurationPartKind::Integer => "integer",
                    blueice_ecma402::DurationPartKind::Decimal => "decimal",
                    blueice_ecma402::DurationPartKind::Fraction => "fraction",
                    blueice_ecma402::DurationPartKind::MinusSign => "minusSign",
                    blueice_ecma402::DurationPartKind::Unit => "unit",
                    blueice_ecma402::DurationPartKind::Literal => "literal",
                };
                self.define_data(part, "type", Value::String(kind.into()), true, true, true)?;
                self.define_data(
                    part,
                    "value",
                    Value::String(source.value.into()),
                    true,
                    true,
                    true,
                )?;
                if let Some(unit) = source.unit {
                    let unit = match unit {
                        blueice_ecma402::DurationUnit::Years => "year",
                        blueice_ecma402::DurationUnit::Months => "month",
                        blueice_ecma402::DurationUnit::Weeks => "week",
                        blueice_ecma402::DurationUnit::Days => "day",
                        blueice_ecma402::DurationUnit::Hours => "hour",
                        blueice_ecma402::DurationUnit::Minutes => "minute",
                        blueice_ecma402::DurationUnit::Seconds => "second",
                        blueice_ecma402::DurationUnit::Milliseconds => "millisecond",
                        blueice_ecma402::DurationUnit::Microseconds => "microsecond",
                        blueice_ecma402::DurationUnit::Nanoseconds => "nanosecond",
                    };
                    self.define_data(part, "unit", Value::String(unit.into()), true, true, true)?;
                }
            }
            self.array_from(self.stack[base..].to_vec())
        })();
        self.stack.truncate(base);
        result
    }

    pub(super) fn duration_format_resolved_options(
        &mut self,
        receiver: &Value,
    ) -> Result<Value, RuntimeError> {
        let data = self.duration_format_data(receiver)?;
        let resolved = data.resolved_options();
        let prototype = self.object_prototype;
        let object = self.with_roots(|heap| heap.alloc_object(Some(prototype)))?;
        self.stack.push(Value::Object(object));
        self.define_data(
            object,
            "locale",
            Value::String(resolved.locale.clone().into()),
            true,
            true,
            true,
        )?;
        self.define_data(
            object,
            "numberingSystem",
            Value::String(resolved.numbering_system.clone().into()),
            true,
            true,
            true,
        )?;
        let style = match resolved.style {
            blueice_ecma402::DurationStyle::Long => "long",
            blueice_ecma402::DurationStyle::Short => "short",
            blueice_ecma402::DurationStyle::Narrow => "narrow",
            blueice_ecma402::DurationStyle::Digital => "digital",
        };
        self.define_data(
            object,
            "style",
            Value::String(style.into()),
            true,
            true,
            true,
        )?;
        for unit in blueice_ecma402::DurationUnit::ALL {
            let (name, display_name) = match unit {
                blueice_ecma402::DurationUnit::Years => ("years", "yearsDisplay"),
                blueice_ecma402::DurationUnit::Months => ("months", "monthsDisplay"),
                blueice_ecma402::DurationUnit::Weeks => ("weeks", "weeksDisplay"),
                blueice_ecma402::DurationUnit::Days => ("days", "daysDisplay"),
                blueice_ecma402::DurationUnit::Hours => ("hours", "hoursDisplay"),
                blueice_ecma402::DurationUnit::Minutes => ("minutes", "minutesDisplay"),
                blueice_ecma402::DurationUnit::Seconds => ("seconds", "secondsDisplay"),
                blueice_ecma402::DurationUnit::Milliseconds => {
                    ("milliseconds", "millisecondsDisplay")
                }
                blueice_ecma402::DurationUnit::Microseconds => {
                    ("microseconds", "microsecondsDisplay")
                }
                blueice_ecma402::DurationUnit::Nanoseconds => ("nanoseconds", "nanosecondsDisplay"),
            };
            let style = match resolved.unit_style(unit) {
                blueice_ecma402::DurationUnitStyle::Long => "long",
                blueice_ecma402::DurationUnitStyle::Short => "short",
                blueice_ecma402::DurationUnitStyle::Narrow => "narrow",
                blueice_ecma402::DurationUnitStyle::Numeric => "numeric",
                blueice_ecma402::DurationUnitStyle::TwoDigit => "2-digit",
            };
            let display = match resolved.unit_display(unit) {
                blueice_ecma402::DurationUnitDisplay::Auto => "auto",
                blueice_ecma402::DurationUnitDisplay::Always => "always",
            };
            self.define_data(object, name, Value::String(style.into()), true, true, true)?;
            self.define_data(
                object,
                display_name,
                Value::String(display.into()),
                true,
                true,
                true,
            )?;
        }
        if let Some(fractional_digits) = resolved.fractional_digits {
            self.define_data(
                object,
                "fractionalDigits",
                Value::Number(f64::from(fractional_digits)),
                true,
                true,
                true,
            )?;
        }
        Ok(Value::Object(object))
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
        let category_for = |value: f64| {
            if data.notation == "compact" {
                data.data
                    .select_compact_f64(value, data.compact_display.as_deref() == Some("long"))
            } else {
                data.data.select_f64(value)
            }
        };
        let category = if start == end {
            category_for(start)
        } else {
            category_for(start)
                .and_then(|start| category_for(end).map(|end| data.data.select_range(start, end)))
        };
        category
            .map(Self::plural_category_name)
            .map(|category| Value::String(category.into()))
            .map_err(|error| RuntimeError::RangeError(error.to_string()))
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

    /// Implements `UnwrapNumberFormat` for the legacy-facing `format` and
    /// `resolvedOptions` methods. `formatToParts` deliberately requires a
    /// directly branded NumberFormat receiver.
    fn unwrap_number_format(&mut self, value: &Value) -> Result<ObjectId, RuntimeError> {
        if let Some(id) = value.object_id() {
            if self.heap.number_format(id)?.is_some() {
                return Ok(id);
            }
        } else {
            return Err(RuntimeError::TypeError(
                "receiver is not an Intl.NumberFormat".into(),
            ));
        }

        let fallback_symbol = self.intl_legacy_constructed_symbol.clone().ok_or_else(|| {
            RuntimeError::TypeError("receiver is not an Intl.NumberFormat".into())
        })?;
        // `Get` is intentional: a Proxy around a chained receiver must
        // observe the fallback-symbol lookup before its hidden formatter is
        // brand-checked.
        let fallback = self.get_property(value, &PropertyName::from(fallback_symbol))?;
        let Some(id) = fallback.object_id() else {
            return Err(RuntimeError::TypeError(
                "receiver is not an Intl.NumberFormat".into(),
            ));
        };
        self.heap
            .number_format(id)?
            .is_some()
            .then_some(id)
            .ok_or_else(|| RuntimeError::TypeError("receiver is not an Intl.NumberFormat".into()))
    }

    pub(super) fn number_format_format_getter(
        &mut self,
        receiver: &Value,
    ) -> Result<Value, RuntimeError> {
        let id = self.unwrap_number_format(receiver)?;
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
            // The getter may have received a legacy chained object. Bind the
            // hidden branded NumberFormat selected by UnwrapNumberFormat.
            this: Value::Object(id),
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

    pub(super) fn number_format_format_to_parts(
        &mut self,
        receiver: &Value,
        value: &Value,
    ) -> Result<Value, RuntimeError> {
        let data = self.number_format_data(receiver)?;
        let parts = self.number_format_parts(&data, value)?;
        self.number_format_parts_to_value(Ok(parts))
    }

    pub(super) fn number_format_format(
        &mut self,
        receiver: &Value,
        value: &Value,
    ) -> Result<Value, RuntimeError> {
        let data = self.number_format_data(receiver)?;
        let parts = self.number_format_parts(&data, value)?;
        Ok(Value::String(
            parts
                .into_iter()
                .map(|part| part.value)
                .collect::<String>()
                .into(),
        ))
    }

    fn number_format_parts(
        &mut self,
        data: &blueice_ecma402::NumberFormat,
        value: &Value,
    ) -> Result<Vec<blueice_ecma402::NumberFormatPart>, RuntimeError> {
        data.format_input_to_parts(self.number_format_input(value)?)
            .map_err(|error| RuntimeError::RangeError(error.to_string()))
    }

    fn number_format_input(
        &mut self,
        value: &Value,
    ) -> Result<blueice_ecma402::NumberFormatInput, RuntimeError> {
        let value = self.coerce_primitive(value, "number")?;
        let value = match value {
            Value::BigInt(value) => NumberFormatValue::Decimal(value.to_string()),
            Value::String(value) => {
                let number = crate::primitive::number(&Value::String(value.clone()))?;
                match value.to_utf8() {
                    Ok(value) => exact_decimal_intl_mathematical_value(&value)
                        .unwrap_or(NumberFormatValue::Number(number)),
                    _ => NumberFormatValue::Number(number),
                }
            }
            value => NumberFormatValue::Number(crate::primitive::number(&value)?),
        };
        Ok(match value {
            NumberFormatValue::Decimal(value) => blueice_ecma402::NumberFormatInput::Decimal(value),
            NumberFormatValue::ScientificDecimal {
                significand,
                exponent,
            } => blueice_ecma402::NumberFormatInput::ScientificDecimal {
                significand,
                exponent,
            },
            NumberFormatValue::Number(value) => blueice_ecma402::NumberFormatInput::Number(value),
        })
    }

    fn number_format_range_values(
        &mut self,
        start: &Value,
        end: &Value,
    ) -> Result<
        (
            blueice_ecma402::NumberFormatInput,
            blueice_ecma402::NumberFormatInput,
        ),
        RuntimeError,
    > {
        // The range methods require both values. Check first so a missing end
        // takes precedence over an observable conversion of the start value.
        if *start == Value::Undefined || *end == Value::Undefined {
            return Err(RuntimeError::TypeError(
                "number range endpoints must not be undefined".into(),
            ));
        }
        // Convert both values before the host checks for NaN, preserving the
        // specified observable order of ToIntlMathematicalValue.
        Ok((
            self.number_format_input(start)?,
            self.number_format_input(end)?,
        ))
    }

    pub(super) fn number_format_format_range(
        &mut self,
        receiver: &Value,
        start: &Value,
        end: &Value,
    ) -> Result<Value, RuntimeError> {
        let data = self.number_format_data(receiver)?;
        let (start, end) = self.number_format_range_values(start, end)?;
        data.format_range_inputs(start, end)
            .map(|formatted| Value::String(formatted.into()))
            .map_err(|error| RuntimeError::RangeError(error.to_string()))
    }

    pub(super) fn number_format_format_range_to_parts(
        &mut self,
        receiver: &Value,
        start: &Value,
        end: &Value,
    ) -> Result<Value, RuntimeError> {
        let data = self.number_format_data(receiver)?;
        let (start, end) = self.number_format_range_values(start, end)?;
        let parts = data
            .format_range_inputs_to_parts(start, end)
            .map_err(|error| RuntimeError::RangeError(error.to_string()))?;
        self.number_format_range_parts_to_value(parts)
    }

    fn number_format_parts_to_value(
        &mut self,
        parts: Result<Vec<blueice_ecma402::NumberFormatPart>, blueice_ecma402::NumberFormatError>,
    ) -> Result<Value, RuntimeError> {
        let parts = parts.map_err(|error| RuntimeError::RangeError(error.to_string()))?;
        let prototype = self.object_prototype;
        let base = self.stack.len();
        let result = (|| {
            for source in parts {
                let part = self.with_roots(|heap| heap.alloc_object(Some(prototype)))?;
                self.stack.push(Value::Object(part));
                let kind = Self::number_format_part_kind_name(source.kind);
                self.define_data(part, "type", Value::String(kind.into()), true, true, true)?;
                self.define_data(
                    part,
                    "value",
                    Value::String(source.value.into()),
                    true,
                    true,
                    true,
                )?;
            }
            self.array_from(self.stack[base..].to_vec())
        })();
        self.stack.truncate(base);
        result
    }

    fn number_format_range_parts_to_value(
        &mut self,
        parts: Vec<blueice_ecma402::NumberRangePart>,
    ) -> Result<Value, RuntimeError> {
        let prototype = self.object_prototype;
        let base = self.stack.len();
        let result = (|| {
            for part in parts {
                let object = self.with_roots(|heap| heap.alloc_object(Some(prototype)))?;
                self.stack.push(Value::Object(object));
                self.define_data(
                    object,
                    "type",
                    Value::String(Self::number_format_part_kind_name(part.kind).into()),
                    true,
                    true,
                    true,
                )?;
                self.define_data(
                    object,
                    "value",
                    Value::String(part.value.into()),
                    true,
                    true,
                    true,
                )?;
                let source = match part.source {
                    blueice_ecma402::NumberRangePartSource::Shared => "shared",
                    blueice_ecma402::NumberRangePartSource::StartRange => "startRange",
                    blueice_ecma402::NumberRangePartSource::EndRange => "endRange",
                };
                self.define_data(
                    object,
                    "source",
                    Value::String(source.into()),
                    true,
                    true,
                    true,
                )?;
            }
            self.array_from(self.stack[base..].to_vec())
        })();
        self.stack.truncate(base);
        result
    }

    fn number_format_part_kind_name(kind: blueice_ecma402::NumberFormatPartKind) -> &'static str {
        match kind {
            blueice_ecma402::NumberFormatPartKind::MinusSign => "minusSign",
            blueice_ecma402::NumberFormatPartKind::PlusSign => "plusSign",
            blueice_ecma402::NumberFormatPartKind::ApproximatelySign => "approximatelySign",
            blueice_ecma402::NumberFormatPartKind::ExponentSeparator => "exponentSeparator",
            blueice_ecma402::NumberFormatPartKind::ExponentMinusSign => "exponentMinusSign",
            blueice_ecma402::NumberFormatPartKind::ExponentInteger => "exponentInteger",
            blueice_ecma402::NumberFormatPartKind::Integer => "integer",
            blueice_ecma402::NumberFormatPartKind::Group => "group",
            blueice_ecma402::NumberFormatPartKind::Decimal => "decimal",
            blueice_ecma402::NumberFormatPartKind::Fraction => "fraction",
            blueice_ecma402::NumberFormatPartKind::Literal => "literal",
            blueice_ecma402::NumberFormatPartKind::Unit => "unit",
            blueice_ecma402::NumberFormatPartKind::Currency => "currency",
            blueice_ecma402::NumberFormatPartKind::PercentSign => "percentSign",
            blueice_ecma402::NumberFormatPartKind::Compact => "compact",
            blueice_ecma402::NumberFormatPartKind::Nan => "nan",
            blueice_ecma402::NumberFormatPartKind::Infinity => "infinity",
        }
    }

    pub(super) fn number_format_resolved_options(
        &mut self,
        receiver: &Value,
    ) -> Result<Value, RuntimeError> {
        let id = self.unwrap_number_format(receiver)?;
        let data = self
            .heap
            .number_format(id)?
            .expect("UnwrapNumberFormat returns a branded object");
        let resolved = data.resolved_options();
        let prototype = self.object_prototype;
        let result = self.with_roots(|heap| heap.alloc_object(Some(prototype)))?;
        self.stack.push(Value::Object(result));
        let mut properties = vec![
            ("locale", Value::String(resolved.locale.as_str().into())),
            (
                "numberingSystem",
                Value::String(resolved.numbering_system.as_str().into()),
            ),
            (
                "style",
                Value::String(
                    match resolved.style {
                        blueice_ecma402::NumberFormatStyle::Decimal => "decimal",
                        blueice_ecma402::NumberFormatStyle::Percent => "percent",
                        blueice_ecma402::NumberFormatStyle::Currency => "currency",
                        blueice_ecma402::NumberFormatStyle::Unit => "unit",
                    }
                    .into(),
                ),
            ),
        ];
        if let Some(currency) = data.currency() {
            properties.push(("currency", Value::String(currency.code.clone().into())));
            properties.push((
                "currencyDisplay",
                Value::String(
                    match currency.display {
                        blueice_ecma402::NumberCurrencyDisplay::Symbol => "symbol",
                        blueice_ecma402::NumberCurrencyDisplay::Code => "code",
                        blueice_ecma402::NumberCurrencyDisplay::Name => "name",
                        blueice_ecma402::NumberCurrencyDisplay::NarrowSymbol => "narrowSymbol",
                    }
                    .into(),
                ),
            ));
            properties.push((
                "currencySign",
                Value::String(
                    match currency.sign {
                        blueice_ecma402::NumberCurrencySign::Standard => "standard",
                        blueice_ecma402::NumberCurrencySign::Accounting => "accounting",
                    }
                    .into(),
                ),
            ));
        }
        if let Some(unit) = resolved.unit {
            properties.push(("unit", Value::String(unit.identifier().into())));
            properties.push((
                "unitDisplay",
                Value::String(
                    match resolved.unit_display {
                        blueice_ecma402::NumberUnitDisplay::Short => "short",
                        blueice_ecma402::NumberUnitDisplay::Narrow => "narrow",
                        blueice_ecma402::NumberUnitDisplay::Long => "long",
                    }
                    .into(),
                ),
            ));
        }
        properties.push((
            "minimumIntegerDigits",
            Value::Number(resolved.minimum_integer_digits.into()),
        ));
        let significant_digits = data.significant_digits();
        let mixed_precision = significant_digits.is_some()
            && data.rounding_priority() != blueice_ecma402::NumberRoundingPriority::Auto;
        if significant_digits.is_none() || mixed_precision {
            properties.extend([
                (
                    "minimumFractionDigits",
                    Value::Number(resolved.minimum_fraction_digits.into()),
                ),
                (
                    "maximumFractionDigits",
                    Value::Number(resolved.maximum_fraction_digits.into()),
                ),
            ]);
        }
        if let Some((minimum, maximum)) = significant_digits {
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
        properties.push((
            "useGrouping",
            match resolved.use_grouping {
                blueice_ecma402::NumberGrouping::Auto => Value::String("auto".into()),
                blueice_ecma402::NumberGrouping::Never => Value::Bool(false),
                blueice_ecma402::NumberGrouping::Always => Value::String("always".into()),
                blueice_ecma402::NumberGrouping::Min2 => Value::String("min2".into()),
            },
        ));
        properties.push((
            "notation",
            Value::String(
                match resolved.notation {
                    blueice_ecma402::NumberNotation::Standard => "standard",
                    blueice_ecma402::NumberNotation::Scientific => "scientific",
                    blueice_ecma402::NumberNotation::Engineering => "engineering",
                    blueice_ecma402::NumberNotation::Compact => "compact",
                }
                .into(),
            ),
        ));
        if resolved.notation == blueice_ecma402::NumberNotation::Compact {
            properties.push((
                "compactDisplay",
                Value::String(
                    match data.compact_display() {
                        blueice_ecma402::NumberCompactDisplay::Short => "short",
                        blueice_ecma402::NumberCompactDisplay::Long => "long",
                    }
                    .into(),
                ),
            ));
        }
        properties.push((
            "signDisplay",
            Value::String(
                match resolved.sign_display {
                    blueice_ecma402::NumberSignDisplay::Auto => "auto",
                    blueice_ecma402::NumberSignDisplay::Never => "never",
                    blueice_ecma402::NumberSignDisplay::Always => "always",
                    blueice_ecma402::NumberSignDisplay::ExceptZero => "exceptZero",
                    blueice_ecma402::NumberSignDisplay::Negative => "negative",
                }
                .into(),
            ),
        ));
        properties.extend([
            (
                "roundingIncrement",
                Value::Number(f64::from(data.rounding_increment())),
            ),
            (
                "roundingMode",
                Value::String(
                    match resolved.rounding_mode {
                        blueice_ecma402::NumberRoundingMode::HalfExpand => "halfExpand",
                        blueice_ecma402::NumberRoundingMode::Floor => "floor",
                        blueice_ecma402::NumberRoundingMode::Ceil => "ceil",
                        blueice_ecma402::NumberRoundingMode::Expand => "expand",
                        blueice_ecma402::NumberRoundingMode::Trunc => "trunc",
                        blueice_ecma402::NumberRoundingMode::HalfCeil => "halfCeil",
                        blueice_ecma402::NumberRoundingMode::HalfFloor => "halfFloor",
                        blueice_ecma402::NumberRoundingMode::HalfTrunc => "halfTrunc",
                        blueice_ecma402::NumberRoundingMode::HalfEven => "halfEven",
                    }
                    .into(),
                ),
            ),
            (
                "roundingPriority",
                Value::String(
                    match data.rounding_priority() {
                        blueice_ecma402::NumberRoundingPriority::Auto => "auto",
                        blueice_ecma402::NumberRoundingPriority::MorePrecision => "morePrecision",
                        blueice_ecma402::NumberRoundingPriority::LessPrecision => "lessPrecision",
                    }
                    .into(),
                ),
            ),
            (
                "trailingZeroDisplay",
                Value::String(
                    match data.trailing_zero_display() {
                        blueice_ecma402::NumberTrailingZeroDisplay::Auto => "auto",
                        blueice_ecma402::NumberTrailingZeroDisplay::StripIfInteger => {
                            "stripIfInteger"
                        }
                    }
                    .into(),
                ),
            ),
        ]);
        for (key, value) in properties {
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
