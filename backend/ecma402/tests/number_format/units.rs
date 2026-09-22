// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

use super::*;

#[test]
fn formats_duration_units_and_parts_through_the_number_service() {
    let english = canonicalize("en").unwrap();
    let short_year = NumberFormat::try_new(
        std::slice::from_ref(&english),
        NumberFormatOptions {
            style: NumberFormatStyle::Unit,
            unit: Some(NumberFormatUnit::Year),
            ..Default::default()
        },
    )
    .unwrap();
    assert_eq!(short_year.format_f64(2.0).unwrap(), "2 yrs");
    assert_eq!(
        short_year
            .format_to_parts_decimal("1234.5")
            .unwrap()
            .into_iter()
            .map(|part| (part.kind, part.value))
            .collect::<Vec<_>>(),
        vec![
            (NumberFormatPartKind::Integer, "1".into()),
            (NumberFormatPartKind::Group, ",".into()),
            (NumberFormatPartKind::Integer, "234".into()),
            (NumberFormatPartKind::Decimal, ".".into()),
            (NumberFormatPartKind::Fraction, "5".into()),
            (NumberFormatPartKind::Literal, " ".into()),
            (NumberFormatPartKind::Unit, "yrs".into()),
        ]
    );
    assert_eq!(
        NumberFormatUnit::parse("microsecond"),
        Some(NumberFormatUnit::Microsecond)
    );
    assert_eq!(NumberFormatUnit::parse("invalid"), None);
}

#[test]
fn sources_korean_cldr_digital_temperature_and_generic_per_units() {
    let formatter = |unit, display| {
        NumberFormat::try_new(
            &[canonicalize("ko").unwrap()],
            NumberFormatOptions {
                style: NumberFormatStyle::Unit,
                unit: Some(unit),
                unit_display: display,
                ..Default::default()
            },
        )
        .unwrap()
    };

    assert_eq!(
        formatter(NumberFormatUnit::Gigabyte, NumberUnitDisplay::Long)
            .format_f64(2.0)
            .unwrap(),
        "2기가바이트"
    );
    assert_eq!(
        formatter(NumberFormatUnit::Celsius, NumberUnitDisplay::Long)
            .format_f64(2.0)
            .unwrap(),
        "섭씨 2도"
    );
    assert_eq!(
        formatter(NumberFormatUnit::Percent, NumberUnitDisplay::Long)
            .format_f64(2.0)
            .unwrap(),
        "2%"
    );
    assert_eq!(
        formatter(NumberFormatUnit::Second, NumberUnitDisplay::Long)
            .format_f64(2.0)
            .unwrap(),
        "2초"
    );
    let celsius = formatter(NumberFormatUnit::Celsius, NumberUnitDisplay::Long);
    assert_eq!(
        celsius
            .format_range_inputs(
                NumberFormatInput::Number(3.0),
                NumberFormatInput::Number(5.0),
            )
            .unwrap(),
        "섭씨 3~5도"
    );

    let per_second = formatter(
        NumberFormatUnit::parse("gigabyte-per-second").unwrap(),
        NumberUnitDisplay::Long,
    );
    assert_eq!(per_second.format_f64(2.0).unwrap(), "초당 2기가바이트");
    assert_eq!(
        per_second
            .format_to_parts_f64(2.0)
            .unwrap()
            .into_iter()
            .map(|part| (part.kind, part.value))
            .collect::<Vec<_>>(),
        vec![
            (NumberFormatPartKind::Unit, "초당".into()),
            (NumberFormatPartKind::Literal, " ".into()),
            (NumberFormatPartKind::Integer, "2".into()),
            (NumberFormatPartKind::Unit, "기가바이트".into()),
        ]
    );
    assert_eq!(
        per_second
            .format_range_inputs(
                NumberFormatInput::Number(3.0),
                NumberFormatInput::Number(5.0),
            )
            .unwrap(),
        "초당 3~5기가바이트"
    );
    assert_eq!(
        formatter(
            NumberFormatUnit::parse("gigabyte-per-second").unwrap(),
            NumberUnitDisplay::Short,
        )
        .format_f64(2.0)
        .unwrap(),
        "2GB/s"
    );
    assert_eq!(
        formatter(
            NumberFormatUnit::parse("gigabyte-per-second").unwrap(),
            NumberUnitDisplay::Narrow,
        )
        .format_f64(2.0)
        .unwrap(),
        "2GB/초"
    );
    assert_eq!(
        formatter(
            NumberFormatUnit::parse("gigabyte-per-celsius").unwrap(),
            NumberUnitDisplay::Long,
        )
        .format_f64(2.0)
        .unwrap(),
        "섭씨당 2기가바이트"
    );
    assert_eq!(
        formatter(
            NumberFormatUnit::parse("celsius-per-second").unwrap(),
            NumberUnitDisplay::Long,
        )
        .format_f64(2.0)
        .unwrap(),
        "초당 섭씨 2도"
    );
}

