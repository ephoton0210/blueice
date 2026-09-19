// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

//! Direct coverage for ECMA-402 Edition 13 Duration Record invariants.

use blueice_ecma402::{
    canonicalize, resolve_duration_format_options, supported_duration_format_locales,
    supported_values_of, DurationFormat, DurationFormatError, DurationFormatOptions,
    DurationFormatOptionsError, DurationPart, DurationPartKind, DurationRecord,
    DurationRecordError, DurationStyle, DurationUnit, DurationUnitDisplay, DurationUnitOptions,
    DurationUnitStyle, LocaleMatcher, NumberFormatUnit, SupportedValuesError,
};

#[test]
fn supported_values_advertises_only_data_backed_service_values() {
    let numbering_systems = supported_values_of("numberingSystem").unwrap();
    assert!(numbering_systems.windows(2).all(|pair| pair[0] < pair[1]));
    assert!(numbering_systems.contains(&"latn".into()));
    assert!(numbering_systems.contains(&"adlm".into()));
    assert!(numbering_systems.contains(&"gara".into()));
    assert!(supported_values_of("calendar")
        .unwrap()
        .contains(&"gregory".into()));
    assert!(supported_values_of("collation")
        .unwrap()
        .contains(&"phonebk".into()));
    let currencies = supported_values_of("currency").unwrap();
    assert_eq!(currencies.len(), 307);
    assert!(currencies.windows(2).all(|pair| pair[0] < pair[1]));
    assert!(currencies.contains(&"AFA".into()));
    assert!(currencies.contains(&"XCG".into()));
    assert!(currencies.contains(&"XXX".into()));
    assert_eq!(
        supported_values_of("unit").unwrap(),
        NumberFormatUnit::ALL
            .iter()
            .map(|unit| unit.as_str().to_owned())
            .collect::<Vec<_>>()
    );
    let time_zones = supported_values_of("timeZone").unwrap();
    assert!(time_zones.windows(2).all(|pair| pair[0] < pair[1]));
    assert!(time_zones.contains(&"UTC".into()));
    assert!(time_zones.contains(&"Etc/GMT-14".into()));
    assert!(!time_zones.contains(&"Etc/UTC".into()));
    let error = supported_values_of("not-a-key").unwrap_err();
    assert_eq!(error, SupportedValuesError::InvalidKey);
    assert_eq!(error.to_string(), "invalid Intl.supportedValuesOf key");
}

#[allow(clippy::too_many_arguments)]
fn duration(
    years: i128,
    months: i128,
    weeks: i128,
    days: i128,
    hours: i128,
    minutes: i128,
    seconds: i128,
    milliseconds: i128,
    microseconds: i128,
    nanoseconds: i128,
) -> Result<DurationRecord, DurationRecordError> {
    DurationRecord::try_new(
        years,
        months,
        weeks,
        days,
        hours,
        minutes,
        seconds,
        milliseconds,
        microseconds,
        nanoseconds,
    )
}

fn formatter(options: DurationFormatOptions) -> DurationFormat {
    DurationFormat::try_new(&[canonicalize("en").unwrap()], options).unwrap()
}

fn part(kind: DurationPartKind, value: &str, unit: Option<DurationUnit>) -> DurationPart {
    DurationPart {
        kind,
        value: value.into(),
        unit,
    }
}

#[test]
fn retains_non_normalized_duration_fields_and_computes_duration_sign() {
    let record = duration(1, 2, 3, 4, 5, 6, 90, 7, 8, 9).unwrap();
    assert_eq!(record.seconds, 90);
    assert_eq!(record.sign(), 1);

    let negative = duration(-1, -2, 0, -4, -5, -6, -90, -7, -8, -9).unwrap();
    assert_eq!(negative.sign(), -1);
    assert_eq!(DurationRecord::default().sign(), 0);
}

