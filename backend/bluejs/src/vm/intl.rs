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
pub(super) enum DateTimeFormatValue {
    Number(f64),
    Temporal(TemporalValue),
}

/// The VM-side `ToIntlMathematicalValue` result. This is intentionally kept
/// separate from `ToNumber`: NumberFormat must preserve exact decimal strings
/// and BigInts through `format`, `formatToParts`, and both range operations.
pub(super) enum NumberFormatValue {
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
}

mod collator_locale;
mod date_time;
mod list_duration;
mod number_options;
mod number_runtime;
mod plural_segmenter;
mod shared;