#[test]
fn sources_chinese_cldr_simple_and_generic_per_units_by_script() {
    let formatter = |locale, unit, display| {
        NumberFormat::try_new(
            &[canonicalize(locale).unwrap()],
            NumberFormatOptions {
                style: NumberFormatStyle::Unit,
                unit: Some(unit),
                unit_display: display,
                ..Default::default()
            },
        )
        .unwrap()
    };

    assert_eq!(
        formatter("zh", NumberFormatUnit::Gigabyte, NumberUnitDisplay::Long)
            .format_f64(2.0)
            .unwrap(),
        "2吉字节"
    );
    assert_eq!(
        formatter("zh", NumberFormatUnit::Celsius, NumberUnitDisplay::Long)
            .format_f64(2.0)
            .unwrap(),
        "2摄氏度"
    );
    assert_eq!(
        formatter("zh", NumberFormatUnit::Gigabyte, NumberUnitDisplay::Short)
            .format_f64(2.0)
            .unwrap(),
        "2 GB"
    );
    assert_eq!(
        formatter(
            "zh",
            NumberFormatUnit::parse("gigabyte-per-second").unwrap(),
            NumberUnitDisplay::Long,
        )
        .format_f64(2.0)
        .unwrap(),
        "2吉字节/秒"
    );

    assert_eq!(
        formatter("zh-TW", NumberFormatUnit::Celsius, NumberUnitDisplay::Long)
            .format_f64(2.0)
            .unwrap(),
        "攝氏 2 度"
    );
    assert_eq!(
        formatter(
            "zh-TW",
            NumberFormatUnit::parse("gigabyte-per-second").unwrap(),
            NumberUnitDisplay::Long,
        )
        .format_f64(2.0)
        .unwrap(),
        "每秒 2 GB"
    );
    assert_eq!(
        formatter(
            "zh-TW",
            NumberFormatUnit::parse("celsius-per-second").unwrap(),
            NumberUnitDisplay::Long,
        )
        .format_f64(2.0)
        .unwrap(),
        "每秒 攝氏 2 度"
    );
    assert_eq!(
        formatter(
            "zh-TW",
            NumberFormatUnit::parse("gigabyte-per-second").unwrap(),
            NumberUnitDisplay::Narrow,
        )
        .format_f64(2.0)
        .unwrap(),
        "2GB/秒"
    );
}

#[test]
fn sources_german_cldr_temperature_and_angle_widths() {
    let formatter = |unit, display| {
        NumberFormat::try_new(
            &[canonicalize("de").unwrap()],
            NumberFormatOptions {
                style: NumberFormatStyle::Unit,
                unit: Some(unit),
                unit_display: display,
                ..Default::default()
            },
        )
        .unwrap()
    };

    assert_eq!(
        formatter(NumberFormatUnit::Celsius, NumberUnitDisplay::Long)
            .format_f64(2.0)
            .unwrap(),
        "2 Grad Celsius"
    );
    assert_eq!(
        formatter(NumberFormatUnit::Fahrenheit, NumberUnitDisplay::Short)
            .format_f64(2.0)
            .unwrap(),
        "2 °F"
    );
    assert_eq!(
        formatter(NumberFormatUnit::Degree, NumberUnitDisplay::Narrow)
            .format_f64(2.0)
            .unwrap(),
        "2°"
    );
    assert_eq!(
        formatter(NumberFormatUnit::Gigabyte, NumberUnitDisplay::Long)
            .format_f64(1.0)
            .unwrap(),
        "1\u{a0}Gigabyte"
    );
    assert_eq!(
        formatter(NumberFormatUnit::Gigabyte, NumberUnitDisplay::Long)
            .format_f64(2.0)
            .unwrap(),
        "2 Gigabyte"
    );
    assert_eq!(
        formatter(NumberFormatUnit::Gigabyte, NumberUnitDisplay::Short)
            .format_f64(2.0)
            .unwrap(),
        "2\u{a0}GB"
    );
    let per_second = formatter(
        NumberFormatUnit::parse("gigabyte-per-second").unwrap(),
        NumberUnitDisplay::Long,
    );
    assert_eq!(
        per_second.format_f64(2.0).unwrap(),
        "2 Gigabyte pro Sekunde"
    );
    assert_eq!(
        formatter(
            NumberFormatUnit::parse("gigabyte-per-second").unwrap(),
            NumberUnitDisplay::Short,
        )
        .format_f64(2.0)
        .unwrap(),
        "2\u{a0}GB/s"
    );
    assert_eq!(
        formatter(
            NumberFormatUnit::parse("celsius-per-second").unwrap(),
            NumberUnitDisplay::Long,
        )
        .format_f64(2.0)
        .unwrap(),
        "2 Grad Celsius pro Sekunde"
    );
}