#[test]
fn rejects_mixed_signs_and_calendar_units_at_the_edition_13_boundary() {
    assert_eq!(
        duration(0, 0, 0, 0, -1, 10, 0, 0, 0, 0),
        Err(DurationRecordError::MixedSign)
    );

    for build in [
        duration((1_i128 << 32) - 1, 0, 0, 0, 0, 0, 0, 0, 0, 0),
        duration(0, -((1_i128 << 32) - 1), 0, 0, 0, 0, 0, 0, 0, 0),
        duration(0, 0, (1_i128 << 32) - 1, 0, 0, 0, 0, 0, 0, 0),
    ] {
        assert!(build.is_ok());
    }
    for build in [
        duration(1_i128 << 32, 0, 0, 0, 0, 0, 0, 0, 0, 0),
        duration(0, 0, -(1_i128 << 32), 0, 0, 0, 0, 0, 0, 0),
    ] {
        assert_eq!(build, Err(DurationRecordError::OutOfRange));
    }
}

#[test]
fn uses_exact_normalized_seconds_limits_without_float_rounding() {
    let limit = 1_i128 << 53;
    assert!(duration(0, 0, 0, 0, 0, 0, limit - 1, 999, 999, 999).is_ok());
    assert_eq!(
        duration(0, 0, 0, 0, 0, 0, limit, 0, 0, 0),
        Err(DurationRecordError::OutOfRange)
    );
    assert_eq!(
        duration(0, 0, 0, 0, 0, 0, -(limit - 1), -1_000, 0, 0),
        Err(DurationRecordError::OutOfRange)
    );

    let maximum_day_count = (limit - 1) / 86_400;
    assert!(duration(0, 0, 0, maximum_day_count, 0, 0, 0, 0, 0, 0).is_ok());
    assert_eq!(
        duration(0, 0, 0, maximum_day_count + 1, 0, 0, 0, 0, 0, 0),
        Err(DurationRecordError::OutOfRange)
    );
}

#[test]
fn converts_finite_integral_ecmascript_numbers_before_validating() {
    let record =
        DurationRecord::try_from_f64(0.0, -0.0, 0.0, 1.0, 2.0, 3.0, 4.0, 5.0, 6.0, 7.0).unwrap();
    assert_eq!(record.sign(), 1);
    assert_eq!(record.nanoseconds, 7);

    for (value, expected) in [
        (f64::NAN, DurationRecordError::NonFinite),
        (f64::INFINITY, DurationRecordError::NonFinite),
        (1.5, DurationRecordError::NonIntegral),
        (2_f64.powi(100), DurationRecordError::OutOfRange),
    ] {
        assert_eq!(
            DurationRecord::try_from_f64(value, 0.0, 0.0, 0.0, 0.0, 0.0, 0.0, 0.0, 0.0, 0.0),
            Err(expected)
        );
    }
}

#[test]
fn exposes_each_duration_record_error_text() {
    for (error, expected) in [
        (
            DurationRecordError::NonFinite,
            "duration fields must be finite",
        ),
        (
            DurationRecordError::NonIntegral,
            "duration fields must be integral",
        ),
        (
            DurationRecordError::MixedSign,
            "duration fields must have a common sign",
        ),
        (
            DurationRecordError::OutOfRange,
            "duration is outside the ECMA-402 range",
        ),
    ] {
        assert_eq!(error.to_string(), expected);
    }
}

#[test]
fn resolves_table_order_defaults_and_digital_clock_defaults() {
    let defaults = resolve_duration_format_options(DurationFormatOptions::default()).unwrap();
    assert_eq!(defaults.style, DurationStyle::Short);
    for unit in DurationUnit::ALL {
        assert_eq!(defaults.unit_style(unit), DurationUnitStyle::Short);
        assert_eq!(defaults.unit_display(unit), DurationUnitDisplay::Auto);
    }

    let narrow = resolve_duration_format_options(DurationFormatOptions {
        style: DurationStyle::Narrow,
        ..Default::default()
    })
    .unwrap();
    for unit in DurationUnit::ALL {
        assert_eq!(narrow.unit_style(unit), DurationUnitStyle::Narrow);
    }

    let digital = resolve_duration_format_options(DurationFormatOptions {
        style: DurationStyle::Digital,
        ..Default::default()
    })
    .unwrap();
    for unit in [
        DurationUnit::Years,
        DurationUnit::Months,
        DurationUnit::Weeks,
        DurationUnit::Days,
    ] {
        assert_eq!(digital.unit_style(unit), DurationUnitStyle::Short);
    }
    assert_eq!(
        digital.unit_style(DurationUnit::Hours),
        DurationUnitStyle::Numeric
    );
    assert_eq!(
        digital.unit_style(DurationUnit::Minutes),
        DurationUnitStyle::TwoDigit
    );
    assert_eq!(
        digital.unit_style(DurationUnit::Seconds),
        DurationUnitStyle::TwoDigit
    );
    for unit in [
        DurationUnit::Milliseconds,
        DurationUnit::Microseconds,
        DurationUnit::Nanoseconds,
    ] {
        assert_eq!(digital.unit_style(unit), DurationUnitStyle::Numeric);
    }
}

#[test]
fn resolves_explicit_styles_displays_and_fractional_digits() {
    let mut options = DurationFormatOptions {
        style: DurationStyle::Long,
        fractional_digits: Some(9),
        ..Default::default()
    };
    options.units[DurationUnit::Years as usize] = DurationUnitOptions {
        style: Some(DurationUnitStyle::Narrow),
        display: Some(DurationUnitDisplay::Always),
    };
    options.units[DurationUnit::Hours as usize].style = Some(DurationUnitStyle::Numeric);
    options.units[DurationUnit::Minutes as usize].style = Some(DurationUnitStyle::TwoDigit);
    options.units[DurationUnit::Seconds as usize].style = Some(DurationUnitStyle::Numeric);
    options.units[DurationUnit::Milliseconds as usize].style = Some(DurationUnitStyle::Numeric);
    options.units[DurationUnit::Microseconds as usize].style = Some(DurationUnitStyle::Numeric);
    options.units[DurationUnit::Nanoseconds as usize].style = Some(DurationUnitStyle::Numeric);
    let resolved = resolve_duration_format_options(options).unwrap();
    assert_eq!(
        resolved.unit_style(DurationUnit::Years),
        DurationUnitStyle::Narrow
    );
    assert_eq!(
        resolved.unit_display(DurationUnit::Years),
        DurationUnitDisplay::Always
    );
    assert_eq!(resolved.fractional_digits, Some(9));
}

#[test]
fn resolves_numeric_cascades_and_omitted_display_defaults() {
    let mut options = DurationFormatOptions::default();
    options.units[DurationUnit::Hours as usize].style = Some(DurationUnitStyle::Numeric);
    let resolved = resolve_duration_format_options(options).unwrap();
    assert_eq!(
        resolved.unit_style(DurationUnit::Hours),
        DurationUnitStyle::Numeric
    );
    assert_eq!(
        resolved.unit_style(DurationUnit::Minutes),
        DurationUnitStyle::TwoDigit
    );
    assert_eq!(
        resolved.unit_style(DurationUnit::Seconds),
        DurationUnitStyle::TwoDigit
    );
    for unit in [
        DurationUnit::Hours,
        DurationUnit::Minutes,
        DurationUnit::Seconds,
    ] {
        assert_eq!(
            resolved.unit_display(unit),
            DurationUnitDisplay::Always,
            "{unit:?} should retain the numeric display default"
        );
    }
    for unit in [
        DurationUnit::Milliseconds,
        DurationUnit::Microseconds,
        DurationUnit::Nanoseconds,
    ] {
        assert_eq!(resolved.unit_style(unit), DurationUnitStyle::Numeric);
        assert_eq!(resolved.unit_display(unit), DurationUnitDisplay::Auto);
    }

    let digital = resolve_duration_format_options(DurationFormatOptions {
        style: DurationStyle::Digital,
        ..Default::default()
    })
    .unwrap();
    for unit in [
        DurationUnit::Hours,
        DurationUnit::Minutes,
        DurationUnit::Seconds,
    ] {
        assert_eq!(digital.unit_display(unit), DurationUnitDisplay::Always);
    }
}