#[test]
fn sources_portuguese_cldr_digital_temperature_and_percent_units() {
    let formatter = |unit, display| {
        NumberFormat::try_new(
            &[canonicalize("pt").unwrap()],
            NumberFormatOptions {
                style: NumberFormatStyle::Unit,
                unit: Some(unit),
                unit_display: display,
                ..Default::default()
            },
        )
        .unwrap()
    };

    assert_eq!(
        formatter(NumberFormatUnit::Gigabyte, NumberUnitDisplay::Long)
            .format_f64(1.0)
            .unwrap(),
        "1 gigabyte"
    );
    assert_eq!(
        formatter(NumberFormatUnit::Gigabyte, NumberUnitDisplay::Long)
            .format_f64(2.0)
            .unwrap(),
        "2 gigabytes"
    );
    assert_eq!(
        formatter(NumberFormatUnit::Celsius, NumberUnitDisplay::Long)
            .format_f64(2.0)
            .unwrap(),
        "2 graus Celsius"
    );
    assert_eq!(
        formatter(NumberFormatUnit::Percent, NumberUnitDisplay::Long)
            .format_f64(2.0)
            .unwrap(),
        "2 por cento"
    );
    assert_eq!(
        formatter(NumberFormatUnit::Bit, NumberUnitDisplay::Narrow)
            .format_f64(2.0)
            .unwrap(),
        "2 bits"
    );
    assert_eq!(
        formatter(
            NumberFormatUnit::parse("gigabyte-per-second").unwrap(),
            NumberUnitDisplay::Long,
        )
        .format_f64(2.0)
        .unwrap(),
        "2 gigabytes por segundo"
    );
    assert_eq!(
        formatter(
            NumberFormatUnit::parse("gigabyte-per-hour").unwrap(),
            NumberUnitDisplay::Long,
        )
        .format_f64(2.0)
        .unwrap(),
        "2 gigabytes por hora"
    );
    let european = NumberFormat::try_new(
        &[canonicalize("pt-PT").unwrap()],
        NumberFormatOptions {
            style: NumberFormatStyle::Unit,
            unit: Some(NumberFormatUnit::parse("gigabyte-per-second").unwrap()),
            unit_display: NumberUnitDisplay::Long,
            ..Default::default()
        },
    )
    .unwrap();
    assert_eq!(european.format_f64(2.0).unwrap(), "2 gigabytes/s");
    let african = NumberFormat::try_new(
        &[canonicalize("pt-AO").unwrap()],
        NumberFormatOptions {
            style: NumberFormatStyle::Unit,
            unit: Some(NumberFormatUnit::parse("gigabyte-per-hour").unwrap()),
            unit_display: NumberUnitDisplay::Long,
            ..Default::default()
        },
    )
    .unwrap();
    assert_eq!(african.format_f64(2.0).unwrap(), "2 gigabytes/h");
}

#[test]
fn sources_italian_cldr_digital_temperature_angle_and_percent_units() {
    let formatter = |unit, display| {
        NumberFormat::try_new(
            &[canonicalize("it").unwrap()],
            NumberFormatOptions {
                style: NumberFormatStyle::Unit,
                unit: Some(unit),
                unit_display: display,
                ..Default::default()
            },
        )
        .unwrap()
    };

    assert_eq!(
        formatter(NumberFormatUnit::Gigabyte, NumberUnitDisplay::Long)
            .format_f64(2.0)
            .unwrap(),
        "2 gigabyte"
    );
    assert_eq!(
        formatter(NumberFormatUnit::Celsius, NumberUnitDisplay::Long)
            .format_f64(1.0)
            .unwrap(),
        "1 grado Celsius"
    );
    assert_eq!(
        formatter(NumberFormatUnit::Celsius, NumberUnitDisplay::Long)
            .format_f64(2.0)
            .unwrap(),
        "2 gradi Celsius"
    );
    assert_eq!(
        formatter(NumberFormatUnit::Degree, NumberUnitDisplay::Short)
            .format_f64(2.0)
            .unwrap(),
        "2°"
    );
    assert_eq!(
        formatter(NumberFormatUnit::Percent, NumberUnitDisplay::Long)
            .format_f64(2.0)
            .unwrap(),
        "2 percento"
    );
    assert_eq!(
        formatter(
            NumberFormatUnit::parse("gigabyte-per-second").unwrap(),
            NumberUnitDisplay::Long,
        )
        .format_f64(2.0)
        .unwrap(),
        "2 gigabyte al secondo"
    );
    assert_eq!(
        formatter(
            NumberFormatUnit::parse("gigabyte-per-hour").unwrap(),
            NumberUnitDisplay::Long,
        )
        .format_f64(2.0)
        .unwrap(),
        "2 gigabyte all’ora"
    );
    assert_eq!(
        formatter(
            NumberFormatUnit::parse("gigabyte-per-second").unwrap(),
            NumberUnitDisplay::Narrow,
        )
        .format_f64(2.0)
        .unwrap(),
        "2GB/s"
    );
    assert_eq!(
        formatter(
            NumberFormatUnit::parse("gigabyte-per-week").unwrap(),
            NumberUnitDisplay::Narrow,
        )
        .format_f64(2.0)
        .unwrap(),
        "2GB/sett."
    );
}