#[test]
fn rejects_invalid_numeric_styles_ordering_and_fractional_digits() {
    let mut calendar_numeric = DurationFormatOptions::default();
    calendar_numeric.units[DurationUnit::Days as usize].style = Some(DurationUnitStyle::Numeric);
    assert_eq!(
        resolve_duration_format_options(calendar_numeric),
        Err(DurationFormatOptionsError::UnsupportedUnitStyle)
    );

    let mut fractional_two_digit = DurationFormatOptions::default();
    fractional_two_digit.units[DurationUnit::Milliseconds as usize].style =
        Some(DurationUnitStyle::TwoDigit);
    assert_eq!(
        resolve_duration_format_options(fractional_two_digit),
        Err(DurationFormatOptionsError::UnsupportedUnitStyle)
    );

    let mut conflict = DurationFormatOptions::default();
    conflict.units[DurationUnit::Hours as usize].style = Some(DurationUnitStyle::Numeric);
    conflict.units[DurationUnit::Minutes as usize].style = Some(DurationUnitStyle::Short);
    assert_eq!(
        resolve_duration_format_options(conflict),
        Err(DurationFormatOptionsError::IncompatibleUnitStyles)
    );

    let mut fractional_display = DurationFormatOptions::default();
    fractional_display.units[DurationUnit::Milliseconds as usize].style =
        Some(DurationUnitStyle::Numeric);
    fractional_display.units[DurationUnit::Milliseconds as usize].display =
        Some(DurationUnitDisplay::Always);
    assert_eq!(
        resolve_duration_format_options(fractional_display),
        Err(DurationFormatOptionsError::FractionalUnitAlwaysDisplay)
    );

    let mut fractional_followed_by_unit = DurationFormatOptions::default();
    fractional_followed_by_unit.units[DurationUnit::Milliseconds as usize].style =
        Some(DurationUnitStyle::Numeric);
    fractional_followed_by_unit.units[DurationUnit::Microseconds as usize].style =
        Some(DurationUnitStyle::Short);
    assert_eq!(
        resolve_duration_format_options(fractional_followed_by_unit),
        Err(DurationFormatOptionsError::IncompatibleFractionalUnitStyles)
    );

    assert_eq!(
        resolve_duration_format_options(DurationFormatOptions {
            fractional_digits: Some(10),
            ..Default::default()
        }),
        Err(DurationFormatOptionsError::FractionalDigitsOutOfRange)
    );
}

#[test]
fn exposes_each_duration_option_error_text() {
    for (error, expected) in [
        (
            DurationFormatOptionsError::UnsupportedUnitStyle,
            "unsupported duration unit style",
        ),
        (
            DurationFormatOptionsError::IncompatibleUnitStyles,
            "duration unit style follows a numeric unit",
        ),
        (
            DurationFormatOptionsError::FractionalUnitAlwaysDisplay,
            "fractional duration units cannot always be displayed",
        ),
        (
            DurationFormatOptionsError::IncompatibleFractionalUnitStyles,
            "duration unit style follows a fractional unit",
        ),
        (
            DurationFormatOptionsError::FractionalDigitsOutOfRange,
            "fractional digits must be between zero and nine",
        ),
    ] {
        assert_eq!(error.to_string(), expected);
    }
}

#[test]
fn formats_english_long_short_narrow_and_digital_duration_patterns() {
    let record = duration(1, 2, 3, 3, 4, 5, 6, 7, 8, 9).unwrap();
    assert_eq!(
        formatter(DurationFormatOptions::default())
            .format(record)
            .unwrap(),
        "1 yr, 2 mths, 3 wks, 3 days, 4 hr, 5 min, 6 sec, 7 ms, 8 μs, 9 ns"
    );
    assert_eq!(
        formatter(DurationFormatOptions {
            style: DurationStyle::Long,
            ..Default::default()
        })
        .format(record)
        .unwrap(),
        "1 year, 2 months, 3 weeks, 3 days, 4 hours, 5 minutes, 6 seconds, 7 milliseconds, 8 microseconds, 9 nanoseconds"
    );
    assert_eq!(
        formatter(DurationFormatOptions {
            style: DurationStyle::Narrow,
            ..Default::default()
        })
        .format(record)
        .unwrap(),
        "1y 2m 3w 3d 4h 5m 6s 7ms 8μs 9ns"
    );
    assert_eq!(
        formatter(DurationFormatOptions {
            style: DurationStyle::Digital,
            ..Default::default()
        })
        .format(record)
        .unwrap(),
        "1 yr, 2 mths, 3 wks, 3 days, 4:05:06.007008009"
    );
}

#[test]
fn formats_exact_fractional_values_with_truncation_and_fixed_precision() {
    let record = duration(0, 0, 0, 0, 1, 22, 33, 111, 222, 333).unwrap();
    for (digits, expected) in [
        (0, "1:22:33"),
        (3, "1:22:33.111"),
        (4, "1:22:33.1112"),
        (6, "1:22:33.111222"),
        (8, "1:22:33.11122233"),
        (9, "1:22:33.111222333"),
    ] {
        assert_eq!(
            formatter(DurationFormatOptions {
                style: DurationStyle::Digital,
                fractional_digits: Some(digits),
                ..Default::default()
            })
            .format(record)
            .unwrap(),
            expected,
            "fractionalDigits={digits}"
        );
    }

    let mut numeric_seconds = DurationFormatOptions::default();
    numeric_seconds.units[DurationUnit::Seconds as usize].style = Some(DurationUnitStyle::Numeric);
    numeric_seconds.fractional_digits = Some(0);
    assert_eq!(
        formatter(numeric_seconds)
            .format(duration(0, 0, 0, 0, 0, 0, 1, 500, 0, 0).unwrap())
            .unwrap(),
        "1"
    );

    let mut fractional_milliseconds = DurationFormatOptions::default();
    fractional_milliseconds.units[DurationUnit::Milliseconds as usize].style =
        Some(DurationUnitStyle::Numeric);
    assert_eq!(
        formatter(fractional_milliseconds)
            .format(duration(0, 0, 0, 0, 0, 0, 0, 1, 250, 0).unwrap())
            .unwrap(),
        "0.00125 sec"
    );

    let mut fractional_milliseconds = DurationFormatOptions::default();
    fractional_milliseconds.units[DurationUnit::Microseconds as usize].style =
        Some(DurationUnitStyle::Numeric);
    assert_eq!(
        formatter(fractional_milliseconds)
            .format(duration(0, 0, 0, 0, 0, 0, 3, 444, 55, 6).unwrap())
            .unwrap(),
        "3 sec, 444.055006 ms"
    );
}

#[test]
fn formats_numeric_clock_groups_zero_units_large_values_and_negative_sign_once() {
    let mut clock = DurationFormatOptions::default();
    clock.units[DurationUnit::Hours as usize].style = Some(DurationUnitStyle::Numeric);
    assert_eq!(
        formatter(clock)
            .format(duration(0, 0, 0, 0, 1_234, 1_234_567, 12_345_678, 0, 0, 0).unwrap())
            .unwrap(),
        "1234:1234567:12345678"
    );

    let mut auto_hour = DurationFormatOptions {
        style: DurationStyle::Digital,
        ..Default::default()
    };
    auto_hour.units[DurationUnit::Hours as usize].display = Some(DurationUnitDisplay::Auto);
    assert_eq!(
        formatter(auto_hour)
            .format(duration(0, 0, 0, 1, 0, 1, 2, 0, 0, 0).unwrap())
            .unwrap(),
        "1 day, 01:02"
    );
    assert_eq!(
        formatter(DurationFormatOptions {
            style: DurationStyle::Digital,
            ..Default::default()
        })
        .format(duration(0, 0, 0, 0, -1, 0, -2, 0, 0, 0).unwrap())
        .unwrap(),
        "-1:00:02"
    );

    let mut leading_zero = DurationFormatOptions::default();
    leading_zero.units[DurationUnit::Hours as usize].display = Some(DurationUnitDisplay::Always);
    assert_eq!(
        formatter(leading_zero)
            .format(duration(0, 0, 0, 0, 0, 0, -1, 0, 0, 0).unwrap())
            .unwrap(),
        "-0 hr, 1 sec"
    );

    let mut leading_zero_digital = DurationFormatOptions {
        style: DurationStyle::Digital,
        ..Default::default()
    };
    leading_zero_digital.units[DurationUnit::Hours as usize].display =
        Some(DurationUnitDisplay::Always);
    assert_eq!(
        formatter(leading_zero_digital)
            .format(duration(0, 0, 0, 0, 0, 0, -1, 0, 0, 0).unwrap())
            .unwrap(),
        "-0:00:01"
    );
}