#[test]
fn sources_dutch_cldr_digital_temperature_angle_and_generic_per_units() {
    let formatter = |unit, display| {
        NumberFormat::try_new(
            &[canonicalize("nl").unwrap()],
            NumberFormatOptions {
                style: NumberFormatStyle::Unit,
                unit: Some(unit),
                unit_display: display,
                ..Default::default()
            },
        )
        .unwrap()
    };

    assert_eq!(
        formatter(NumberFormatUnit::Bit, NumberUnitDisplay::Long)
            .format_f64(1.0)
            .unwrap(),
        "1 bit"
    );
    assert_eq!(
        formatter(NumberFormatUnit::Bit, NumberUnitDisplay::Long)
            .format_f64(2.0)
            .unwrap(),
        "2 bits"
    );
    assert_eq!(
        formatter(NumberFormatUnit::Celsius, NumberUnitDisplay::Long)
            .format_f64(2.0)
            .unwrap(),
        "2 graden Celsius"
    );
    assert_eq!(
        formatter(NumberFormatUnit::Celsius, NumberUnitDisplay::Narrow)
            .format_f64(2.0)
            .unwrap(),
        "2°"
    );
    assert_eq!(
        formatter(NumberFormatUnit::Degree, NumberUnitDisplay::Long)
            .format_f64(2.0)
            .unwrap(),
        "2 booggraden"
    );
    assert_eq!(
        formatter(
            NumberFormatUnit::parse("gigabyte-per-second").unwrap(),
            NumberUnitDisplay::Long,
        )
        .format_f64(2.0)
        .unwrap(),
        "2 gigabyte per seconde"
    );
    assert_eq!(
        formatter(
            NumberFormatUnit::parse("gigabyte-per-hour").unwrap(),
            NumberUnitDisplay::Short,
        )
        .format_f64(2.0)
        .unwrap(),
        "2 GB/uur"
    );
    assert_eq!(
        formatter(
            NumberFormatUnit::parse("celsius-per-second").unwrap(),
            NumberUnitDisplay::Narrow,
        )
        .format_f64(2.0)
        .unwrap(),
        "2°/s"
    );
}

#[test]
fn sources_turkish_cldr_prefix_units_and_generic_per_units() {
    let formatter = |unit, display| {
        NumberFormat::try_new(
            &[canonicalize("tr").unwrap()],
            NumberFormatOptions {
                style: NumberFormatStyle::Unit,
                unit: Some(unit),
                unit_display: display,
                ..Default::default()
            },
        )
        .unwrap()
    };

    assert_eq!(
        formatter(NumberFormatUnit::Percent, NumberUnitDisplay::Long)
            .format_f64(2.0)
            .unwrap(),
        "yüzde 2"
    );
    assert_eq!(
        formatter(NumberFormatUnit::Percent, NumberUnitDisplay::Short)
            .format_f64(2.0)
            .unwrap(),
        "%2"
    );
    assert_eq!(
        formatter(NumberFormatUnit::Celsius, NumberUnitDisplay::Short)
            .format_f64(1.0)
            .unwrap(),
        "1 °C"
    );
    assert_eq!(
        formatter(NumberFormatUnit::Celsius, NumberUnitDisplay::Short)
            .format_f64(2.0)
            .unwrap(),
        "2°C"
    );
    assert_eq!(
        formatter(NumberFormatUnit::Celsius, NumberUnitDisplay::Narrow)
            .format_f64(1.0)
            .unwrap(),
        "1°C"
    );
    assert_eq!(
        formatter(NumberFormatUnit::Celsius, NumberUnitDisplay::Narrow)
            .format_f64(2.0)
            .unwrap(),
        "2 °C"
    );
    assert_eq!(
        formatter(
            NumberFormatUnit::parse("gigabyte-per-second").unwrap(),
            NumberUnitDisplay::Long,
        )
        .format_f64(2.0)
        .unwrap(),
        "2 gigabayt/saniye"
    );
    assert_eq!(
        formatter(
            NumberFormatUnit::parse("gigabyte-per-hour").unwrap(),
            NumberUnitDisplay::Short,
        )
        .format_f64(2.0)
        .unwrap(),
        "2 GB/sa"
    );
    let cypriot = NumberFormat::try_new(
        &[canonicalize("tr-CY").unwrap()],
        NumberFormatOptions {
            style: NumberFormatStyle::Unit,
            unit: Some(NumberFormatUnit::parse("celsius-per-second").unwrap()),
            unit_display: NumberUnitDisplay::Short,
            ..Default::default()
        },
    )
    .unwrap();
    assert_eq!(cypriot.format_f64(2.0).unwrap(), "2°C/sn");
}