#[test]
fn exposes_ecma402_duration_parts_and_resolved_service_options() {
    let service = formatter(DurationFormatOptions {
        style: DurationStyle::Digital,
        ..Default::default()
    });
    assert_eq!(service.resolved_options().locale, "en");
    assert_eq!(service.resolved_options().numbering_system, "latn");
    assert_eq!(
        service
            .format_to_parts(duration(0, 0, 0, 0, 1, 2, 3, 0, 0, 0).unwrap())
            .unwrap(),
        vec![
            part(DurationPartKind::Integer, "1", Some(DurationUnit::Hours)),
            part(DurationPartKind::Literal, ":", None),
            part(DurationPartKind::Integer, "02", Some(DurationUnit::Minutes)),
            part(DurationPartKind::Literal, ":", None),
            part(DurationPartKind::Integer, "03", Some(DurationUnit::Seconds)),
        ]
    );

    let short = formatter(DurationFormatOptions::default());
    assert_eq!(
        short
            .format_to_parts(duration(0, 0, 0, 0, 1, 2, 0, 0, 0, 0).unwrap())
            .unwrap(),
        vec![
            part(DurationPartKind::Integer, "1", Some(DurationUnit::Hours)),
            part(DurationPartKind::Literal, " ", Some(DurationUnit::Hours)),
            part(DurationPartKind::Unit, "hr", Some(DurationUnit::Hours)),
            part(DurationPartKind::Literal, ", ", None),
            part(DurationPartKind::Integer, "2", Some(DurationUnit::Minutes)),
            part(DurationPartKind::Literal, " ", Some(DurationUnit::Minutes)),
            part(DurationPartKind::Unit, "min", Some(DurationUnit::Minutes)),
        ]
    );
}

#[test]
fn resolves_spanish_unit_data_and_rejects_invalid_publicly_constructed_records() {
    let spanish = canonicalize("es").unwrap();
    let supported =
        supported_duration_format_locales(std::slice::from_ref(&spanish), LocaleMatcher::Lookup);
    assert_eq!(supported.len(), 1);
    assert_eq!(supported.first(), Some(&spanish));
    assert_eq!(
        DurationFormat::try_new(&[spanish], DurationFormatOptions::default())
            .unwrap()
            .resolved_options()
            .locale,
        "es"
    );
    let spanish = DurationFormat::try_new(
        &[canonicalize("es").unwrap()],
        DurationFormatOptions {
            style: DurationStyle::Long,
            ..Default::default()
        },
    )
    .unwrap();
    let units = spanish
        .format_to_parts(duration(1, 2, 0, 0, 0, 0, 0, 0, 0, 0).unwrap())
        .unwrap()
        .into_iter()
        .filter(|part| part.kind == DurationPartKind::Unit)
        .map(|part| part.value)
        .collect::<Vec<_>>();
    assert_eq!(units, ["año", "meses"]);

    let invalid = DurationRecord {
        years: 1,
        months: -1,
        ..Default::default()
    };
    assert_eq!(
        formatter(DurationFormatOptions::default()).format(invalid),
        Err(DurationFormatError::InvalidDuration(
            DurationRecordError::MixedSign
        ))
    );
    assert!(formatter(DurationFormatOptions::default()).bytes() > 0);
}

#[test]
fn covers_duration_service_error_text_negotiation_and_remaining_english_patterns() {
    for (error, expected) in [
        (
            DurationFormatError::InvalidOptions(
                DurationFormatOptionsError::FractionalDigitsOutOfRange,
            ),
            "fractional digits must be between zero and nine",
        ),
        (
            DurationFormatError::InvalidDuration(DurationRecordError::NonIntegral),
            "duration fields must be integral",
        ),
        (
            DurationFormatError::ListFormattingUnavailable,
            "duration list-pattern data is unavailable",
        ),
    ] {
        assert_eq!(error.to_string(), expected);
    }

    let service = formatter(DurationFormatOptions::default());
    assert_eq!(service.negotiation().selected().as_str(), "en");
    assert_eq!(
        service
            .format(duration(0, 0, 0, 1_234_567, 0, 0, 0, 0, 0, 0).unwrap())
            .unwrap(),
        "1,234,567 days"
    );

    let one_of_every_unit = duration(1, 1, 1, 1, 1, 1, 1, 1, 1, 1).unwrap();
    assert_eq!(
        formatter(DurationFormatOptions {
            style: DurationStyle::Long,
            ..Default::default()
        })
        .format(one_of_every_unit)
        .unwrap(),
        "1 year, 1 month, 1 week, 1 day, 1 hour, 1 minute, 1 second, 1 millisecond, 1 microsecond, 1 nanosecond"
    );
    assert_eq!(
        formatter(DurationFormatOptions {
            style: DurationStyle::Long,
            ..Default::default()
        })
        .format(duration(2, 0, 0, 0, 0, 0, 0, 0, 0, 0).unwrap())
        .unwrap(),
        "2 years"
    );
    assert_eq!(
        formatter(DurationFormatOptions::default())
            .format(duration(2, 1, 1, 1, 1, 1, 1, 1, 1, 1).unwrap())
            .unwrap(),
        "2 yrs, 1 mth, 1 wk, 1 day, 1 hr, 1 min, 1 sec, 1 ms, 1 μs, 1 ns"
    );
}

#[test]
fn covers_zero_numeric_units_negative_fractional_parts_and_fraction_padding() {
    let mut seconds = DurationFormatOptions::default();
    seconds.units[DurationUnit::Seconds as usize].style = Some(DurationUnitStyle::Numeric);
    assert_eq!(
        formatter(seconds)
            .format(DurationRecord::default())
            .unwrap(),
        "0"
    );
    assert_eq!(
        formatter(seconds)
            .format(duration(0, 0, 0, 0, 0, 0, -1, -500, 0, 0).unwrap())
            .unwrap(),
        "-1.5"
    );

    assert_eq!(
        formatter(DurationFormatOptions {
            style: DurationStyle::Digital,
            ..Default::default()
        })
        .format(duration(0, 0, 0, 0, 0, 0, 10_000_000, 0, 0, 1).unwrap())
        .unwrap(),
        "0:00:10000000.000000001"
    );

    assert_eq!(
        formatter(DurationFormatOptions {
            style: DurationStyle::Digital,
            ..Default::default()
        })
        .format(duration(0, 0, 0, 0, 0, 0, 1, 2, 3, 9_007_199_254_740_991,).unwrap(),)
        .unwrap(),
        "0:00:9007200.256743991"
    );

    let mut nanoseconds = DurationFormatOptions::default();
    nanoseconds.units[DurationUnit::Nanoseconds as usize].style = Some(DurationUnitStyle::Numeric);
    nanoseconds.fractional_digits = Some(9);
    assert_eq!(
        formatter(nanoseconds)
            .format(duration(0, 0, 0, 0, 0, 0, 0, 0, 0, 1).unwrap())
            .unwrap(),
        "0.001000000 μs"
    );

    let mut minutes = DurationFormatOptions::default();
    minutes.units[DurationUnit::Minutes as usize].style = Some(DurationUnitStyle::Numeric);
    assert_eq!(
        formatter(minutes)
            .format(DurationRecord::default())
            .unwrap(),
        "0:00"
    );

    minutes.units[DurationUnit::Minutes as usize].display = Some(DurationUnitDisplay::Auto);
    minutes.units[DurationUnit::Seconds as usize].display = Some(DurationUnitDisplay::Auto);
    assert_eq!(
        formatter(minutes)
            .format(duration(0, 0, 0, 0, 0, 1, 0, 0, 0, 0).unwrap())
            .unwrap(),
        "1"
    );
}