#[test]
fn sources_hindi_devanagari_cldr_units_and_generic_per_patterns() {
    let formatter = |unit, display| {
        NumberFormat::try_new(
            &[canonicalize("hi").unwrap()],
            NumberFormatOptions {
                style: NumberFormatStyle::Unit,
                unit: Some(unit),
                unit_display: display,
                ..Default::default()
            },
        )
        .unwrap()
    };

    assert_eq!(
        formatter(NumberFormatUnit::Percent, NumberUnitDisplay::Long)
            .format_f64(2.0)
            .unwrap(),
        "2 प्रतिशत"
    );
    assert_eq!(
        formatter(NumberFormatUnit::Celsius, NumberUnitDisplay::Short)
            .format_f64(2.0)
            .unwrap(),
        "2°से॰"
    );
    assert_eq!(
        formatter(NumberFormatUnit::Degree, NumberUnitDisplay::Narrow)
            .format_f64(2.0)
            .unwrap(),
        "2°"
    );
    assert_eq!(
        formatter(
            NumberFormatUnit::parse("gigabyte-per-second").unwrap(),
            NumberUnitDisplay::Long,
        )
        .format_f64(2.0)
        .unwrap(),
        "2 गीगाबाइट प्रति सेकंड"
    );
    assert_eq!(
        formatter(
            NumberFormatUnit::parse("gigabyte-per-hour").unwrap(),
            NumberUnitDisplay::Short,
        )
        .format_f64(2.0)
        .unwrap(),
        "2 GB/घं॰"
    );
    let latin = NumberFormat::try_new(
        &[canonicalize("hi-Latn").unwrap()],
        NumberFormatOptions {
            style: NumberFormatStyle::Unit,
            unit: Some(NumberFormatUnit::Gigabyte),
            unit_display: NumberUnitDisplay::Long,
            ..Default::default()
        },
    )
    .unwrap();
    assert_eq!(latin.format_f64(2.0).unwrap(), "2 gigabytes");
}

#[test]
fn sources_greek_cldr_inflection_and_generic_per_patterns() {
    let formatter = |unit, display| {
        NumberFormat::try_new(
            &[canonicalize("el").unwrap()],
            NumberFormatOptions {
                style: NumberFormatStyle::Unit,
                unit: Some(unit),
                unit_display: display,
                ..Default::default()
            },
        )
        .unwrap()
    };

    assert_eq!(
        formatter(NumberFormatUnit::Celsius, NumberUnitDisplay::Long)
            .format_f64(1.0)
            .unwrap(),
        "1 βαθμός Κελσίου"
    );
    assert_eq!(
        formatter(NumberFormatUnit::Celsius, NumberUnitDisplay::Long)
            .format_f64(2.0)
            .unwrap(),
        "2 βαθμοί Κελσίου"
    );
    assert_eq!(
        formatter(NumberFormatUnit::Gigabit, NumberUnitDisplay::Long)
            .format_f64(2.0)
            .unwrap(),
        "2 gigabit"
    );
    assert_eq!(
        formatter(NumberFormatUnit::Percent, NumberUnitDisplay::Long)
            .format_f64(2.0)
            .unwrap(),
        "2 τοις εκατό"
    );
    assert_eq!(
        formatter(
            NumberFormatUnit::parse("gigabyte-per-second").unwrap(),
            NumberUnitDisplay::Long,
        )
        .format_f64(2.0)
        .unwrap(),
        "2 gigabyte ανά δευτερόλεπτο"
    );
    assert_eq!(
        formatter(
            NumberFormatUnit::parse("gigabyte-per-hour").unwrap(),
            NumberUnitDisplay::Short,
        )
        .format_f64(2.0)
        .unwrap(),
        "2 GB/ώ."
    );
    let cypriot = NumberFormat::try_new(
        &[canonicalize("el-CY").unwrap()],
        NumberFormatOptions {
            style: NumberFormatStyle::Unit,
            unit: Some(NumberFormatUnit::parse("celsius-per-second").unwrap()),
            unit_display: NumberUnitDisplay::Narrow,
            ..Default::default()
        },
    )
    .unwrap();
    assert_eq!(cypriot.format_f64(2.0).unwrap(), "2°C/δ");
}

#[test]
fn sources_polish_cldr_cardinal_forms_and_generic_per_patterns() {
    let formatter = |unit, display| {
        NumberFormat::try_new(
            &[canonicalize("pl").unwrap()],
            NumberFormatOptions {
                style: NumberFormatStyle::Unit,
                unit: Some(unit),
                unit_display: display,
                ..Default::default()
            },
        )
        .unwrap()
    };

    assert_eq!(
        formatter(NumberFormatUnit::Gigabyte, NumberUnitDisplay::Long)
            .format_f64(1.0)
            .unwrap(),
        "1 gigabajt"
    );
    assert_eq!(
        formatter(NumberFormatUnit::Gigabyte, NumberUnitDisplay::Long)
            .format_f64(2.0)
            .unwrap(),
        "2 gigabajty"
    );
    assert_eq!(
        formatter(NumberFormatUnit::Gigabyte, NumberUnitDisplay::Long)
            .format_f64(5.0)
            .unwrap(),
        "5 gigabajtów"
    );
    assert_eq!(
        formatter(NumberFormatUnit::Gigabyte, NumberUnitDisplay::Long)
            .format_f64(1.5)
            .unwrap(),
        "1,5 gigabajta"
    );
    assert_eq!(
        formatter(NumberFormatUnit::Celsius, NumberUnitDisplay::Long)
            .format_f64(5.0)
            .unwrap(),
        "5 stopni Celsjusza"
    );
    assert_eq!(
        formatter(
            NumberFormatUnit::parse("gigabyte-per-second").unwrap(),
            NumberUnitDisplay::Long,
        )
        .format_f64(2.0)
        .unwrap(),
        "2 gigabajty na sekundę"
    );
    assert_eq!(
        formatter(
            NumberFormatUnit::parse("gigabyte-per-hour").unwrap(),
            NumberUnitDisplay::Short,
        )
        .format_f64(2.0)
        .unwrap(),
        "2 GB/godz."
    );
    assert_eq!(
        formatter(
            NumberFormatUnit::parse("celsius-per-second").unwrap(),
            NumberUnitDisplay::Narrow,
        )
        .format_f64(2.0)
        .unwrap(),
        "2°C/s"
    );
}

#[test]
fn sources_hebrew_cldr_bidi_hidden_number_and_generic_per_patterns() {
    let formatter = |unit, display| {
        NumberFormat::try_new(
            &[canonicalize("he").unwrap()],
            NumberFormatOptions {
                style: NumberFormatStyle::Unit,
                unit: Some(unit),
                unit_display: display,
                ..Default::default()
            },
        )
        .unwrap()
    };

    assert_eq!(
        formatter(NumberFormatUnit::Degree, NumberUnitDisplay::Long)
            .format_f64(1.0)
            .unwrap(),
        "מעלה אחת"
    );
    assert_eq!(
        formatter(NumberFormatUnit::Degree, NumberUnitDisplay::Long)
            .format_f64(2.0)
            .unwrap(),
        "שתי מעלות"
    );
    assert_eq!(
        formatter(NumberFormatUnit::Celsius, NumberUnitDisplay::Long)
            .format_f64(1.0)
            .unwrap(),
        "1 מעלת צלזיוס"
    );
    assert_eq!(
        formatter(NumberFormatUnit::Gigabyte, NumberUnitDisplay::Short)
            .format_f64(1.0)
            .unwrap(),
        "GB\u{200f}1"
    );
    assert_eq!(
        formatter(
            NumberFormatUnit::parse("gigabyte-per-second").unwrap(),
            NumberUnitDisplay::Long,
        )
        .format_f64(2.0)
        .unwrap(),
        "2 ג׳יגה-בייט לשניה"
    );
    assert_eq!(
        formatter(
            NumberFormatUnit::parse("gigabyte-per-second").unwrap(),
            NumberUnitDisplay::Short,
        )
        .format_f64(2.0)
        .unwrap(),
        "2 GB/שנ׳"
    );
    assert_eq!(
        formatter(
            NumberFormatUnit::parse("celsius-per-second").unwrap(),
            NumberUnitDisplay::Narrow,
        )
        .format_f64(2.0)
        .unwrap(),
        "2°C/שנ׳"
    );
}

#[test]
fn parses_compound_units_and_keeps_their_locale_pattern_part_boundaries() {
    let unit = NumberFormatUnit::parse("kilometer-per-hour").unwrap();
    assert!(unit.is_compound());
    assert_eq!(unit.identifier(), "kilometer-per-hour");
    assert_eq!(
        unit.compound_parts(),
        Some((NumberFormatUnit::Kilometer, NumberFormatUnit::Hour))
    );
    assert!(NumberFormatUnit::parse("meter-per-second").is_some());
    assert_eq!(NumberFormatUnit::parse("meter-per-per-second"), None);
    assert_eq!(NumberFormatUnit::parse("per-hour"), None);

    let format = NumberFormat::try_new(
        &[canonicalize("ko-KR").unwrap()],
        NumberFormatOptions {
            style: NumberFormatStyle::Unit,
            unit: Some(unit),
            unit_display: NumberUnitDisplay::Long,
            ..Default::default()
        },
    )
    .unwrap();
    assert_eq!(
        format
            .format_to_parts_f64(-987.0)
            .unwrap()
            .into_iter()
            .map(|part| (part.kind, part.value))
            .collect::<Vec<_>>(),
        vec![
            (NumberFormatPartKind::Unit, "시속".into()),
            (NumberFormatPartKind::Literal, " ".into()),
            (NumberFormatPartKind::MinusSign, "-".into()),
            (NumberFormatPartKind::Integer, "987".into()),
            (NumberFormatPartKind::Unit, "킬로미터".into()),
        ]
    );

    let generic = NumberFormat::try_new(
        &[canonicalize("en-US").unwrap()],
        NumberFormatOptions {
            style: NumberFormatStyle::Unit,
            unit: Some(NumberFormatUnit::parse("meter-per-second").unwrap()),
            unit_display: NumberUnitDisplay::Long,
            ..Default::default()
        },
    )
    .unwrap();
    assert_eq!(generic.format_f64(2.0).unwrap(), "2 meters per second");

    let french_generic = NumberFormat::try_new(
        &[canonicalize("fr").unwrap()],
        NumberFormatOptions {
            style: NumberFormatStyle::Unit,
            unit: Some(NumberFormatUnit::parse("meter-per-second").unwrap()),
            unit_display: NumberUnitDisplay::Long,
            ..Default::default()
        },
    )
    .unwrap();
    assert_eq!(
        french_generic.format_f64(2.0).unwrap(),
        "2\u{a0}mètres par seconde"
    );
    assert_eq!(
        french_generic
            .format_to_parts_f64(2.0)
            .unwrap()
            .into_iter()
            .map(|part| (part.kind, part.value))
            .collect::<Vec<_>>(),
        vec![
            (NumberFormatPartKind::Integer, "2".into()),
            (NumberFormatPartKind::Literal, "\u{a0}".into()),
            (NumberFormatPartKind::Unit, "mètres par seconde".into(),),
        ]
    );

    let full_english_inventory = NumberFormat::try_new(
        &[canonicalize("en").unwrap()],
        NumberFormatOptions {
            style: NumberFormatStyle::Unit,
            unit: Some(NumberFormatUnit::parse("fluid-ounce-per-second").unwrap()),
            unit_display: NumberUnitDisplay::Short,
            ..Default::default()
        },
    )
    .unwrap();
    assert_eq!(full_english_inventory.format_f64(2.0).unwrap(), "2 fl oz/s");
}

#[test]
fn loads_localized_simple_unit_patterns_from_the_shared_cldr_provider() {
    let french = NumberFormat::try_new(
        &[canonicalize("fr").unwrap()],
        NumberFormatOptions {
            style: NumberFormatStyle::Unit,
            unit: Some(NumberFormatUnit::Meter),
            unit_display: NumberUnitDisplay::Long,
            ..Default::default()
        },
    )
    .unwrap();
    assert_eq!(french.format_f64(2.0).unwrap(), "2\u{a0}mètres");
    assert_eq!(
        french
            .format_to_parts_f64(2.0)
            .unwrap()
            .into_iter()
            .map(|part| (part.kind, part.value))
            .collect::<Vec<_>>(),
        vec![
            (NumberFormatPartKind::Integer, "2".into()),
            (NumberFormatPartKind::Literal, "\u{a0}".into()),
            (NumberFormatPartKind::Unit, "mètres".into()),
        ]
    );

    let german = NumberFormat::try_new(
        &[canonicalize("de").unwrap()],
        NumberFormatOptions {
            style: NumberFormatStyle::Unit,
            unit: Some(NumberFormatUnit::Liter),
            unit_display: NumberUnitDisplay::Short,
            ..Default::default()
        },
    )
    .unwrap();
    assert_eq!(german.format_f64(2.0).unwrap(), "2 l");

    let arabic = NumberFormat::try_new(
        &[canonicalize("ar-u-nu-arab").unwrap()],
        NumberFormatOptions {
            style: NumberFormatStyle::Unit,
            unit: Some(NumberFormatUnit::Meter),
            unit_display: NumberUnitDisplay::Long,
            ..Default::default()
        },
    )
    .unwrap();
    assert_eq!(arabic.format_f64(1.0).unwrap(), "متر");
    assert_eq!(
        arabic
            .format_to_parts_f64(1.0)
            .unwrap()
            .into_iter()
            .map(|part| (part.kind, part.value))
            .collect::<Vec<_>>(),
        vec![(NumberFormatPartKind::Unit, "متر".into())]
    );
    assert_eq!(
        arabic
            .format_range_inputs(
                NumberFormatInput::Number(0.0),
                NumberFormatInput::Number(1.0),
            )
            .unwrap(),
        "٠–١ متر"
    );
    assert_eq!(
        arabic
            .format_range_inputs_to_parts(
                NumberFormatInput::Number(0.0),
                NumberFormatInput::Number(1.0),
            )
            .unwrap()
            .into_iter()
            .map(|part| (part.kind, part.value, part.source))
            .collect::<Vec<_>>(),
        vec![
            (
                NumberFormatPartKind::Integer,
                "٠".into(),
                NumberRangePartSource::StartRange,
            ),
            (
                NumberFormatPartKind::Literal,
                "–".into(),
                NumberRangePartSource::Shared,
            ),
            (
                NumberFormatPartKind::Integer,
                "١".into(),
                NumberRangePartSource::EndRange,
            ),
            (
                NumberFormatPartKind::Literal,
                " ".into(),
                NumberRangePartSource::Shared,
            ),
            (
                NumberFormatPartKind::Unit,
                "متر".into(),
                NumberRangePartSource::Shared,
            ),
        ]
    );
}

#[test]
fn loads_full_cldr_units_for_previously_untyped_locale_cells() {
    let afrikaans = NumberFormat::try_new(
        &[canonicalize("af-NA-u-nu-latn").unwrap()],
        NumberFormatOptions {
            style: NumberFormatStyle::Unit,
            unit: Some(NumberFormatUnit::Bit),
            unit_display: NumberUnitDisplay::Long,
            ..Default::default()
        },
    )
    .unwrap();

    assert_eq!(afrikaans.format_f64(2.0).unwrap(), "2 bis");
    assert_eq!(
        afrikaans
            .format_to_parts_f64(2.0)
            .unwrap()
            .into_iter()
            .map(|part| (part.kind, part.value))
            .collect::<Vec<_>>(),
        vec![
            (NumberFormatPartKind::Integer, "2".into()),
            (NumberFormatPartKind::Literal, " ".into()),
            (NumberFormatPartKind::Unit, "bis".into()),
        ]
    );
}

#[test]
fn compound_unit_patterns_use_the_rounded_cldr_plural_category() {
    let unit = NumberFormatUnit::parse("kilometer-per-hour").unwrap();
    let russian = NumberFormat::try_new(
        &[canonicalize("ru").unwrap()],
        NumberFormatOptions {
            style: NumberFormatStyle::Unit,
            unit: Some(unit),
            unit_display: NumberUnitDisplay::Long,
            ..Default::default()
        },
    )
    .unwrap();
    assert_eq!(russian.format_f64(1.0).unwrap(), "1 километр в час");
    assert_eq!(russian.format_f64(2.0).unwrap(), "2 километра в час");
    assert_eq!(russian.format_f64(5.0).unwrap(), "5 километров в час");

    let french = NumberFormat::try_new(
        &[canonicalize("fr").unwrap()],
        NumberFormatOptions {
            style: NumberFormatStyle::Unit,
            unit: Some(unit),
            unit_display: NumberUnitDisplay::Short,
            ..Default::default()
        },
    )
    .unwrap();
    assert_eq!(french.format_f64(2.0).unwrap(), "2\u{202f}km/h");
}
