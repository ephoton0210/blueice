// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

//! Version-pinned, host-neutral locale data used by the ECMA-402 services.
//!
//! ICU4X's compiled data supplies the localized algorithms. This provider is
//! the single place where BlueIce declares the subset it exposes through
//! ECMA-402 and the deterministic fallbacks around that data. It deliberately
//! does not negotiate locales: `ResolveLocale` remains a separate layer.

use fixed_decimal::Decimal;
use icu_experimental::{
    dimension::{
        currency::CurrencyType,
        provider::{
            currency::fractions::CurrencyFractionsV1,
            percent::PercentEssentialsV1,
            units::{
                categorized_display_names::{
                    UnitsNamesAreaCoreV1, UnitsNamesAreaExtendedV1, UnitsNamesAreaOutlierV1,
                    UnitsNamesDurationCoreV1, UnitsNamesDurationExtendedV1,
                    UnitsNamesDurationOutlierV1, UnitsNamesLengthCoreV1,
                    UnitsNamesLengthExtendedV1, UnitsNamesLengthOutlierV1, UnitsNamesMassCoreV1,
                    UnitsNamesMassExtendedV1, UnitsNamesMassOutlierV1, UnitsNamesVolumeCoreV1,
                    UnitsNamesVolumeExtendedV1, UnitsNamesVolumeOutlierV1,
                },
                display_names::UnitsDisplayNames,
            },
        },
    },
    provider::Baked as ExperimentalData,
};
use icu_locale_core::Locale as IcuLocale;
use icu_plurals::{
    PluralCategory as IcuPluralCategory, PluralRules as IcuPluralRules,
    PluralRulesWithRanges as IcuPluralRulesWithRanges,
};
use icu_provider::{
    DataIdentifierBorrowed, DataMarker, DataMarkerAttributes, DataProvider, DataRequest,
};
use writeable::Writeable;

mod compact_patterns;
mod currency_patterns;
mod decimal_symbols;
mod range_patterns;
mod unit_patterns;

pub(crate) use decimal_symbols::NumberDecimalSymbols;

/// Immutable source revision for the ICU4X data bundle consumed by this crate.
///
/// This is a source revision, rather than a claimed upstream CLDR release:
/// the compatibility fork may contain narrowly scoped integration fixes while
/// retaining the upstream baked-data layout. Consumers that persist or
/// compare Intl output can record this value with their corpus revision.
pub const ICU4X_LOCALE_DATA_REVISION: &str = "31dcf42731d45cb191cdbd5bb92b669b5be12b57";

/// The immutable data inputs used by a BlueIce ECMA-402 build.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct LocaleDataRevisions {
    /// The pinned ICU4X source revision containing compiled CLDR data.
    pub icu4x: &'static str,
    /// The IANA TZDB release compiled into the Jiff bundle.
    pub tzdb: &'static str,
}

/// Calendar identifiers backed by `Intl.DateTimeFormat`.
pub const SUPPORTED_CALENDARS: &[&str] = &[
    "buddhist",
    "chinese",
    "coptic",
    "dangi",
    "ethioaa",
    "ethiopic",
    "gregory",
    "hebrew",
    "indian",
    "islamic-civil",
    "islamic-tbla",
    "islamic-umalqura",
    "iso8601",
    "japanese",
    "persian",
    "roc",
];

/// Decimal numbering systems backed by the pinned CLDR simple-digit dataset
/// used by date, number, and relative-time formatting.
///
/// ICU4X's compact baked decimal payload is intentionally selective, so the
/// provider completes it from the same pinned CLDR source before advertising
/// the ECMA-402 simple-digit registry. The query in
/// [`LocaleDataProvider::decimal_digits`] is the source of truth for the
/// characters used by NumberFormat and DurationFormat.
pub const SUPPORTED_NUMBERING_SYSTEMS: &[&str] = &[
    "adlm", "ahom", "arab", "arabext", "bali", "beng", "bhks", "brah", "cakm", "cham", "deva",
    "diak", "fullwide", "gara", "gong", "gonm", "gujr", "gukh", "guru", "hanidec", "hmng", "hmnp",
    "java", "kali", "kawi", "khmr", "knda", "krai", "lana", "lanatham", "laoo", "latn", "lepc",
    "limb", "mathbold", "mathdbl", "mathmono", "mathsanb", "mathsans", "mlym", "modi", "mong",
    "mroo", "mtei", "mymr", "mymrepka", "mymrpao", "mymrshan", "mymrtlng", "nagm", "newa", "nkoo",
    "olck", "onao", "orya", "osma", "outlined", "rohg", "saur", "segment", "shrd", "sind", "sinh",
    "sora", "sund", "sunu", "takr", "talu", "tamldec", "telu", "thai", "tibt", "tirh", "tnsa",
    "tols", "vaii", "wara", "wcho",
];

/// Language-level locales advertised by the NumberFormat provider.
///
/// ICU4X compacts locale records through their language parents, so this
/// inventory intentionally contains language identifiers rather than every
/// language/script/region spelling that resolves through one. Cantonese is a
/// NumberFormat-only addition: its pinned decimal and raw unit data are
/// present even though the compact language registry omits it.
const NUMBER_FORMAT_LOCALES: &[&str] = &[
    "af", "am", "ar", "as", "az", "be", "bg", "bn", "bo", "br", "bs", "ca", "ceb", "chr", "cs",
    "cy", "da", "de", "dsb", "dz", "ee", "el", "en", "eo", "es", "et", "fa", "ff", "fi", "fil",
    "fo", "fr", "fy", "ga", "gl", "gu", "gv", "ha", "haw", "he", "hi", "hr", "hsb", "hu", "hy",
    "id", "ig", "is", "it", "ja", "ka", "kk", "kl", "km", "kn", "ko", "kok", "ku", "ky", "la",
    "lb", "lkt", "ln", "lo", "lt", "lv", "mk", "ml", "mn", "mr", "ms", "mt", "my", "nb", "ne",
    "nl", "nn", "no", "om", "or", "pa", "pl", "ps", "pt", "ro", "ru", "sa", "se", "si", "sk", "sl",
    "so", "sq", "sr", "sv", "sw", "ta", "te", "th", "tk", "to", "tr", "ug", "uk", "ur", "uz", "vi",
    "wae", "wo", "xh", "yi", "yo", "yue", "zh", "zu",
];

/// Collation types that can resolve through `Intl.Collator` for at least one
/// bundled locale.
pub const SUPPORTED_COLLATIONS: &[&str] = &[
    "compat", "dict", "emoji", "eor", "phonebk", "pinyin", "searchjl", "stroke", "trad", "unihan",
    "zhuyin",
];

/// ISO 4217 codes with localized `Intl.DisplayNames` data.
pub const SUPPORTED_CURRENCIES: &[&str] = &["EUR", "JPY", "USD"];

/// Sanctioned simple units whose NumberFormat patterns are implemented.
pub const SUPPORTED_UNITS: &[&str] = &[
    "acre",
    "bit",
    "byte",
    "celsius",
    "centimeter",
    "day",
    "degree",
    "fahrenheit",
    "fluid-ounce",
    "foot",
    "gallon",
    "gigabit",
    "gigabyte",
    "gram",
    "hectare",
    "hour",
    "inch",
    "kilobit",
    "kilobyte",
    "kilogram",
    "kilometer",
    "liter",
    "megabit",
    "megabyte",
    "meter",
    "microsecond",
    "mile",
    "mile-scandinavian",
    "milliliter",
    "millimeter",
    "millisecond",
    "minute",
    "month",
    "nanosecond",
    "ounce",
    "percent",
    "petabyte",
    "pound",
    "second",
    "stone",
    "terabit",
    "terabyte",
    "week",
    "yard",
    "year",
];

/// A capability represented by the bundled locale data.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum LocaleDataCategory {
    /// The DateTimeFormat calendar registry.
    Calendar,
    /// The Collator collation-type registry.
    Collation,
    /// The DisplayNames currency registry.
    Currency,
    /// The decimal-digit-system registry.
    NumberingSystem,
    /// The NumberFormat unit registry.
    Unit,
}

/// An Intl service whose data coverage is declared by this provider.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum IntlService {
    /// `Intl.Collator` tailoring data.
    Collator,
    /// `Intl.DateTimeFormat` calendars, patterns, and time zones.
    DateTimeFormat,
    /// `Intl.DisplayNames` localized code names.
    DisplayNames,
    /// `Intl.DurationFormat` unit patterns.
    DurationFormat,
    /// `Intl.ListFormat` list patterns.
    ListFormat,
    /// `Intl.NumberFormat` decimal symbols and number patterns.
    NumberFormat,
    /// `Intl.PluralRules` plural categories.
    PluralRules,
    /// `Intl.RelativeTimeFormat` relative-time patterns.
    RelativeTimeFormat,
    /// `Intl.Segmenter` break rules.
    Segmenter,
}

/// Explicit capabilities for one service's data bundle.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct LocaleDataCapabilities {
    /// Static `Intl.supportedValuesOf` registries consumed by the service.
    pub value_categories: &'static [LocaleDataCategory],
    /// Whether the service consumes the pinned IANA TZDB registry.
    pub uses_time_zone_data: bool,
}

/// One count within a NumberFormat provider-coverage inventory.
///
/// `data_backed` means the selected record came from the pinned ICU4X/CLDR
/// provider or one of this provider's pinned raw CLDR supplements. It does
/// not count an English compatibility fallback as localized coverage.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub struct NumberFormatCoverageCount {
    /// Number of matrix cells whose selected result is provider-backed.
    pub data_backed: usize,
    /// Number of matrix cells evaluated.
    pub total: usize,
}

impl NumberFormatCoverageCount {
    /// Returns the integer percentage of data-backed matrix cells.
    pub const fn percentage(self) -> usize {
        match self.data_backed.saturating_mul(100).checked_div(self.total) {
            Some(percentage) => percentage,
            None => 0,
        }
    }
}

/// Data provenance coverage for one NumberFormat language locale.
///
/// This is deliberately an inventory, not an observable Intl API. It gives
/// tests and release tooling a stable way to detect when a newly advertised
/// locale would otherwise reach a bounded compatibility fallback.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub struct NumberFormatProviderCoverage {
    /// Whether decimal symbols are loaded from this locale's provider data.
    pub decimal_symbols: bool,
    /// Scientific separator/minus records for every supported numbering
    /// system after the same locale-default resolution as decimal symbols.
    pub scientific_symbols: NumberFormatCoverageCount,
    /// Every pinned CLDR compact plural/magnitude/numbering-system record.
    pub compact_patterns: NumberFormatCoverageCount,
    /// Every currency code, display mode, pattern/sign form, plural-name
    /// form, and supported numbering-system selection.
    pub currency_patterns: NumberFormatCoverageCount,
    /// Unsigned and signed percent-pattern lookups.
    pub percent_patterns: NumberFormatCoverageCount,
    /// Every simple unit, display width and reachable plural category.
    pub simple_unit_patterns: NumberFormatCoverageCount,
    /// Every sanctioned denominator and display width. Each data-backed cell
    /// composes every sanctioned numerator from its localized simple pattern.
    pub generic_compound_patterns: NumberFormatCoverageCount,
}

/// The centralized locale-data provider.
///
/// The type has no mutable state: the source data is compiled into the pinned
/// ICU4X dependency and the small ECMA-402 exposure registry is static.
#[derive(Clone, Copy, Debug, Default)]
pub struct LocaleDataProvider;

/// Data-owned placement and labels for a compound NumberFormat unit pattern.
///
/// The fields intentionally preserve part boundaries: `prefix` and `suffix`
/// become `unit` parts while the separators become `literal` parts.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) struct NumberCompoundUnitPattern {
    pub(crate) prefix: &'static str,
    pub(crate) prefix_separator: &'static str,
    pub(crate) suffix_separator: &'static str,
    pub(crate) suffix: &'static str,
}

/// A provider-owned generic compound-unit shape.
///
/// When ICU4X supplies both simple display names and a `per` composition
/// record this is the fully localized result. Otherwise it is the bounded
/// provider fallback. The owned fields preserve the observable `unit` and
/// `literal` boundaries while permitting a CLDR composition to occur before
/// or after the formatted number.
#[derive(Clone, Debug, Eq, PartialEq)]
pub(crate) struct NumberGenericCompoundUnitPattern {
    pub(crate) prefix: String,
    pub(crate) prefix_separator: String,
    pub(crate) suffix_separator: String,
    pub(crate) suffix: String,
}

/// A localized NumberFormat simple-unit affix split along its observable
/// `unit` and `literal` part boundaries.
#[derive(Clone, Debug, Eq, PartialEq)]
pub(crate) struct NumberUnitPattern {
    pub(crate) prefix: String,
    pub(crate) prefix_separator: String,
    pub(crate) suffix_separator: String,
    pub(crate) suffix: String,
    /// Whether this CLDR unit pattern deliberately omits its number
    /// placeholder (for example Arabic long singular `متر`).
    pub(crate) hides_number: bool,
}

/// The non-numeric portions of a localized currency pattern, split at the
/// number placeholder so NumberFormat can retain the decimal formatter's
/// existing sign and digit parts.
#[derive(Clone, Debug, Eq, PartialEq)]
pub(crate) enum NumberCurrencyPatternPiece {
    Literal(String),
    Currency(String),
    /// A sign encoded by an explicit CLDR negative subpattern.
    Sign,
}

/// A localized CLDR currency pattern split around its number placeholder.
#[derive(Clone, Debug, Eq, PartialEq)]
pub(crate) struct NumberCurrencyPattern {
    pub(crate) before_number: Vec<NumberCurrencyPatternPiece>,
    pub(crate) after_number: Vec<NumberCurrencyPatternPiece>,
    /// Whether the selected CLDR pattern supplied the negative presentation,
    /// so NumberFormat must not retain its decimal formatter minus part.
    pub(crate) consumes_decimal_sign: bool,
}

/// Currency-specific choices that jointly select one pinned CLDR pattern.
pub(crate) struct NumberCurrencyPatternRequest<'a> {
    pub(crate) numbering_system: &'a str,
    pub(crate) code: &'a str,
    pub(crate) display: crate::NumberCurrencyDisplay,
    pub(crate) accounting: bool,
    pub(crate) negative: bool,
    pub(crate) plural: crate::PluralCategory,
}

/// The two provider-owned connectors used by a formatted numeric range.
///
/// A range that collapses a shared affix or sign uses `collapsed_separator`
/// inside one localized pattern. A range whose endpoint affixes must both
/// remain visible uses `uncollapsed_separator` between two complete endpoint
/// patterns. Keeping both forms in the provider prevents NumberFormat from
/// guessing punctuation from the fraction width.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) struct NumberRangePattern {
    pub(crate) collapsed_separator: &'static str,
    pub(crate) uncollapsed_separator: &'static str,
}

/// The non-numeric portions of an ICU percent pattern. A `Sign` item keeps
/// the decimal formatter's existing typed plus/minus part while allowing
/// CLDR to position it relative to the percent sign.
#[derive(Clone, Debug, Eq, PartialEq)]
pub(crate) enum NumberPercentPatternPiece {
    Literal(String),
    PercentSign(String),
    Sign,
}

/// A localized CLDR percent pattern split around its number placeholder.
#[derive(Clone, Debug, Eq, PartialEq)]
pub(crate) struct NumberPercentPattern {
    pub(crate) before_number: Vec<NumberPercentPatternPiece>,
    pub(crate) after_number: Vec<NumberPercentPatternPiece>,
}

/// CLDR symbols used only by the scientific/engineering rendering branch.
///
/// ICU4X's decimal payload intentionally retains ordinary decimal signs and
/// separators but does not expose the CLDR `exponential` symbol. Keeping the
/// missing record beside the other NumberFormat provider data prevents the
/// host-neutral formatter from manufacturing ASCII `E-` for every locale.
#[derive(Clone, Debug, Eq, PartialEq)]
pub(crate) struct NumberScientificSymbols {
    pub(crate) exponent_separator: String,
    pub(crate) exponent_minus_prefix: String,
    pub(crate) exponent_minus_sign: String,
    pub(crate) exponent_minus_suffix: String,
}

impl NumberScientificSymbols {
    /// Splits CLDR's complete negative-sign string into the ECMA-402
    /// exponent-minus part and directional literal boundaries. CLDR carries
    /// those controls in `minusSign`; retaining a trailing control is needed
    /// for shapes such as Pashto `\u{200e}-\u{200e}` before exponent digits.
    pub(crate) fn from_cldr(exponent_separator: String, exponent_minus_sign: String) -> Self {
        let prefix_length = exponent_minus_sign
            .chars()
            .take_while(|character| is_bidi_control(*character))
            .map(char::len_utf8)
            .sum::<usize>();
        let suffix_length = exponent_minus_sign
            .chars()
            .rev()
            .take_while(|character| is_bidi_control(*character))
            .map(char::len_utf8)
            .sum::<usize>();
        let sign_end = exponent_minus_sign.len().saturating_sub(suffix_length);
        let (prefix, sign, suffix) = if prefix_length < sign_end {
            (
                exponent_minus_sign[..prefix_length].into(),
                exponent_minus_sign[prefix_length..sign_end].into(),
                exponent_minus_sign[sign_end..].into(),
            )
        } else {
            (String::new(), exponent_minus_sign, String::new())
        };
        Self {
            exponent_separator,
            exponent_minus_prefix: prefix,
            exponent_minus_sign: sign,
            exponent_minus_suffix: suffix,
        }
    }
}

const fn is_bidi_control(character: char) -> bool {
    matches!(
        character,
        '\u{61c}' | '\u{200e}' | '\u{200f}' | '\u{202a}'..='\u{202e}' | '\u{2066}'..='\u{2069}'
    )
}

/// A data-owned compact decimal scaling pattern.
#[derive(Clone, Debug, Eq, PartialEq)]
pub(crate) struct CompactNumberPattern {
    /// Decimal power removed from the source magnitude before formatting.
    pub(crate) divisor: i16,
    /// Directional controls before a leading compact label.
    pub(crate) prefix_leading_literal: String,
    /// Leading compact label before the formatted magnitude.
    pub(crate) prefix: String,
    /// Directional controls after a leading compact label.
    pub(crate) prefix_trailing_literal: String,
    /// Literal text between a leading compact label and the magnitude.
    pub(crate) prefix_separator: String,
    /// Literal text between the magnitude and a trailing compact label.
    pub(crate) suffix_separator: String,
    /// The compact suffix as an ECMA-402 `compact` part.
    pub(crate) suffix: String,
    /// Directional controls after a trailing compact label.
    pub(crate) suffix_trailing_literal: String,
    /// Whether this CLDR pattern intentionally omits the numeric placeholder.
    ///
    /// Exact-value patterns such as French long `mille` carry the entire
    /// compact display in `suffix`; the NumberFormat core keeps a sign, but
    /// suppresses its ordinary digit parts before appending this record.
    pub(crate) hides_number: bool,
}

/// Returns the process-wide provider for every host-neutral Intl service.
pub const fn locale_data_provider() -> LocaleDataProvider {
    LocaleDataProvider
}

/// Splits a rendered pinned-CLDR currency pattern around the two fixed
/// placeholders. Keeping the split here preserves its CLDR literals as
/// `formatToParts()` literal records instead of flattening them into a
/// service-local prefix/suffix rule.
fn split_number_currency_pattern(
    rendered: &str,
    currency: String,
    consumes_decimal_sign: bool,
) -> Option<NumberCurrencyPattern> {
    const NUMBER: &str = "\u{fdd0}";
    const CURRENCY: &str = "\u{fdd1}";

    let number_index = rendered.find(NUMBER)?;
    let currency_index = rendered.find(CURRENCY)?;
    if number_index == currency_index {
        return None;
    }

    let mut before_number = Vec::new();
    let mut after_number = Vec::new();
    if currency_index < number_index {
        push_currency_literal(&mut before_number, &rendered[..currency_index]);
        before_number.push(NumberCurrencyPatternPiece::Currency(currency));
        push_currency_literal(
            &mut before_number,
            &rendered[currency_index + CURRENCY.len()..number_index],
        );
        push_currency_literal(&mut after_number, &rendered[number_index + NUMBER.len()..]);
    } else {
        push_currency_literal(&mut before_number, &rendered[..number_index]);
        push_currency_literal(
            &mut after_number,
            &rendered[number_index + NUMBER.len()..currency_index],
        );
        after_number.push(NumberCurrencyPatternPiece::Currency(currency));
        push_currency_literal(
            &mut after_number,
            &rendered[currency_index + CURRENCY.len()..],
        );
    }
    Some(NumberCurrencyPattern {
        before_number,
        after_number,
        consumes_decimal_sign,
    })
}

/// Replaces CLDR's numeric skeleton and currency sign with fixed internal
/// placeholders before the currency pattern is split into typed parts.
fn currency_pattern_with_placeholders(raw: &str) -> Option<String> {
    let first = raw.find(['0', '#'])?;
    let mut end = first;
    for (offset, character) in raw[first..].char_indices() {
        if matches!(character, '0' | '#' | ',' | '.') {
            end = first + offset + character.len_utf8();
        } else {
            break;
        }
    }
    let mut rendered = raw.to_owned();
    rendered.replace_range(first..end, "﷐");
    let currency = rendered.find('¤')?;
    rendered.replace_range(currency..currency + '¤'.len_utf8(), "﷑");
    (!rendered.contains('¤')).then_some(rendered)
}

fn push_currency_literal(pieces: &mut Vec<NumberCurrencyPatternPiece>, literal: &str) {
    let mut literal_start = 0;
    for (index, character) in literal.char_indices() {
        if matches!(character, '-' | '\u{2212}') {
            if literal_start < index {
                pieces.push(NumberCurrencyPatternPiece::Literal(
                    literal[literal_start..index].into(),
                ));
            }
            pieces.push(NumberCurrencyPatternPiece::Sign);
            literal_start = index + character.len_utf8();
        }
    }
    if literal_start < literal.len() {
        pieces.push(NumberCurrencyPatternPiece::Literal(
            literal[literal_start..].into(),
        ));
    }
}

/// Splits a rendered ICU percent pattern around its number and optional sign
/// placeholders, preserving the localized percent sign as its own part.
fn split_number_percent_pattern(rendered: &str) -> Option<NumberPercentPattern> {
    const NUMBER: &str = "\u{fdd0}";
    const SIGN: &str = "\u{fdd1}";

    let mut pieces = Vec::new();
    let mut remaining = rendered;
    while !remaining.is_empty() {
        let number_index = remaining.find(NUMBER);
        let sign_index = remaining.find(SIGN);
        let percent_index = remaining.char_indices().find_map(|(index, character)| {
            matches!(character, '%' | '\u{066a}' | '\u{fe6a}' | '\u{ff05}').then_some(index)
        });
        let Some(next) = [number_index, sign_index, percent_index]
            .into_iter()
            .flatten()
            .min()
        else {
            push_percent_literal(&mut pieces, remaining);
            break;
        };
        push_percent_literal(&mut pieces, &remaining[..next]);
        if number_index == Some(next) {
            pieces.push(None);
            remaining = &remaining[next + NUMBER.len()..];
        } else if sign_index == Some(next) {
            pieces.push(Some(NumberPercentPatternPiece::Sign));
            remaining = &remaining[next + SIGN.len()..];
        } else {
            let percent = remaining[next..]
                .chars()
                .next()
                .expect("percent index is a character boundary");
            pieces.push(Some(NumberPercentPatternPiece::PercentSign(percent.into())));
            remaining = &remaining[next + percent.len_utf8()..];
        }
    }
    let number_index = pieces.iter().position(Option::is_none)?;
    if pieces[number_index + 1..].iter().any(Option::is_none) {
        return None;
    }
    let before_number = pieces[..number_index]
        .iter()
        .filter_map(Clone::clone)
        .collect();
    let after_number = pieces[number_index + 1..]
        .iter()
        .filter_map(Clone::clone)
        .collect();
    Some(NumberPercentPattern {
        before_number,
        after_number,
    })
}

fn push_percent_literal(pieces: &mut Vec<Option<NumberPercentPatternPiece>>, literal: &str) {
    if !literal.is_empty() {
        pieces.push(Some(NumberPercentPatternPiece::Literal(literal.into())));
    }
}

/// Turns CLDR's required ten-code-point simple digit sequence into the
/// provider payload shape used by ICU4X.
fn decimal_digit_array(digits: &str) -> Option<[char; 10]> {
    let mut chars = digits.chars();
    let result = [
        chars.next()?,
        chars.next()?,
        chars.next()?,
        chars.next()?,
        chars.next()?,
        chars.next()?,
        chars.next()?,
        chars.next()?,
        chars.next()?,
        chars.next()?,
    ];
    chars.next().is_none().then_some(result)
}

/// The complete simple-digit data from the pinned CLDR source. ICU4X's baked
/// decimal component intentionally carries only a compact subset; this
/// provider-owned completion record preserves ECMA-402's Table 4 capability
/// without making NumberFormat invent a second digit table.
fn cldr_simple_decimal_digits(numbering_system: &str) -> Option<&'static str> {
    Some(match numbering_system {
        "adlm" => "𞥐𞥑𞥒𞥓𞥔𞥕𞥖𞥗𞥘𞥙",
        "ahom" => "𑜰𑜱𑜲𑜳𑜴𑜵𑜶𑜷𑜸𑜹",
        "arab" => "٠١٢٣٤٥٦٧٨٩",
        "arabext" => "۰۱۲۳۴۵۶۷۸۹",
        "bali" => "᭐᭑᭒᭓᭔᭕᭖᭗᭘᭙",
        "beng" => "০১২৩৪৫৬৭৮৯",
        "bhks" => "𑱐𑱑𑱒𑱓𑱔𑱕𑱖𑱗𑱘𑱙",
        "brah" => "𑁦𑁧𑁨𑁩𑁪𑁫𑁬𑁭𑁮𑁯",
        "cakm" => "𑄶𑄷𑄸𑄹𑄺𑄻𑄼𑄽𑄾𑄿",
        "cham" => "꩐꩑꩒꩓꩔꩕꩖꩗꩘꩙",
        "deva" => "०१२३४५६७८९",
        "diak" => "𑥐𑥑𑥒𑥓𑥔𑥕𑥖𑥗𑥘𑥙",
        "fullwide" => "０１２３４５６７８９",
        "gara" => "𐵀𐵁𐵂𐵃𐵄𐵅𐵆𐵇𐵈𐵉",
        "gong" => "𑶠𑶡𑶢𑶣𑶤𑶥𑶦𑶧𑶨𑶩",
        "gonm" => "𑵐𑵑𑵒𑵓𑵔𑵕𑵖𑵗𑵘𑵙",
        "gujr" => "૦૧૨૩૪૫૬૭૮૯",
        "gukh" => "𖄰𖄱𖄲𖄳𖄴𖄵𖄶𖄷𖄸𖄹",
        "guru" => "੦੧੨੩੪੫੬੭੮੯",
        "hanidec" => "〇一二三四五六七八九",
        "hmng" => "𖭐𖭑𖭒𖭓𖭔𖭕𖭖𖭗𖭘𖭙",
        "hmnp" => "𞅀𞅁𞅂𞅃𞅄𞅅𞅆𞅇𞅈𞅉",
        "java" => "꧐꧑꧒꧓꧔꧕꧖꧗꧘꧙",
        "kali" => "꤀꤁꤂꤃꤄꤅꤆꤇꤈꤉",
        "kawi" => "𑽐𑽑𑽒𑽓𑽔𑽕𑽖𑽗𑽘𑽙",
        "khmr" => "០១២៣៤៥៦៧៨៩",
        "knda" => "೦೧೨೩೪೫೬೭೮೯",
        "krai" => "𖵰𖵱𖵲𖵳𖵴𖵵𖵶𖵷𖵸𖵹",
        "lana" => "᪀᪁᪂᪃᪄᪅᪆᪇᪈᪉",
        "lanatham" => "᪐᪑᪒᪓᪔᪕᪖᪗᪘᪙",
        "laoo" => "໐໑໒໓໔໕໖໗໘໙",
        "latn" => "0123456789",
        "lepc" => "᱀᱁᱂᱃᱄᱅᱆᱇᱈᱉",
        "limb" => "᥆᥇᥈᥉᥊᥋᥌᥍᥎᥏",
        "mathbold" => "𝟎𝟏𝟐𝟑𝟒𝟓𝟔𝟕𝟖𝟗",
        "mathdbl" => "𝟘𝟙𝟚𝟛𝟜𝟝𝟞𝟟𝟠𝟡",
        "mathmono" => "𝟶𝟷𝟸𝟹𝟺𝟻𝟼𝟽𝟾𝟿",
        "mathsanb" => "𝟬𝟭𝟮𝟯𝟰𝟱𝟲𝟳𝟴𝟵",
        "mathsans" => "𝟢𝟣𝟤𝟥𝟦𝟧𝟨𝟩𝟪𝟫",
        "mlym" => "൦൧൨൩൪൫൬൭൮൯",
        "modi" => "𑙐𑙑𑙒𑙓𑙔𑙕𑙖𑙗𑙘𑙙",
        "mong" => "᠐᠑᠒᠓᠔᠕᠖᠗᠘᠙",
        "mroo" => "𖩠𖩡𖩢𖩣𖩤𖩥𖩦𖩧𖩨𖩩",
        "mtei" => "꯰꯱꯲꯳꯴꯵꯶꯷꯸꯹",
        "mymr" => "၀၁၂၃၄၅၆၇၈၉",
        "mymrepka" => "𑛚𑛛𑛜𑛝𑛞𑛟𑛠𑛡𑛢𑛣",
        "mymrpao" => "𑛐𑛑𑛒𑛓𑛔𑛕𑛖𑛗𑛘𑛙",
        "mymrshan" => "႐႑႒႓႔႕႖႗႘႙",
        "mymrtlng" => "꧰꧱꧲꧳꧴꧵꧶꧷꧸꧹",
        "nagm" => "𞓰𞓱𞓲𞓳𞓴𞓵𞓶𞓷𞓸𞓹",
        "newa" => "𑑐𑑑𑑒𑑓𑑔𑑕𑑖𑑗𑑘𑑙",
        "nkoo" => "߀߁߂߃߄߅߆߇߈߉",
        "olck" => "᱐᱑᱒᱓᱔᱕᱖᱗᱘᱙",
        "onao" => "𞗱𞗲𞗳𞗴𞗵𞗶𞗷𞗸𞗹𞗺",
        "orya" => "୦୧୨୩୪୫୬୭୮୯",
        "osma" => "𐒠𐒡𐒢𐒣𐒤𐒥𐒦𐒧𐒨𐒩",
        "outlined" => "𜳰𜳱𜳲𜳳𜳴𜳵𜳶𜳷𜳸𜳹",
        "rohg" => "𐴰𐴱𐴲𐴳𐴴𐴵𐴶𐴷𐴸𐴹",
        "saur" => "꣐꣑꣒꣓꣔꣕꣖꣗꣘꣙",
        "segment" => "🯰🯱🯲🯳🯴🯵🯶🯷🯸🯹",
        "shrd" => "𑇐𑇑𑇒𑇓𑇔𑇕𑇖𑇗𑇘𑇙",
        "sind" => "𑋰𑋱𑋲𑋳𑋴𑋵𑋶𑋷𑋸𑋹",
        "sinh" => "෦෧෨෩෪෫෬෭෮෯",
        "sora" => "𑃰𑃱𑃲𑃳𑃴𑃵𑃶𑃷𑃸𑃹",
        "sund" => "᮰᮱᮲᮳᮴᮵᮶᮷᮸᮹",
        "sunu" => "𑯰𑯱𑯲𑯳𑯴𑯵𑯶𑯷𑯸𑯹",
        "takr" => "𑛀𑛁𑛂𑛃𑛄𑛅𑛆𑛇𑛈𑛉",
        "talu" => "᧐᧑᧒᧓᧔᧕᧖᧗᧘᧙",
        "tamldec" => "௦௧௨௩௪௫௬௭௮௯",
        "telu" => "౦౧౨౩౪౫౬౭౮౯",
        "thai" => "๐๑๒๓๔๕๖๗๘๙",
        "tibt" => "༠༡༢༣༤༥༦༧༨༩",
        "tirh" => "𑓐𑓑𑓒𑓓𑓔𑓕𑓖𑓗𑓘𑓙",
        "tnsa" => "𖫀𖫁𖫂𖫃𖫄𖫅𖫆𖫇𖫈𖫉",
        "tols" => "𑷠𑷡𑷢𑷣𑷤𑷥𑷦𑷧𑷨𑷩",
        "vaii" => "꘠꘡꘢꘣꘤꘥꘦꘧꘨꘩",
        "wara" => "𑣠𑣡𑣢𑣣𑣤𑣥𑣦𑣧𑣨𑣩",
        "wcho" => "𞋰𞋱𞋲𞋳𞋴𞋵𞋶𞋷𞋸𞋹",
        _ => return None,
    })
}

impl LocaleDataProvider {
    /// Returns the immutable ICU4X source revision for this provider.
    pub const fn revision(self) -> &'static str {
        ICU4X_LOCALE_DATA_REVISION
    }

    /// Returns every versioned locale-data input used by this provider.
    pub fn revisions(self) -> LocaleDataRevisions {
        LocaleDataRevisions {
            icu4x: self.revision(),
            tzdb: jiff_tzdb::VERSION.unwrap_or("unknown"),
        }
    }

    /// Returns the IANA TZDB release used by DateTimeFormat zone data.
    pub fn tzdb_version(self) -> &'static str {
        self.revisions().tzdb
    }

    /// Returns the registry values supplied by a static data category.
    pub const fn values(self, category: LocaleDataCategory) -> &'static [&'static str] {
        match category {
            LocaleDataCategory::Calendar => SUPPORTED_CALENDARS,
            LocaleDataCategory::Collation => SUPPORTED_COLLATIONS,
            LocaleDataCategory::Currency => SUPPORTED_CURRENCIES,
            LocaleDataCategory::NumberingSystem => SUPPORTED_NUMBERING_SYSTEMS,
            LocaleDataCategory::Unit => SUPPORTED_UNITS,
        }
    }

    /// Returns the registry and TZDB capabilities available to an Intl service.
    pub const fn capabilities(self, service: IntlService) -> LocaleDataCapabilities {
        const NONE: &[LocaleDataCategory] = &[];
        const CALENDAR_AND_NUMBERING: &[LocaleDataCategory] = &[
            LocaleDataCategory::Calendar,
            LocaleDataCategory::NumberingSystem,
        ];
        const DISPLAY_NAMES: &[LocaleDataCategory] =
            &[LocaleDataCategory::Calendar, LocaleDataCategory::Currency];
        const NUMBER_FORMAT: &[LocaleDataCategory] = &[
            LocaleDataCategory::Currency,
            LocaleDataCategory::NumberingSystem,
            LocaleDataCategory::Unit,
        ];
        const NUMBERING: &[LocaleDataCategory] = &[LocaleDataCategory::NumberingSystem];
        const COLLATION: &[LocaleDataCategory] = &[LocaleDataCategory::Collation];
        let value_categories = match service {
            IntlService::Collator => COLLATION,
            IntlService::DateTimeFormat => CALENDAR_AND_NUMBERING,
            IntlService::DisplayNames => DISPLAY_NAMES,
            IntlService::DurationFormat | IntlService::RelativeTimeFormat => NUMBERING,
            IntlService::NumberFormat => NUMBER_FORMAT,
            IntlService::ListFormat | IntlService::PluralRules | IntlService::Segmenter => NONE,
        };
        LocaleDataCapabilities {
            value_categories,
            uses_time_zone_data: matches!(service, IntlService::DateTimeFormat),
        }
    }

    /// Whether the compiled CLDR data has language-level fallback coverage.
    ///
    /// ICU4X compacts records whose values equal their parent locale. The
    /// provider therefore treats a supported primary language as data-backed
    /// even when the concrete marker resolves through that parent record.
    pub fn supports_language(self, locale: &IcuLocale) -> bool {
        NUMBER_FORMAT_LOCALES
            .iter()
            .any(|language| *language != "yue" && locale.id.language.as_str() == *language)
    }

    /// Returns the language-level inventory used by NumberFormat coverage
    /// tooling. Script and region variants resolve through these records or
    /// through their explicit raw CLDR supplement.
    pub const fn number_format_locales(self) -> &'static [&'static str] {
        NUMBER_FORMAT_LOCALES
    }

    /// Measures the NumberFormat provider's localized-data coverage for one
    /// advertised language locale.
    ///
    /// The unit denominator is intentionally every ECMA-402 sanctioned
    /// simple unit. Each locale contributes only plural categories that its
    /// pinned cardinal rules can actually select, preventing unreachable
    /// categories from diluting the rate.
    pub fn number_format_coverage(self, locale: &str) -> Option<NumberFormatProviderCoverage> {
        let locale = crate::canonicalize(locale).ok()?;
        if !self.supports_service_locale(crate::IntlService::NumberFormat, locale.locale()) {
            return None;
        }
        let locale_name = locale.as_str();
        let plural_categories = number_format_plural_categories(locale_name);
        let mut simple_unit_patterns = NumberFormatCoverageCount::default();
        for unit in crate::NumberFormatUnit::ALL {
            for display in [
                crate::NumberUnitDisplay::Long,
                crate::NumberUnitDisplay::Short,
                crate::NumberUnitDisplay::Narrow,
            ] {
                for plural in &plural_categories {
                    simple_unit_patterns.total += 1;
                    if self
                        .number_unit_pattern_with_provenance(locale_name, *unit, display, *plural)
                        .1
                    {
                        simple_unit_patterns.data_backed += 1;
                    }
                }
            }
        }
        let mut generic_compound_patterns = NumberFormatCoverageCount::default();
        let generic_cells_per_width = crate::NumberFormatUnit::ALL
            .len()
            .saturating_mul(crate::NumberFormatUnit::ALL.len())
            .saturating_mul(plural_categories.len());
        for display in [
            crate::NumberUnitDisplay::Long,
            crate::NumberUnitDisplay::Short,
            crate::NumberUnitDisplay::Narrow,
        ] {
            generic_compound_patterns.total += generic_cells_per_width;
            // A validated table record contains every numerator, denominator
            // and plural form. Generic composition never delegates to the
            // simple-unit compatibility path.
            if unit_patterns::has_cldr_full_generic_compound_data(
                locale_name,
                crate::NumberFormatUnit::ALL[0],
                display,
            ) {
                generic_compound_patterns.data_backed += generic_cells_per_width;
            }
        }
        let compact_patterns = compact_patterns::compact_coverage(locale_name);
        // Symbol, narrow-symbol, and code each select standard/accounting
        // positive/negative patterns (12 cells). Name selects all six CLDR
        // plural unit patterns. Every sanctioned numbering-system request
        // uses its direct currency record or the CLDR latn fallback.
        let currency_cells = currency_patterns::currency_code_count()
            .saturating_mul(SUPPORTED_NUMBERING_SYSTEMS.len())
            .saturating_mul(18);
        let currency_patterns = NumberFormatCoverageCount {
            data_backed: if currency_patterns::has_full_cldr_currency_data(locale_name) {
                currency_cells
            } else {
                0
            },
            total: currency_cells,
        };
        let percent_patterns = [
            self.number_percent_pattern(locale_name, false),
            self.number_percent_pattern(locale_name, true),
        ];
        let scientific_symbols = SUPPORTED_NUMBERING_SYSTEMS.iter().fold(
            NumberFormatCoverageCount::default(),
            |mut coverage, numbering_system| {
                coverage.total += 1;
                if self
                    .number_decimal_symbols(locale.locale(), numbering_system)
                    .is_some_and(|symbols| {
                        !symbols.exponent_separator.is_empty()
                            && !symbols.exponent_minus_sign.is_empty()
                    })
                {
                    coverage.data_backed += 1;
                }
                coverage
            },
        );
        Some(NumberFormatProviderCoverage {
            decimal_symbols: self.supports_decimal_locale(locale.locale()),
            scientific_symbols,
            compact_patterns,
            currency_patterns,
            percent_patterns: NumberFormatCoverageCount {
                data_backed: percent_patterns
                    .iter()
                    .filter(|pattern| pattern.is_some())
                    .count(),
                total: percent_patterns.len(),
            },
            simple_unit_patterns,
            generic_compound_patterns,
        })
    }

    /// Whether decimal-symbol data resolves to this locale's language.
    ///
    /// NumberFormat's advertised inventory must be backed by an explicit,
    /// pinned CLDR decimal record. ICU4X root data is deliberately not part
    /// of this capability check: returning root symbols for a supported
    /// language would make the result depend on a compact-data omission.
    pub fn supports_decimal_locale(self, locale: &IcuLocale) -> bool {
        decimal_symbols::default_numbering_system(&locale.to_string()).is_some()
    }

    /// Returns the fixed CLDR decimal symbols for a supported locale and
    /// resolved numbering system.
    pub(crate) fn number_decimal_symbols(
        self,
        locale: &IcuLocale,
        numbering_system: &str,
    ) -> Option<decimal_symbols::NumberDecimalSymbols> {
        decimal_symbols::decimal_symbols(&locale.to_string(), numbering_system)
    }

    /// Returns the ten pinned CLDR decimal digits for a sanctioned simple
    /// numbering system.
    ///
    /// The complete fixed dataset is the provider's source of truth. It does
    /// not delegate to ICU4X's partial baked marker and therefore cannot
    /// silently select a synthetic or root digit record.
    pub(crate) fn decimal_digits(self, numbering_system: &str) -> Option<[char; 10]> {
        cldr_simple_decimal_digits(numbering_system).and_then(decimal_digit_array)
    }

    /// Resolves the CLDR standard fraction precision for one ISO 4217 code.
    ///
    /// Currency fraction rules are global supplemental data, not a
    /// service-local list of exceptional currencies. A structurally invalid
    /// code is left to the embedding's ECMA-402 validation path.
    pub(crate) fn currency_fraction_digits(self, code: &str) -> Option<u8> {
        let currency = code.parse::<CurrencyType>().ok()?;
        <ExperimentalData as DataProvider<CurrencyFractionsV1>>::load(
            &ExperimentalData,
            DataRequest::default(),
        )
        .ok()
        .map(|response| response.payload.get().resolve(currency).digits)
    }

    /// Resolves the CLDR currency pattern around a decimal formatter's output.
    ///
    /// The result deliberately excludes the number placeholder. NumberFormat
    /// already owns sign-display and decimal part construction, while this
    /// provider owns every locale-sensitive currency literal, spacing and
    /// symbol position. Currency names dispatch to their separate
    /// plural-sensitive CLDR record family below.
    pub(crate) fn number_currency_pattern(
        self,
        locale: &str,
        request: NumberCurrencyPatternRequest<'_>,
    ) -> Option<NumberCurrencyPattern> {
        let (currency, starts_with_letter, ends_with_letter) = match request.display {
            crate::NumberCurrencyDisplay::Name => {
                let currency =
                    currency_patterns::currency_name(locale, request.code, request.plural);
                let rendered = currency_patterns::currency_name_pattern(
                    locale,
                    request.numbering_system,
                    request.plural,
                )?;
                let rendered = rendered.replace("{0}", "﷐").replace("{1}", "﷑");
                return split_number_currency_pattern(&rendered, currency, false);
            }
            crate::NumberCurrencyDisplay::Code => (request.code.to_owned(), true, true),
            crate::NumberCurrencyDisplay::Symbol | crate::NumberCurrencyDisplay::NarrowSymbol => {
                currency_patterns::currency_symbol(locale, request.code, request.display)
            }
        };
        let (raw_pattern, consumes_decimal_sign) = currency_patterns::currency_pattern(
            locale,
            request.numbering_system,
            request.accounting,
            starts_with_letter,
            ends_with_letter,
            request.negative,
        )?;
        let rendered = currency_pattern_with_placeholders(&raw_pattern)?;
        split_number_currency_pattern(&rendered, currency, consumes_decimal_sign)
    }

    /// Resolves the CLDR percent pattern for a number with or without an
    /// explicit sign. The sign itself remains a typed NumberFormat part; the
    /// pattern only controls its locale-sensitive placement.
    pub(crate) fn number_percent_pattern(
        self,
        locale: &str,
        signed: bool,
    ) -> Option<NumberPercentPattern> {
        let locale = crate::canonicalize(locale).ok()?;
        let data_locale = icu_locale_core::DataLocale::from(locale.locale().clone());
        let payload = <ExperimentalData as DataProvider<PercentEssentialsV1>>::load(
            &ExperimentalData,
            DataRequest {
                id: DataIdentifierBorrowed::for_locale(&data_locale),
                metadata: Default::default(),
            },
        )
        .ok()?
        .payload;
        let rendered = if signed {
            payload
                .get()
                .signed_pattern
                .interpolate(["\u{fdd0}", "\u{fdd1}"])
                .write_to_string()
                .into_owned()
        } else {
            payload
                .get()
                .unsigned_pattern
                .interpolate(["\u{fdd0}"])
                .write_to_string()
                .into_owned()
        };
        split_number_percent_pattern(&rendered)
    }

    /// Returns the CLDR approximately sign shared by all NumberFormat styles
    /// when an equal rounded range is rendered. ICU4X exposes this symbol on
    /// its percent essentials payload because it originates in the same CLDR
    /// decimal-symbol record; it is not percent-specific in ECMA-402.
    pub(crate) fn number_approximately_sign(self, locale: &str) -> Option<String> {
        let locale = crate::canonicalize(locale).ok()?;
        let data_locale = icu_locale_core::DataLocale::from(locale.locale().clone());
        <ExperimentalData as DataProvider<PercentEssentialsV1>>::load(
            &ExperimentalData,
            DataRequest {
                id: DataIdentifierBorrowed::for_locale(&data_locale),
                metadata: Default::default(),
            },
        )
        .ok()
        .map(|response| response.payload.get().approximately_sign.to_string())
    }

    /// Whether a service has data coverage for this locale.
    ///
    /// This is data availability only. Locale-list ordering, lookup, best-fit,
    /// Unicode-extension handling, and option precedence remain the future
    /// shared `ResolveLocale` layer.
    pub fn supports_service_locale(self, service: IntlService, locale: &IcuLocale) -> bool {
        match service {
            IntlService::NumberFormat => self.supports_decimal_locale(locale),
            _ => self.supports_language(locale),
        }
    }

    /// Selects provider data for a requested locale under one ECMA-402
    /// matching policy.
    ///
    /// Lookup preserves a directly available language-parent request. Best
    /// fit first takes that same exact path, then consults the pinned CLDR
    /// likely-subtag data. This gives structurally valid `und` requests a
    /// data-backed language/region match without inventing a service-local
    /// fallback table. The shared resolver decides request-list ordering and
    /// observable Unicode-key retention around this provider decision.
    pub(crate) fn match_service_locale(
        self,
        service: IntlService,
        locale: &IcuLocale,
        matcher: crate::LocaleMatcher,
    ) -> Option<IcuLocale> {
        if self.supports_service_locale(service, locale) {
            return Some(locale.clone());
        }
        if matcher != crate::LocaleMatcher::BestFit {
            return None;
        }
        let maximal = self.maximize_likely_subtags(locale);
        self.supports_service_locale(service, &maximal)
            .then(|| self.minimize_likely_subtags(&maximal))
    }

    /// Applies the pinned likely-subtag data to an ICU locale.
    ///
    /// ICU4X 2.3 intentionally leaves an `und` primary language unchanged in
    /// this API. Current ECMA-402/UTS 35 requires the language selected by
    /// likely-subtags to become observable, so the provider supplies the
    /// missing CLDR 44+ undefined-language records after ICU's expansion.
    pub(crate) fn maximize_likely_subtags(self, locale: &IcuLocale) -> IcuLocale {
        let mut maximal = locale.clone();
        icu_locale::LocaleExpander::new_extended().maximize(&mut maximal.id);
        self.patch_undefined_likely_subtags(&mut maximal);
        maximal
    }

    /// Removes likely subtags using the same provider data used for expansion.
    pub(crate) fn minimize_likely_subtags(self, locale: &IcuLocale) -> IcuLocale {
        let mut minimal = self.maximize_likely_subtags(locale);
        icu_locale::LocaleExpander::new_extended().minimize(&mut minimal.id);
        self.patch_minimal_likely_subtags(&mut minimal);
        minimal
    }

    fn patch_undefined_likely_subtags(self, locale: &mut IcuLocale) {
        if locale.id.language.as_str() != "und" {
            return;
        }
        let script = locale.id.script.map(|script| script.to_string());
        let region = locale.id.region.map(|region| region.to_string());
        let (language, default_script, default_region) =
            match (script.as_deref(), region.as_deref()) {
                (Some("Thai"), _) => ("th", "Thai", "TH"),
                (Some("Cyrl"), Some("RO")) => ("bg", "Cyrl", "RO"),
                (_, Some("419")) => ("es", "Latn", "419"),
                (_, Some("AT")) => ("de", "Latn", "AT"),
                (_, Some("CW")) => ("pap", "Latn", "CW"),
                (_, Some("150")) => ("en", "Latn", "150"),
                (_, Some("AQ")) => ("en", "Latn", "AQ"),
                (_, Some("US")) => ("en", "Latn", "US"),
                _ => ("en", "Latn", "US"),
            };
        locale.id.language = language.parse().expect("a pinned language tag is valid");
        if locale.id.script.is_none() {
            locale.id.script = Some(
                default_script
                    .parse()
                    .expect("a pinned script tag is valid"),
            );
        }
        if locale.id.region.is_none() {
            locale.id.region = Some(
                default_region
                    .parse()
                    .expect("a pinned region tag is valid"),
            );
        }
    }

    fn patch_minimal_likely_subtags(self, locale: &mut IcuLocale) {
        if locale.id.language.as_str() == "zh"
            && locale
                .id
                .script
                .is_some_and(|script| script.as_str() == "Hant")
            && locale.id.region.is_none()
        {
            locale.id.script = None;
            locale.id.region = Some("TW".parse().expect("a pinned region tag is valid"));
        }
    }

    /// Whether a collation tailoring is data-backed for the selected locale.
    pub fn supports_collation(self, locale: &IcuLocale, collation: &str) -> bool {
        let language = locale.id.language.to_string();
        matches!(collation, "emoji" | "eor")
            || matches!(
                (language.as_str(), collation),
                ("de", "phonebk")
                    | ("es" | "fi" | "sv" | "bn" | "kn", "trad")
                    | ("zh", "pinyin" | "stroke" | "unihan" | "zhuyin")
                    | ("ja" | "ko", "unihan")
                    | ("ko", "searchjl")
                    | ("si", "dict")
                    | ("ar", "compat")
            )
    }

    /// Whether a numbering-system identifier is present in the provider.
    pub const fn supports_numbering_system(self, value: &str) -> bool {
        contains(SUPPORTED_NUMBERING_SYSTEMS, value)
    }

    /// Returns the complete Zone-and-Link registry from the pinned TZDB
    /// bundle, in ECMA-402 canonical supported-values order.
    ///
    /// The UTC-equivalent legacy spellings are folded to `UTC`; all other
    /// accepted IANA Link identifiers are retained rather than replaced by
    /// their targets.
    pub fn time_zones(self) -> Vec<String> {
        let mut values = jiff_tzdb::available()
            .map(canonical_time_zone)
            .map(str::to_owned)
            .collect::<Vec<_>>();
        values.sort_unstable();
        values.dedup();
        values
    }

    /// Returns DateTimeFormat's default calendar for a supported locale.
    pub fn default_calendar(self, locale: &IcuLocale) -> &'static str {
        match locale.id.language.as_str() {
            "fa" => "persian",
            "th" => "buddhist",
            _ => "gregory",
        }
    }

    /// Returns the locale's default decimal numbering system.
    pub fn default_numbering_system(self, locale: &IcuLocale) -> &'static str {
        decimal_symbols::default_numbering_system(&locale.to_string()).unwrap_or("latn")
    }

    /// Returns the locale's default hour cycle.
    pub fn default_hour_cycle(self, locale: &IcuLocale) -> &'static str {
        match locale.id.language.as_str() {
            "ar" | "en" | "ko" => "h12",
            _ => "h23",
        }
    }

    pub(crate) fn calendars_for_region(self, region: &str) -> &'static [&'static str] {
        match region {
            "TH" => &["buddhist", "gregory"],
            "JP" => &["gregory", "japanese"],
            "IN" => &["gregory", "indian"],
            "IR" | "AF" => &[
                "persian",
                "gregory",
                "islamic",
                "islamic-civil",
                "islamic-tbla",
            ],
            "ET" => &["gregory", "ethiopic"],
            "BD" | "MY" | "PK" => &["gregory", "islamic", "islamic-civil", "islamic-tbla"],
            "KR" => &["gregory", "dangi"],
            _ => &["gregory"],
        }
    }

    pub(crate) fn collations_for_language(self, language: &str) -> &'static [&'static str] {
        if language == "und" || ("qfz"..="qtz").contains(&language) {
            &["emoji", "eor"]
        } else {
            &["emoji"]
        }
    }

    pub(crate) fn hour_cycle_for_locale(self, language: &str, region: &str) -> &'static str {
        match (language, region) {
            ("en", "US" | "CA" | "001") | ("ar", "001") => "h12",
            _ => match region {
                "US" | "IN" | "ET" | "BD" | "GR" | "PH" | "KR" | "MY" | "PK" => "h12",
                _ => "h23",
            },
        }
    }

    pub(crate) fn is_right_to_left(self, language: &str, script: Option<&str>) -> bool {
        script.is_some_and(|script| {
            matches!(
                script,
                "Arab" | "Hebr" | "Syrc" | "Thaa" | "Nkoo" | "Adlm" | "Rohg"
            )
        }) || matches!(
            language,
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
        )
    }

    pub(crate) fn time_zones_for_region(self, region: &str) -> &'static [&'static str] {
        match region {
            "US" => &[
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
            "GB" => &["Europe/London"],
            "JP" => &["Asia/Tokyo"],
            "TW" => &["Asia/Taipei"],
            _ => &["Etc/UTC"],
        }
    }

    pub(crate) fn week_data_for_region(self, region: &str) -> (u8, &'static [u8]) {
        match region {
            "US" | "CA" | "JP" | "TH" => (7, &[6, 7]),
            "IN" => (7, &[7]),
            "IR" => (6, &[5]),
            "AF" => (6, &[4, 5]),
            _ => (1, &[6, 7]),
        }
    }

    /// Looks up one localized name in the bounded DisplayNames data bundle.
    ///
    /// A missing entry is data absence, not a semantic fallback; the
    /// DisplayNames service applies its requested `code`/`none` policy after
    /// this lookup.
    pub(crate) fn display_name(
        self,
        locale: &str,
        display_type: crate::DisplayNamesType,
        style: crate::DisplayNamesStyle,
        language_display: Option<crate::DisplayNamesLanguageDisplay>,
        code: &str,
    ) -> Option<String> {
        use crate::{DisplayNamesLanguageDisplay, DisplayNamesStyle, DisplayNamesType};

        let english = locale.starts_with("en");
        let french = locale.starts_with("fr");
        let name = match (display_type, code) {
            (DisplayNamesType::Language, "en") if english => "English",
            (DisplayNamesType::Language, "fr") if english => "French",
            (DisplayNamesType::Language, "de") if english => "German",
            (DisplayNamesType::Language, "es") if english => "Spanish",
            (DisplayNamesType::Language, "ja") if english => "Japanese",
            (DisplayNamesType::Language, "zh") if english => "Chinese",
            (DisplayNamesType::Language, "en-US")
                if english && language_display == Some(DisplayNamesLanguageDisplay::Dialect) =>
            {
                "American English"
            }
            (DisplayNamesType::Language, "en") if french => "anglais",
            (DisplayNamesType::Language, "fr") if french => "français",
            (DisplayNamesType::Region, "US") if english => "United States",
            (DisplayNamesType::Region, "GB") if english => "United Kingdom",
            (DisplayNamesType::Region, "FR") if english => "France",
            (DisplayNamesType::Region, "TW") if english => "Taiwan",
            (DisplayNamesType::Script, "Latn") if english => "Latin",
            (DisplayNamesType::Script, "Cyrl") if english => "Cyrillic",
            (DisplayNamesType::Currency, "USD") if english => "US Dollar",
            (DisplayNamesType::Currency, "EUR") if english => "Euro",
            (DisplayNamesType::Currency, "JPY") if english => "Japanese Yen",
            (DisplayNamesType::Calendar, "gregory") if english => "Gregorian Calendar",
            (DisplayNamesType::Calendar, "buddhist") if english => "Buddhist Calendar",
            (DisplayNamesType::Calendar, "chinese") if english => "Chinese Calendar",
            (DisplayNamesType::Calendar, "coptic") if english => "Coptic Calendar",
            (DisplayNamesType::Calendar, "dangi") if english => "Dangi Calendar",
            (DisplayNamesType::Calendar, "ethioaa") if english => "Ethiopic Amete Alem Calendar",
            (DisplayNamesType::Calendar, "ethiopic") if english => "Ethiopic Calendar",
            (DisplayNamesType::Calendar, "hebrew") if english => "Hebrew Calendar",
            (DisplayNamesType::Calendar, "indian") if english => "Indian National Calendar",
            (DisplayNamesType::Calendar, "islamic-civil") if english => "Islamic Civil Calendar",
            (DisplayNamesType::Calendar, "islamic-tbla") if english => "Islamic Tabular Calendar",
            (DisplayNamesType::Calendar, "islamic-umalqura") if english => {
                "Islamic Calendar (Umm al-Qura)"
            }
            (DisplayNamesType::Calendar, "iso8601") if english => "ISO-8601 Calendar",
            (DisplayNamesType::Calendar, "japanese") if english => "Japanese Calendar",
            (DisplayNamesType::Calendar, "persian") if english => "Persian Calendar",
            (DisplayNamesType::Calendar, "roc") if english => "Minguo Calendar",
            (DisplayNamesType::DateTimeField, "year") if english => "year",
            (DisplayNamesType::DateTimeField, "month") if english => "month",
            (DisplayNamesType::DateTimeField, "day") if english => "day",
            (DisplayNamesType::DateTimeField, "hour") if english => "hour",
            (DisplayNamesType::DateTimeField, "minute") if english => "minute",
            (DisplayNamesType::DateTimeField, "second") if english => "second",
            _ => return None,
        };
        Some(match style {
            DisplayNamesStyle::Long => name.into(),
            // The bundle has no distinct short/narrow name. CLDR permits a
            // parent-width fallback, so retain the long form.
            DisplayNamesStyle::Short | DisplayNamesStyle::Narrow => name.into(),
        })
    }

    /// Returns the bundled DurationFormat unit pattern for `locale`.
    ///
    /// The provider carries the available Spanish CLDR width data alongside
    /// the original English bundle. Other supported languages resolve through
    /// the explicit English parent pattern until their own unit data is added.
    pub(crate) fn duration_unit_pattern(
        self,
        locale: &str,
        unit: crate::DurationUnit,
        style: crate::DurationUnitStyle,
        singular: bool,
    ) -> (&'static str, &'static str) {
        use crate::{DurationUnit, DurationUnitStyle};

        if locale.starts_with("es") {
            return spanish_duration_unit_pattern(unit, style, singular);
        }
        match style {
            DurationUnitStyle::Long => (
                " ",
                match (unit, singular) {
                    (DurationUnit::Years, true) => "year",
                    (DurationUnit::Months, true) => "month",
                    (DurationUnit::Weeks, true) => "week",
                    (DurationUnit::Days, true) => "day",
                    (DurationUnit::Hours, true) => "hour",
                    (DurationUnit::Minutes, true) => "minute",
                    (DurationUnit::Seconds, true) => "second",
                    (DurationUnit::Milliseconds, true) => "millisecond",
                    (DurationUnit::Microseconds, true) => "microsecond",
                    (DurationUnit::Nanoseconds, true) => "nanosecond",
                    (DurationUnit::Years, false) => "years",
                    (DurationUnit::Months, false) => "months",
                    (DurationUnit::Weeks, false) => "weeks",
                    (DurationUnit::Days, false) => "days",
                    (DurationUnit::Hours, false) => "hours",
                    (DurationUnit::Minutes, false) => "minutes",
                    (DurationUnit::Seconds, false) => "seconds",
                    (DurationUnit::Milliseconds, false) => "milliseconds",
                    (DurationUnit::Microseconds, false) => "microseconds",
                    (DurationUnit::Nanoseconds, false) => "nanoseconds",
                },
            ),
            DurationUnitStyle::Short => (
                " ",
                match (unit, singular) {
                    (DurationUnit::Years, true) => "yr",
                    (DurationUnit::Years, false) => "yrs",
                    (DurationUnit::Months, true) => "mth",
                    (DurationUnit::Months, false) => "mths",
                    (DurationUnit::Weeks, true) => "wk",
                    (DurationUnit::Weeks, false) => "wks",
                    (DurationUnit::Days, true) => "day",
                    (DurationUnit::Days, false) => "days",
                    (DurationUnit::Hours, _) => "hr",
                    (DurationUnit::Minutes, _) => "min",
                    (DurationUnit::Seconds, _) => "sec",
                    (DurationUnit::Milliseconds, _) => "ms",
                    (DurationUnit::Microseconds, _) => "μs",
                    (DurationUnit::Nanoseconds, _) => "ns",
                },
            ),
            DurationUnitStyle::Narrow => (
                "",
                match unit {
                    DurationUnit::Years => "y",
                    DurationUnit::Months => "m",
                    DurationUnit::Weeks => "w",
                    DurationUnit::Days => "d",
                    DurationUnit::Hours => "h",
                    DurationUnit::Minutes => "m",
                    DurationUnit::Seconds => "s",
                    DurationUnit::Milliseconds => "ms",
                    DurationUnit::Microseconds => "μs",
                    DurationUnit::Nanoseconds => "ns",
                },
            ),
            // Callers route numeric styles to FormatNumericUnits; the neutral
            // empty pattern keeps this data lookup total if one misroutes it.
            DurationUnitStyle::Numeric | DurationUnitStyle::TwoDigit => ("", ""),
        }
    }

    /// Returns the NumberFormat simple-unit pattern selected from the pinned
    /// CLDR unit-name records when that unit category is bundled.
    ///
    /// ICU4X currently exposes typed generated records for area, duration,
    /// length, mass, and volume. The ECMA sanctioned inventory also includes
    /// categories that ICU4X has not generated yet (digital, temperature,
    /// angle, and percent). Pinned raw CLDR records fill the remaining cells
    /// for every advertised NumberFormat locale; unsupported locales retain
    /// the provider's bounded English fallback.
    pub(crate) fn number_unit_pattern(
        self,
        locale: &str,
        unit: crate::NumberFormatUnit,
        display: crate::NumberUnitDisplay,
        plural: crate::PluralCategory,
    ) -> NumberUnitPattern {
        self.number_unit_pattern_with_provenance(locale, unit, display, plural)
            .0
    }

    /// Resolves a simple-unit pattern together with whether it is localized
    /// provider data rather than the bounded English compatibility fallback.
    fn number_unit_pattern_with_provenance(
        self,
        locale: &str,
        unit: crate::NumberFormatUnit,
        display: crate::NumberUnitDisplay,
        plural: crate::PluralCategory,
    ) -> (NumberUnitPattern, bool) {
        if let Some(pattern) = experimental_number_unit_pattern(locale, unit, display, plural) {
            return (pattern, true);
        }
        if let Some(pattern) = cldr_temperature_unit_pattern(locale, unit, display, plural) {
            return (pattern, true);
        }
        if let Some(pattern) = cldr_korean_additional_unit_pattern(locale, unit, display, plural) {
            return (pattern, true);
        }
        if let Some(pattern) = cldr_chinese_additional_unit_pattern(locale, unit, display, plural) {
            return (pattern, true);
        }
        if let Some(pattern) = cldr_german_digital_unit_pattern(locale, unit, display, plural) {
            return (pattern, true);
        }
        if let Some(pattern) = unit_patterns::additional_unit_pattern(locale, unit, display, plural)
        {
            return (pattern, true);
        }
        if let Some(pattern) =
            cldr_portuguese_additional_unit_pattern(locale, unit, display, plural)
        {
            return (pattern, true);
        }
        if let Some(pattern) = cldr_italian_additional_unit_pattern(locale, unit, display, plural) {
            return (pattern, true);
        }
        if let Some(pattern) = cldr_dutch_additional_unit_pattern(locale, unit, display, plural) {
            return (pattern, true);
        }
        if let Some(pattern) = cldr_japanese_digital_unit_pattern(locale, unit, display, plural) {
            return (pattern, true);
        }
        if let Some(pattern) = cldr_russian_digital_unit_pattern(locale, unit, display, plural) {
            return (pattern, true);
        }
        if let Some(pattern) = cldr_arabic_digital_unit_pattern(locale, unit, display, plural) {
            return (pattern, true);
        }
        if let Some(pattern) = cldr_french_digital_unit_pattern(locale, unit, display, plural) {
            return (pattern, true);
        }
        if let Some(pattern) = cldr_spanish_additional_unit_pattern(locale, unit, display, plural) {
            return (pattern, true);
        }
        {
            let duration_unit = match unit {
                crate::NumberFormatUnit::Year => Some(crate::DurationUnit::Years),
                crate::NumberFormatUnit::Month => Some(crate::DurationUnit::Months),
                crate::NumberFormatUnit::Week => Some(crate::DurationUnit::Weeks),
                crate::NumberFormatUnit::Day => Some(crate::DurationUnit::Days),
                crate::NumberFormatUnit::Hour => Some(crate::DurationUnit::Hours),
                crate::NumberFormatUnit::Minute => Some(crate::DurationUnit::Minutes),
                crate::NumberFormatUnit::Second => Some(crate::DurationUnit::Seconds),
                crate::NumberFormatUnit::Millisecond => Some(crate::DurationUnit::Milliseconds),
                crate::NumberFormatUnit::Microsecond => Some(crate::DurationUnit::Microseconds),
                crate::NumberFormatUnit::Nanosecond => Some(crate::DurationUnit::Nanoseconds),
                _ => None,
            };
            if let Some(unit) = duration_unit {
                let style = match display {
                    crate::NumberUnitDisplay::Long => crate::DurationUnitStyle::Long,
                    crate::NumberUnitDisplay::Short => crate::DurationUnitStyle::Short,
                    crate::NumberUnitDisplay::Narrow => crate::DurationUnitStyle::Narrow,
                };
                let (separator, label) = self.duration_unit_pattern(
                    locale,
                    unit,
                    style,
                    plural == crate::PluralCategory::One,
                );
                return (
                    NumberUnitPattern {
                        prefix: String::new(),
                        prefix_separator: String::new(),
                        suffix_separator: separator.into(),
                        suffix: label.into(),
                        hides_number: false,
                    },
                    locale.starts_with("es"),
                );
            }
            (
                english_number_unit_pattern(unit, display, plural == crate::PluralCategory::One),
                false,
            )
        }
    }

    /// Returns a data-backed compound NumberFormat unit pattern.
    ///
    /// This is intentionally a provider query rather than a NumberFormat
    /// string rewrite: localized word order and the `unit`/`literal` part
    /// boundary are observable through `formatToParts`.
    pub(crate) fn number_compound_unit_pattern(
        self,
        locale: &str,
        numerator: &str,
        denominator: &str,
        display: crate::NumberUnitDisplay,
        plural: crate::PluralCategory,
    ) -> Option<NumberCompoundUnitPattern> {
        if (numerator, denominator) != ("kilometer", "hour") {
            return None;
        }
        use crate::NumberUnitDisplay::{Long, Narrow, Short};
        use crate::PluralCategory::{Few, One};
        let language = locale.split('-').next().unwrap_or(locale);
        Some(match (language, display) {
            ("de", Short | Narrow) => NumberCompoundUnitPattern {
                prefix: "",
                prefix_separator: "",
                suffix_separator: " ",
                suffix: "km/h",
            },
            ("de", Long) => NumberCompoundUnitPattern {
                prefix: "",
                prefix_separator: "",
                suffix_separator: " ",
                suffix: "Kilometer pro Stunde",
            },
            ("es", Long) => NumberCompoundUnitPattern {
                prefix: "",
                prefix_separator: "",
                suffix_separator: " ",
                suffix: if plural == One {
                    "kilómetro por hora"
                } else {
                    "kilómetros por hora"
                },
            },
            ("es", Short) => NumberCompoundUnitPattern {
                prefix: "",
                prefix_separator: "",
                suffix_separator: " ",
                suffix: "km/h",
            },
            ("es", Narrow) => NumberCompoundUnitPattern {
                prefix: "",
                prefix_separator: "",
                suffix_separator: "",
                suffix: "km/h",
            },
            ("fr", Long) => NumberCompoundUnitPattern {
                prefix: "",
                prefix_separator: "",
                suffix_separator: "\u{a0}",
                suffix: if plural == One {
                    "kilomètre par heure"
                } else {
                    "kilomètres par heure"
                },
            },
            ("fr", Short) => NumberCompoundUnitPattern {
                prefix: "",
                prefix_separator: "",
                suffix_separator: "\u{202f}",
                suffix: "km/h",
            },
            ("fr", Narrow) => NumberCompoundUnitPattern {
                prefix: "",
                prefix_separator: "",
                suffix_separator: "",
                suffix: "km/h",
            },
            ("it", Long) => NumberCompoundUnitPattern {
                prefix: "",
                prefix_separator: "",
                suffix_separator: " ",
                suffix: if plural == One {
                    "chilometro orario"
                } else {
                    "chilometri orari"
                },
            },
            ("it", Short) => NumberCompoundUnitPattern {
                prefix: "",
                prefix_separator: "",
                suffix_separator: " ",
                suffix: "km/h",
            },
            ("it", Narrow) => NumberCompoundUnitPattern {
                prefix: "",
                prefix_separator: "",
                suffix_separator: "",
                suffix: "km/h",
            },
            ("pt", Long) => NumberCompoundUnitPattern {
                prefix: "",
                prefix_separator: "",
                suffix_separator: " ",
                suffix: if plural == One {
                    "quilômetro por hora"
                } else {
                    "quilômetros por hora"
                },
            },
            ("pt", Short) => NumberCompoundUnitPattern {
                prefix: "",
                prefix_separator: "",
                suffix_separator: " ",
                suffix: "km/h",
            },
            ("pt", Narrow) => NumberCompoundUnitPattern {
                prefix: "",
                prefix_separator: "",
                suffix_separator: "",
                suffix: "km/h",
            },
            ("ru", Long) => NumberCompoundUnitPattern {
                prefix: "",
                prefix_separator: "",
                suffix_separator: " ",
                suffix: match plural {
                    One => "километр в час",
                    Few => "километра в час",
                    _ => "километров в час",
                },
            },
            ("ru", Short | Narrow) => NumberCompoundUnitPattern {
                prefix: "",
                prefix_separator: "",
                suffix_separator: " ",
                suffix: "км/ч",
            },
            ("ar", Long) => NumberCompoundUnitPattern {
                prefix: "",
                prefix_separator: "",
                suffix_separator: " ",
                suffix: "كيلومتر في الساعة",
            },
            ("ar", Short | Narrow) => NumberCompoundUnitPattern {
                prefix: "",
                prefix_separator: "",
                suffix_separator: " ",
                suffix: "كم/س",
            },
            ("hi", Long) => NumberCompoundUnitPattern {
                prefix: "",
                prefix_separator: "",
                suffix_separator: " ",
                suffix: "किलोमीटर प्रति घंटा",
            },
            ("hi", Short) => NumberCompoundUnitPattern {
                prefix: "",
                prefix_separator: "",
                suffix_separator: " ",
                suffix: "कि॰मी॰/घं॰",
            },
            ("hi", Narrow) => NumberCompoundUnitPattern {
                prefix: "",
                prefix_separator: "",
                suffix_separator: " ",
                suffix: "किमी/घं",
            },
            ("ja", Short) => NumberCompoundUnitPattern {
                prefix: "",
                prefix_separator: "",
                suffix_separator: " ",
                suffix: "km/h",
            },
            ("ja", Narrow) => NumberCompoundUnitPattern {
                prefix: "",
                prefix_separator: "",
                suffix_separator: "",
                suffix: "km/h",
            },
            ("ja", Long) => NumberCompoundUnitPattern {
                prefix: "時速",
                prefix_separator: " ",
                suffix_separator: " ",
                suffix: "キロメートル",
            },
            ("ko", Short | Narrow) => NumberCompoundUnitPattern {
                prefix: "",
                prefix_separator: "",
                suffix_separator: "",
                suffix: "km/h",
            },
            ("ko", Long) => NumberCompoundUnitPattern {
                prefix: "시속",
                prefix_separator: " ",
                suffix_separator: "",
                suffix: "킬로미터",
            },
            ("zh", Short) => NumberCompoundUnitPattern {
                prefix: "",
                prefix_separator: "",
                suffix_separator: " ",
                suffix: "公里/小時",
            },
            ("zh", Narrow) => NumberCompoundUnitPattern {
                prefix: "",
                prefix_separator: "",
                suffix_separator: "",
                suffix: "公里/小時",
            },
            ("zh", Long) => NumberCompoundUnitPattern {
                prefix: "每小時",
                prefix_separator: " ",
                suffix_separator: " ",
                suffix: "公里",
            },
            // The direct `speed-kilometer-per-hour` record is only an
            // optimization for the explicitly localized shapes above. All
            // other supported locales continue through the complete CLDR
            // generic-compound table instead of receiving an English form.
            _ => return None,
        })
    }

    /// Returns the CLDR-composed generic `-per-` unit pattern.
    ///
    /// Every resolved CLDR locale carries a generic `per` record and a
    /// localized display name for each sanctioned denominator. Supported
    /// NumberFormat locales therefore never cross this method's English
    /// compatibility boundary merely because a direct `perUnitPattern` is
    /// absent.
    pub(crate) fn number_generic_compound_unit_pattern(
        self,
        locale: &str,
        numerator: crate::NumberFormatUnit,
        denominator: crate::NumberFormatUnit,
        display: crate::NumberUnitDisplay,
        plural: crate::PluralCategory,
    ) -> NumberGenericCompoundUnitPattern {
        unit_patterns::cldr_full_generic_compound_unit_pattern(
            locale,
            numerator,
            denominator,
            display,
            plural,
        )
        .expect("every resolved NumberFormat locale has pinned CLDR generic-compound data")
    }

    /// Whether a generic compound's full CLDR numerator pattern hides the
    /// formatted number. This is a property of the selected plural form, not
    /// a compatibility fallback.
    pub(crate) fn generic_compound_unit_hides_number(
        self,
        locale: &str,
        numerator: crate::NumberFormatUnit,
        display: crate::NumberUnitDisplay,
        plural: crate::PluralCategory,
    ) -> bool {
        unit_patterns::cldr_full_generic_compound_hides_number(locale, numerator, display, plural)
            .unwrap_or(false)
    }

    /// Returns the CLDR compact-decimal scale and suffix for one magnitude.
    ///
    /// A `None` result means the locale's compact data deliberately leaves
    /// that magnitude in ordinary decimal notation; callers must not invent a
    /// suffix. The output's dynamic fractional precision is handled by the
    /// NumberFormat core after this data selection.
    pub(crate) fn compact_number_pattern(
        self,
        locale: &str,
        numbering_system: &str,
        magnitude: i16,
        display: crate::NumberCompactDisplay,
        plural: crate::PluralCategory,
        rounded_value: Option<&Decimal>,
    ) -> Option<CompactNumberPattern> {
        compact_patterns::compact_number_pattern(
            locale,
            numbering_system,
            magnitude,
            display,
            plural,
            rounded_value,
        )
    }

    /// Resolves CLDR's cardinal plural category for a formatted numeric
    /// range. Missing explicit range data follows UTS 35's end-category
    /// fallback inside ICU4X.
    pub(crate) fn number_range_plural_category(
        self,
        locale: &str,
        start: crate::PluralCategory,
        end: crate::PluralCategory,
    ) -> Option<crate::PluralCategory> {
        let locale = crate::canonicalize(locale).ok()?;
        let rules = IcuPluralRulesWithRanges::try_new_cardinal(locale.locale().into()).ok()?;
        Some(plural_category_from_icu(rules.resolve_range(
            plural_category_to_icu(start),
            plural_category_to_icu(end),
        )))
    }

    /// Returns the provider-owned range pattern for a resolved NumberFormat
    /// locale.
    ///
    /// ICU4X's current decimal provider does not expose the CLDR
    /// `miscPatterns.range` record or NumberRangeFormatter's style-sensitive
    /// interval selection. The two connectors preserve both data shapes: a
    /// collapsed pattern has no endpoint-affix spacing, while the full-
    /// endpoint form retains its localized outer spacing.
    pub(crate) fn number_range_pattern(self, locale: &str) -> NumberRangePattern {
        range_patterns::number_range_pattern(locale)
    }

    /// Whether the legacy percent-only compatibility branch places its sign
    /// after the number for this locale family.
    pub(crate) fn fallback_percent_is_trailing(self, locale: &str) -> bool {
        matches!(
            locale.split('-').next().unwrap_or(locale),
            "ar" | "be"
                | "bg"
                | "ca"
                | "cs"
                | "da"
                | "de"
                | "el"
                | "es"
                | "et"
                | "fi"
                | "fr"
                | "he"
                | "hr"
                | "hu"
                | "is"
                | "it"
                | "lt"
                | "lv"
                | "nl"
                | "no"
                | "pl"
                | "pt"
                | "ro"
                | "ru"
                | "sk"
                | "sl"
                | "sr"
                | "sv"
                | "tr"
                | "uk"
        )
    }

    /// Whether the bounded percent fallback adds a non-breaking space before
    /// its percent sign.
    pub(crate) fn fallback_percent_has_space(self, locale: &str) -> bool {
        self.fallback_percent_is_trailing(locale)
            && !matches!(
                locale.split('-').next().unwrap_or(locale),
                "ar" | "he" | "tr"
            )
    }

    /// Returns whether the bundled relative-time data uses Polish patterns.
    pub(crate) fn relative_time_uses_polish(self, locale: &str) -> bool {
        locale.starts_with("pl")
    }

    /// Returns a qualitative English relative-time term when bundled data has
    /// one for `numeric: "auto"`.
    pub(crate) fn relative_time_qualitative_term(
        self,
        locale: &str,
        numeric: crate::RelativeTimeNumeric,
        value: f64,
        unit: crate::RelativeTimeUnit,
    ) -> Option<&'static str> {
        use crate::{RelativeTimeNumeric, RelativeTimeUnit};

        if numeric != RelativeTimeNumeric::Auto || !locale.starts_with("en") {
            return None;
        }
        match (value.is_sign_negative(), value.abs() as i64, unit) {
            (_, 0, RelativeTimeUnit::Second) if value == 0.0 => Some("now"),
            (_, 0, RelativeTimeUnit::Minute) if value == 0.0 => Some("this minute"),
            (_, 0, RelativeTimeUnit::Hour) if value == 0.0 => Some("this hour"),
            (_, 0, RelativeTimeUnit::Day) if value == 0.0 => Some("today"),
            (false, 1, RelativeTimeUnit::Day) => Some("tomorrow"),
            (true, 1, RelativeTimeUnit::Day) => Some("yesterday"),
            (_, 0, RelativeTimeUnit::Week) if value == 0.0 => Some("this week"),
            (false, 1, RelativeTimeUnit::Week) => Some("next week"),
            (true, 1, RelativeTimeUnit::Week) => Some("last week"),
            (_, 0, RelativeTimeUnit::Month) if value == 0.0 => Some("this month"),
            (false, 1, RelativeTimeUnit::Month) => Some("next month"),
            (true, 1, RelativeTimeUnit::Month) => Some("last month"),
            (_, 0, RelativeTimeUnit::Quarter) if value == 0.0 => Some("this quarter"),
            (false, 1, RelativeTimeUnit::Quarter) => Some("next quarter"),
            (true, 1, RelativeTimeUnit::Quarter) => Some("last quarter"),
            (_, 0, RelativeTimeUnit::Year) if value == 0.0 => Some("this year"),
            (false, 1, RelativeTimeUnit::Year) => Some("next year"),
            (true, 1, RelativeTimeUnit::Year) => Some("last year"),
            _ => None,
        }
    }

    /// Returns the bundled relative-time number affix for a finite value.
    pub(crate) fn relative_time_affixes(
        self,
        locale: &str,
        past: bool,
        label: &str,
    ) -> (&'static str, String) {
        let prefix = if past {
            ""
        } else if self.relative_time_uses_polish(locale) {
            "za "
        } else {
            "in "
        };
        let suffix = match (past, self.relative_time_uses_polish(locale)) {
            (true, true) => format!(" {label} temu"),
            (true, false) => format!(" {label} ago"),
            (false, _) => format!(" {label}"),
        };
        (prefix, suffix)
    }

    /// Returns the bundled relative-time unit pattern for one finite value.
    pub(crate) fn relative_time_unit_label(
        self,
        locale: &str,
        style: crate::RelativeTimeStyle,
        unit: crate::RelativeTimeUnit,
        value: f64,
    ) -> &'static str {
        if self.relative_time_uses_polish(locale) {
            return polish_relative_time_label(style, unit, value);
        }
        english_relative_time_label(style, unit, value)
    }
}

fn spanish_duration_unit_pattern(
    unit: crate::DurationUnit,
    style: crate::DurationUnitStyle,
    singular: bool,
) -> (&'static str, &'static str) {
    use crate::{DurationUnit, DurationUnitStyle};

    let label = match style {
        DurationUnitStyle::Long => match (unit, singular) {
            (DurationUnit::Years, true) => "año",
            (DurationUnit::Months, true) => "mes",
            (DurationUnit::Weeks, true) => "semana",
            (DurationUnit::Days, true) => "día",
            (DurationUnit::Hours, true) => "hora",
            (DurationUnit::Minutes, true) => "minuto",
            (DurationUnit::Seconds, true) => "segundo",
            (DurationUnit::Milliseconds, true) => "milisegundo",
            (DurationUnit::Microseconds, true) => "microsegundo",
            (DurationUnit::Nanoseconds, true) => "nanosegundo",
            (DurationUnit::Years, false) => "años",
            (DurationUnit::Months, false) => "meses",
            (DurationUnit::Weeks, false) => "semanas",
            (DurationUnit::Days, false) => "días",
            (DurationUnit::Hours, false) => "horas",
            (DurationUnit::Minutes, false) => "minutos",
            (DurationUnit::Seconds, false) => "segundos",
            (DurationUnit::Milliseconds, false) => "milisegundos",
            (DurationUnit::Microseconds, false) => "microsegundos",
            (DurationUnit::Nanoseconds, false) => "nanosegundos",
        },
        DurationUnitStyle::Short => match unit {
            DurationUnit::Years => "a",
            DurationUnit::Months => "m",
            DurationUnit::Weeks => "sem",
            DurationUnit::Days => "d",
            DurationUnit::Hours => "h",
            DurationUnit::Minutes => "min",
            DurationUnit::Seconds => "s",
            DurationUnit::Milliseconds => "ms",
            DurationUnit::Microseconds => "μs",
            DurationUnit::Nanoseconds => "ns",
        },
        DurationUnitStyle::Narrow => match unit {
            DurationUnit::Years => "a",
            DurationUnit::Months => "m",
            DurationUnit::Weeks => "sem",
            DurationUnit::Days => "d",
            DurationUnit::Hours => "h",
            DurationUnit::Minutes => "min",
            DurationUnit::Seconds => "s",
            DurationUnit::Milliseconds => "ms",
            DurationUnit::Microseconds => "μs",
            DurationUnit::Nanoseconds => "ns",
        },
        DurationUnitStyle::Numeric | DurationUnitStyle::TwoDigit => "",
    };
    let separator = if matches!(style, DurationUnitStyle::Narrow) {
        ""
    } else {
        " "
    };
    (separator, label)
}

fn english_relative_time_label(
    style: crate::RelativeTimeStyle,
    unit: crate::RelativeTimeUnit,
    value: f64,
) -> &'static str {
    use crate::{RelativeTimeStyle, RelativeTimeUnit};

    let one = value == 1.0;
    match (style, unit, one) {
        (RelativeTimeStyle::Long, RelativeTimeUnit::Second, true) => "second",
        (RelativeTimeStyle::Long, RelativeTimeUnit::Minute, true) => "minute",
        (RelativeTimeStyle::Long, RelativeTimeUnit::Hour, true) => "hour",
        (RelativeTimeStyle::Long, RelativeTimeUnit::Day, true) => "day",
        (RelativeTimeStyle::Long, RelativeTimeUnit::Week, true) => "week",
        (RelativeTimeStyle::Long, RelativeTimeUnit::Month, true) => "month",
        (RelativeTimeStyle::Long, RelativeTimeUnit::Quarter, true) => "quarter",
        (RelativeTimeStyle::Long, RelativeTimeUnit::Year, true) => "year",
        (RelativeTimeStyle::Long, RelativeTimeUnit::Second, false) => "seconds",
        (RelativeTimeStyle::Long, RelativeTimeUnit::Minute, false) => "minutes",
        (RelativeTimeStyle::Long, RelativeTimeUnit::Hour, false) => "hours",
        (RelativeTimeStyle::Long, RelativeTimeUnit::Day, false) => "days",
        (RelativeTimeStyle::Long, RelativeTimeUnit::Week, false) => "weeks",
        (RelativeTimeStyle::Long, RelativeTimeUnit::Month, false) => "months",
        (RelativeTimeStyle::Long, RelativeTimeUnit::Quarter, false) => "quarters",
        (RelativeTimeStyle::Long, RelativeTimeUnit::Year, false) => "years",
        (RelativeTimeStyle::Short, RelativeTimeUnit::Second, _) => "sec.",
        (RelativeTimeStyle::Short, RelativeTimeUnit::Minute, _) => "min.",
        (RelativeTimeStyle::Short, RelativeTimeUnit::Hour, _) => "hr.",
        (RelativeTimeStyle::Short, RelativeTimeUnit::Day, true) => "day",
        (RelativeTimeStyle::Short, RelativeTimeUnit::Day, false) => "days",
        (RelativeTimeStyle::Short, RelativeTimeUnit::Week, _) => "wk.",
        (RelativeTimeStyle::Short, RelativeTimeUnit::Month, _) => "mo.",
        (RelativeTimeStyle::Short, RelativeTimeUnit::Quarter, true) => "qtr.",
        (RelativeTimeStyle::Short, RelativeTimeUnit::Quarter, false) => "qtrs.",
        (RelativeTimeStyle::Short, RelativeTimeUnit::Year, _) => "yr.",
        (RelativeTimeStyle::Narrow, RelativeTimeUnit::Second, _) => "s",
        (RelativeTimeStyle::Narrow, RelativeTimeUnit::Minute, _) => "m",
        (RelativeTimeStyle::Narrow, RelativeTimeUnit::Hour, _) => "h",
        (RelativeTimeStyle::Narrow, RelativeTimeUnit::Day, _) => "d",
        (RelativeTimeStyle::Narrow, RelativeTimeUnit::Week, _) => "w",
        (RelativeTimeStyle::Narrow, RelativeTimeUnit::Month, _) => "mo",
        (RelativeTimeStyle::Narrow, RelativeTimeUnit::Quarter, _) => "q",
        (RelativeTimeStyle::Narrow, RelativeTimeUnit::Year, _) => "y",
    }
}

fn polish_relative_time_label(
    style: crate::RelativeTimeStyle,
    unit: crate::RelativeTimeUnit,
    value: f64,
) -> &'static str {
    use crate::{RelativeTimeStyle, RelativeTimeUnit};

    #[derive(Clone, Copy)]
    enum Category {
        One,
        Few,
        Many,
        Other,
    }

    let integer = value.fract() == 0.0;
    let number = value as i64;
    let category = if integer && number == 1 {
        Category::One
    } else if integer
        && (2..=4).contains(&(number.rem_euclid(10)))
        && !(12..=14).contains(&(number.rem_euclid(100)))
    {
        Category::Few
    } else if integer {
        Category::Many
    } else {
        Category::Other
    };
    let select = |one, few, many, other| match category {
        Category::One => one,
        Category::Few => few,
        Category::Many => many,
        Category::Other => other,
    };
    match style {
        RelativeTimeStyle::Long => match unit {
            RelativeTimeUnit::Second => select("sekundę", "sekundy", "sekund", "sekundy"),
            RelativeTimeUnit::Minute => select("minutę", "minuty", "minut", "minuty"),
            RelativeTimeUnit::Hour => select("godzinę", "godziny", "godzin", "godziny"),
            RelativeTimeUnit::Day => select("dzień", "dni", "dni", "dnia"),
            RelativeTimeUnit::Week => select("tydzień", "tygodnie", "tygodni", "tygodnia"),
            RelativeTimeUnit::Month => select("miesiąc", "miesiące", "miesięcy", "miesiąca"),
            RelativeTimeUnit::Quarter => select("kwartał", "kwartały", "kwartałów", "kwartału"),
            RelativeTimeUnit::Year => select("rok", "lata", "lat", "roku"),
        },
        RelativeTimeStyle::Short => match unit {
            RelativeTimeUnit::Second => "sek.",
            RelativeTimeUnit::Minute => "min",
            RelativeTimeUnit::Hour => "godz.",
            RelativeTimeUnit::Day => select("dzień", "dni", "dni", "dnia"),
            RelativeTimeUnit::Week => select("tydz.", "tyg.", "tyg.", "tyg."),
            RelativeTimeUnit::Month => "mies.",
            RelativeTimeUnit::Quarter => "kw.",
            RelativeTimeUnit::Year => select("rok", "lata", "lat", "roku"),
        },
        RelativeTimeStyle::Narrow => match unit {
            RelativeTimeUnit::Second => "s",
            RelativeTimeUnit::Minute => "min",
            RelativeTimeUnit::Hour => "g.",
            RelativeTimeUnit::Day => select("dzień", "dni", "dni", "dnia"),
            RelativeTimeUnit::Week => select("tydz.", "tyg.", "tyg.", "tyg."),
            RelativeTimeUnit::Month => "mies.",
            RelativeTimeUnit::Quarter => "kw.",
            RelativeTimeUnit::Year => select("rok", "lata", "lat", "roku"),
        },
    }
}

#[derive(Clone, Copy)]
enum ExperimentalUnitCategory {
    Area,
    Duration,
    Length,
    Mass,
    Volume,
}

fn experimental_unit_category_and_id(
    unit: crate::NumberFormatUnit,
) -> Option<(ExperimentalUnitCategory, &'static str)> {
    use crate::NumberFormatUnit as Unit;

    Some(match unit {
        Unit::Acre => (ExperimentalUnitCategory::Area, "acre"),
        Unit::Hectare => (ExperimentalUnitCategory::Area, "hectare"),
        Unit::Day => (ExperimentalUnitCategory::Duration, "day"),
        Unit::Hour => (ExperimentalUnitCategory::Duration, "hour"),
        Unit::Microsecond => (ExperimentalUnitCategory::Duration, "microsecond"),
        Unit::Millisecond => (ExperimentalUnitCategory::Duration, "millisecond"),
        Unit::Minute => (ExperimentalUnitCategory::Duration, "minute"),
        Unit::Month => (ExperimentalUnitCategory::Duration, "month"),
        Unit::Nanosecond => (ExperimentalUnitCategory::Duration, "nanosecond"),
        Unit::Second => (ExperimentalUnitCategory::Duration, "second"),
        Unit::Week => (ExperimentalUnitCategory::Duration, "week"),
        Unit::Year => (ExperimentalUnitCategory::Duration, "year"),
        Unit::Centimeter => (ExperimentalUnitCategory::Length, "centimeter"),
        Unit::Foot => (ExperimentalUnitCategory::Length, "foot"),
        Unit::Inch => (ExperimentalUnitCategory::Length, "inch"),
        Unit::Kilometer => (ExperimentalUnitCategory::Length, "kilometer"),
        Unit::Meter => (ExperimentalUnitCategory::Length, "meter"),
        Unit::Mile => (ExperimentalUnitCategory::Length, "mile"),
        Unit::MileScandinavian => (ExperimentalUnitCategory::Length, "mile-scandinavian"),
        Unit::Millimeter => (ExperimentalUnitCategory::Length, "millimeter"),
        Unit::Yard => (ExperimentalUnitCategory::Length, "yard"),
        Unit::Gram => (ExperimentalUnitCategory::Mass, "gram"),
        Unit::Kilogram => (ExperimentalUnitCategory::Mass, "kilogram"),
        Unit::Ounce => (ExperimentalUnitCategory::Mass, "ounce"),
        Unit::Pound => (ExperimentalUnitCategory::Mass, "pound"),
        Unit::Stone => (ExperimentalUnitCategory::Mass, "stone"),
        Unit::FluidOunce => (ExperimentalUnitCategory::Volume, "fluid-ounce"),
        Unit::Gallon => (ExperimentalUnitCategory::Volume, "gallon"),
        Unit::Liter => (ExperimentalUnitCategory::Volume, "liter"),
        Unit::Milliliter => (ExperimentalUnitCategory::Volume, "milliliter"),
        Unit::Bit
        | Unit::Byte
        | Unit::Celsius
        | Unit::Degree
        | Unit::Fahrenheit
        | Unit::Gigabit
        | Unit::Gigabyte
        | Unit::Kilobit
        | Unit::Kilobyte
        | Unit::Megabit
        | Unit::Megabyte
        | Unit::Percent
        | Unit::Petabyte
        | Unit::Terabit
        | Unit::Terabyte
        | Unit::CompoundPer { .. } => return None,
    })
}

fn experimental_number_unit_pattern(
    locale: &str,
    unit: crate::NumberFormatUnit,
    display: crate::NumberUnitDisplay,
    plural: crate::PluralCategory,
) -> Option<NumberUnitPattern> {
    let (category, identifier) = experimental_unit_category_and_id(unit)?;
    let locale = crate::canonicalize(locale).ok()?;
    let rules = IcuPluralRules::try_new_cardinal(locale.locale().into()).ok()?;
    let operands = plural_category_sample(&rules, plural)?;
    let width = match display {
        crate::NumberUnitDisplay::Long => "long",
        crate::NumberUnitDisplay::Short => "short",
        crate::NumberUnitDisplay::Narrow => "narrow",
    };
    let attribute = format!("{width}-{identifier}");

    macro_rules! query_category {
        ($core:ty, $extended:ty, $outlier:ty) => {{
            let mut root_fallback = None;
            let localized = [
                experimental_number_unit_pattern_for_marker::<$core>(
                    locale.locale(),
                    &attribute,
                    operands,
                    &rules,
                ),
                experimental_number_unit_pattern_for_marker::<$extended>(
                    locale.locale(),
                    &attribute,
                    operands,
                    &rules,
                ),
                experimental_number_unit_pattern_for_marker::<$outlier>(
                    locale.locale(),
                    &attribute,
                    operands,
                    &rules,
                ),
            ]
            .into_iter()
            .find_map(|candidate| {
                if let Some((pattern, is_localized)) = candidate {
                    if is_localized {
                        return Some(pattern);
                    }
                    root_fallback = Some(pattern);
                }
                None
            });
            localized.or(root_fallback)
        }};
    }

    match category {
        ExperimentalUnitCategory::Area => query_category!(
            UnitsNamesAreaCoreV1,
            UnitsNamesAreaExtendedV1,
            UnitsNamesAreaOutlierV1
        ),
        ExperimentalUnitCategory::Duration => query_category!(
            UnitsNamesDurationCoreV1,
            UnitsNamesDurationExtendedV1,
            UnitsNamesDurationOutlierV1
        ),
        ExperimentalUnitCategory::Length => query_category!(
            UnitsNamesLengthCoreV1,
            UnitsNamesLengthExtendedV1,
            UnitsNamesLengthOutlierV1
        ),
        ExperimentalUnitCategory::Mass => query_category!(
            UnitsNamesMassCoreV1,
            UnitsNamesMassExtendedV1,
            UnitsNamesMassOutlierV1
        ),
        ExperimentalUnitCategory::Volume => query_category!(
            UnitsNamesVolumeCoreV1,
            UnitsNamesVolumeExtendedV1,
            UnitsNamesVolumeOutlierV1
        ),
    }
}

fn experimental_number_unit_pattern_for_marker<M>(
    locale: &IcuLocale,
    attribute: &str,
    operands: usize,
    rules: &IcuPluralRules,
) -> Option<(NumberUnitPattern, bool)>
where
    M: DataMarker<DataStruct = UnitsDisplayNames<'static>>,
    ExperimentalData: DataProvider<M>,
{
    let attributes = DataMarkerAttributes::try_from_utf8(attribute.as_bytes()).ok()?;
    let data_locale = icu_locale_core::DataLocale::from(locale.clone());
    let response = <ExperimentalData as DataProvider<M>>::load(
        &ExperimentalData,
        DataRequest {
            id: DataIdentifierBorrowed::for_marker_attributes_and_locale(attributes, &data_locale),
            metadata: Default::default(),
        },
    )
    .ok()?;
    let is_localized = response
        .metadata
        .locale
        .is_none_or(|locale| !locale.is_unknown());
    let rendered = response
        .payload
        .get()
        .get(operands.into(), rules)
        .interpolate(["\u{fdd0}"])
        .write_to_string()
        .into_owned();
    number_unit_pattern_from_placeholder(&rendered).map(|pattern| (pattern, is_localized))
}

fn plural_category_sample(
    rules: &IcuPluralRules,
    expected: crate::PluralCategory,
) -> Option<usize> {
    for sample in 0..=200 {
        if plural_category_from_icu(rules.category_for(sample)) == expected {
            return Some(sample);
        }
    }
    [1_000, 1_000_000]
        .into_iter()
        .find(|sample| plural_category_from_icu(rules.category_for(*sample)) == expected)
}

/// Returns only the cardinal categories that can be selected for a locale.
///
/// Coverage is measured over observable patterns. A category whose plural
/// rule can never return it is not a missing locale-data cell. If ICU lacks a
/// rule for an otherwise advertised NumberFormat locale, retain `other` so
/// the report makes that fallback visible instead of reporting an empty
/// matrix.
fn number_format_plural_categories(locale: &str) -> Vec<crate::PluralCategory> {
    const CATEGORIES: &[crate::PluralCategory] = &[
        crate::PluralCategory::Zero,
        crate::PluralCategory::One,
        crate::PluralCategory::Two,
        crate::PluralCategory::Few,
        crate::PluralCategory::Many,
        crate::PluralCategory::Other,
    ];
    let Ok(locale) = crate::canonicalize(locale) else {
        return vec![crate::PluralCategory::Other];
    };
    let Ok(rules) = IcuPluralRules::try_new_cardinal(locale.locale().into()) else {
        return vec![crate::PluralCategory::Other];
    };
    let categories = CATEGORIES
        .iter()
        .copied()
        .filter(|category| plural_category_sample(&rules, *category).is_some())
        .collect::<Vec<_>>();
    if categories.is_empty() {
        vec![crate::PluralCategory::Other]
    } else {
        categories
    }
}

fn plural_category_from_icu(category: IcuPluralCategory) -> crate::PluralCategory {
    match category {
        IcuPluralCategory::Zero => crate::PluralCategory::Zero,
        IcuPluralCategory::One => crate::PluralCategory::One,
        IcuPluralCategory::Two => crate::PluralCategory::Two,
        IcuPluralCategory::Few => crate::PluralCategory::Few,
        IcuPluralCategory::Many => crate::PluralCategory::Many,
        IcuPluralCategory::Other => crate::PluralCategory::Other,
    }
}

fn plural_category_to_icu(category: crate::PluralCategory) -> IcuPluralCategory {
    match category {
        crate::PluralCategory::Zero => IcuPluralCategory::Zero,
        crate::PluralCategory::One => IcuPluralCategory::One,
        crate::PluralCategory::Two => IcuPluralCategory::Two,
        crate::PluralCategory::Few => IcuPluralCategory::Few,
        crate::PluralCategory::Many => IcuPluralCategory::Many,
        crate::PluralCategory::Other => IcuPluralCategory::Other,
    }
}

fn number_unit_pattern_from_placeholder(rendered: &str) -> Option<NumberUnitPattern> {
    let Some((prefix, suffix)) = rendered.split_once('\u{fdd0}') else {
        return (!rendered.is_empty()).then(|| NumberUnitPattern {
            prefix: String::new(),
            prefix_separator: String::new(),
            suffix_separator: String::new(),
            suffix: rendered.into(),
            hides_number: true,
        });
    };
    let prefix_label = prefix.trim_end();
    let suffix_label = suffix.trim_start();
    Some(NumberUnitPattern {
        prefix: prefix_label.into(),
        prefix_separator: prefix[prefix_label.len()..].into(),
        suffix_separator: suffix[..suffix.len() - suffix_label.len()].into(),
        suffix: suffix_label.into(),
        hides_number: false,
    })
}

/// Returns pinned-CLDR temperature and angle patterns that ICU4X has not yet
/// generated a typed unit-name marker for.
///
/// The records below are copied from the `unitPattern-count-*` entries in the
/// same `cldr-units-full` input revision as the ICU4X bundle. They stay at the
/// locale-data boundary so a missing upstream marker cannot silently turn a
/// non-English NumberFormat unit into English text.
fn cldr_temperature_unit_pattern(
    locale: &str,
    unit: crate::NumberFormatUnit,
    display: crate::NumberUnitDisplay,
    plural: crate::PluralCategory,
) -> Option<NumberUnitPattern> {
    use crate::{NumberFormatUnit as Unit, NumberUnitDisplay as Display, PluralCategory};

    macro_rules! patterns {
        ($($category:ident => $pattern:literal),+ $(,)?) => {
            &[$((PluralCategory::$category, $pattern)),+]
        };
    }

    let language = locale
        .split_once("-u-")
        .map_or(locale, |(base, _)| base)
        .split('-')
        .next()
        .unwrap_or(locale);
    let patterns: &[(PluralCategory, &str)] = match (language, unit, display) {
        ("ar", Unit::Celsius, Display::Long) => patterns!(Other => "{0} درجة مئوية"),
        ("ar", Unit::Fahrenheit, Display::Long) => {
            patterns!(Other => "{0} درجة فهرنهايت")
        }
        ("ar", Unit::Degree, Display::Long | Display::Short) => patterns!(
            Zero => "{0} درجة",
            One => "درجة",
            Two => "درجتان",
            Few => "{0} درجات",
            Many => "{0} درجة",
            Other => "{0} درجة",
        ),
        ("ar", Unit::Degree, Display::Narrow) => patterns!(
            Zero => "{0} درجة",
            One => "{0} درجة",
            Two => "درجتان",
            Few => "{0} درجات",
            Many => "{0} درجة",
            Other => "{0} درجة",
        ),
        ("ar", Unit::Celsius, Display::Short | Display::Narrow) => {
            patterns!(Other => "{0}°م")
        }
        ("ar", Unit::Fahrenheit, Display::Short | Display::Narrow) => {
            patterns!(Other => "{0}°ف")
        }

        ("fr", Unit::Celsius, Display::Long) => patterns!(
            One => "{0}\u{a0}degré Celsius",
            Other => "{0}\u{a0}degrés Celsius",
        ),
        ("fr", Unit::Fahrenheit, Display::Long) => patterns!(
            One => "{0}\u{a0}degré Fahrenheit",
            Other => "{0}\u{a0}degrés Fahrenheit",
        ),
        ("fr", Unit::Degree, Display::Long) => patterns!(
            One => "{0}\u{a0}degré",
            Other => "{0}\u{a0}degrés",
        ),
        ("fr", Unit::Celsius, Display::Short) => patterns!(Other => "{0}\u{202f}°C"),
        ("fr", Unit::Fahrenheit, Display::Short) => patterns!(Other => "{0}\u{202f}°F"),
        ("fr", Unit::Degree, Display::Short | Display::Narrow) => {
            patterns!(Other => "{0}°")
        }
        ("fr", Unit::Celsius, Display::Narrow) => patterns!(Other => "{0}°C"),
        ("fr", Unit::Fahrenheit, Display::Narrow) => patterns!(Other => "{0}°F"),

        ("de", Unit::Celsius, Display::Long) => patterns!(Other => "{0} Grad Celsius"),
        ("de", Unit::Fahrenheit, Display::Long) => {
            patterns!(Other => "{0} Grad Fahrenheit")
        }
        ("de", Unit::Degree, Display::Long) => patterns!(Other => "{0} Grad"),
        ("de", Unit::Celsius, Display::Short | Display::Narrow) => {
            patterns!(Other => "{0} °C")
        }
        ("de", Unit::Fahrenheit, Display::Short) => patterns!(Other => "{0} °F"),
        ("de", Unit::Fahrenheit, Display::Narrow) => patterns!(Other => "{0}°F"),
        ("de", Unit::Degree, Display::Short | Display::Narrow) => patterns!(Other => "{0}°"),

        ("ja", Unit::Celsius, Display::Long) => patterns!(Other => "摂氏 {0} 度"),
        ("ja", Unit::Fahrenheit, Display::Long) => patterns!(Other => "華氏 {0} 度"),
        ("ja", Unit::Degree, Display::Long | Display::Short) => {
            patterns!(Other => "{0} 度")
        }
        ("ja", Unit::Celsius, Display::Short | Display::Narrow) => {
            patterns!(Other => "{0}°C")
        }
        ("ja", Unit::Fahrenheit, Display::Short | Display::Narrow) => {
            patterns!(Other => "{0}°F")
        }
        ("ja", Unit::Degree, Display::Narrow) => patterns!(Other => "{0}°"),

        ("ru", Unit::Celsius, Display::Long) => patterns!(
            One => "{0} градус Цельсия",
            Few => "{0} градуса Цельсия",
            Many => "{0} градусов Цельсия",
            Other => "{0} градуса Цельсия",
        ),
        ("ru", Unit::Fahrenheit, Display::Long) => patterns!(
            One => "{0} градус Фаренгейта",
            Few => "{0} градуса Фаренгейта",
            Many => "{0} градусов Фаренгейта",
            Other => "{0} градуса Фаренгейта",
        ),
        ("ru", Unit::Degree, Display::Long) => patterns!(
            One => "{0} градус",
            Few => "{0} градуса",
            Many => "{0} градусов",
            Other => "{0} градуса",
        ),
        ("ru", Unit::Celsius, Display::Short | Display::Narrow) => {
            patterns!(Other => "{0} °C")
        }
        ("ru", Unit::Fahrenheit, Display::Short) => patterns!(Other => "{0} °F"),
        ("ru", Unit::Fahrenheit, Display::Narrow) => patterns!(Other => "{0}°F"),
        ("ru", Unit::Degree, Display::Short | Display::Narrow) => {
            patterns!(Other => "{0}°")
        }
        _ => return None,
    };
    let raw = patterns
        .iter()
        .find(|(category, _)| *category == plural)
        .or_else(|| {
            patterns
                .iter()
                .find(|(category, _)| *category == PluralCategory::Other)
        })?
        .1;
    number_unit_pattern_from_placeholder(&raw.replace("{0}", "\u{fdd0}"))
}

/// Returns the pinned Japanese CLDR records for digital, percentage, and the
/// long `duration-second` unit needed by generic compounds.
///
/// ICU4X's typed unit-name markers omit these sanctioned categories. The
/// Japanese records differ by width (for example, long `ギガバイト`, short
/// `GB`, and narrow `GB`) and retain the CLDR no-space percentage forms. The
/// long second label is included because the generated marker falls back to
/// English for this locale even though the raw CLDR record is available.
fn cldr_japanese_digital_unit_pattern(
    locale: &str,
    unit: crate::NumberFormatUnit,
    display: crate::NumberUnitDisplay,
    _plural: crate::PluralCategory,
) -> Option<NumberUnitPattern> {
    use crate::{NumberFormatUnit as Unit, NumberUnitDisplay as Display};

    if locale
        .split_once("-u-")
        .map_or(locale, |(base, _)| base)
        .split('-')
        .next()
        != Some("ja")
    {
        return None;
    }

    let (suffix_separator, suffix) = match display {
        Display::Long => (
            " ",
            match unit {
                Unit::Bit => "ビット",
                Unit::Byte => "バイト",
                Unit::Gigabit => "ギガビット",
                Unit::Gigabyte => "ギガバイト",
                Unit::Kilobit => "キロビット",
                Unit::Kilobyte => "キロバイト",
                Unit::Megabit => "メガビット",
                Unit::Megabyte => "メガバイト",
                Unit::Petabyte => "ペタバイト",
                Unit::Second => "秒",
                Unit::Terabit => "テラビット",
                Unit::Terabyte => "テラバイト",
                Unit::Percent => "パーセント",
                _ => return None,
            },
        ),
        Display::Short => match unit {
            Unit::Bit => (" ", "bit"),
            Unit::Byte => (" ", "byte"),
            Unit::Gigabit => (" ", "Gb"),
            Unit::Gigabyte => (" ", "GB"),
            Unit::Kilobit => (" ", "kb"),
            Unit::Kilobyte => (" ", "KB"),
            Unit::Megabit => (" ", "Mb"),
            Unit::Megabyte => (" ", "MB"),
            Unit::Petabyte => (" ", "PB"),
            Unit::Terabit => (" ", "Tb"),
            Unit::Terabyte => (" ", "TB"),
            Unit::Percent => ("", "%"),
            _ => return None,
        },
        Display::Narrow => match unit {
            Unit::Bit => ("", "b"),
            Unit::Byte => ("", "B"),
            Unit::Gigabit => ("", "Gb"),
            Unit::Gigabyte => ("", "GB"),
            Unit::Kilobit => ("", "kb"),
            Unit::Kilobyte => ("", "KB"),
            Unit::Megabit => ("", "Mb"),
            Unit::Megabyte => ("", "MB"),
            Unit::Petabyte => ("", "PB"),
            Unit::Terabit => ("", "Tb"),
            Unit::Terabyte => ("", "TB"),
            Unit::Percent => ("", "%"),
            _ => return None,
        },
    };
    Some(NumberUnitPattern {
        prefix: String::new(),
        prefix_separator: String::new(),
        suffix_separator: suffix_separator.into(),
        suffix: suffix.into(),
        hides_number: false,
    })
}

/// Returns the pinned Korean CLDR records for the simple-unit categories that
/// ICU4X does not expose through a typed unit-name marker.
///
/// Korean has both ordinary suffix forms and the long temperature forms
/// `섭씨 {0}도` / `화씨 {0}도`, so retaining the raw placeholder pattern is
/// necessary for `formatToParts` and range-affix collapsing to preserve the
/// observable prefix/literal/unit boundaries.
fn cldr_korean_additional_unit_pattern(
    locale: &str,
    unit: crate::NumberFormatUnit,
    display: crate::NumberUnitDisplay,
    _plural: crate::PluralCategory,
) -> Option<NumberUnitPattern> {
    use crate::{NumberFormatUnit as Unit, NumberUnitDisplay as Display};

    if locale
        .split_once("-u-")
        .map_or(locale, |(base, _)| base)
        .split('-')
        .next()
        != Some("ko")
    {
        return None;
    }

    let raw = match (unit, display) {
        (Unit::Bit, Display::Long) => "{0}비트",
        (Unit::Byte, Display::Long) => "{0}바이트",
        (Unit::Celsius, Display::Long) => "섭씨 {0}도",
        (Unit::Degree, Display::Long) => "{0}도",
        (Unit::Fahrenheit, Display::Long) => "화씨 {0}도",
        (Unit::Gigabit, Display::Long) => "{0}기가비트",
        (Unit::Gigabyte, Display::Long) => "{0}기가바이트",
        (Unit::Kilobit, Display::Long) => "{0}킬로비트",
        (Unit::Kilobyte, Display::Long) => "{0}킬로바이트",
        (Unit::Megabit, Display::Long) => "{0}메가비트",
        (Unit::Megabyte, Display::Long) => "{0}메가바이트",
        (Unit::Percent, Display::Long) => "{0}%",
        (Unit::Petabyte, Display::Long) => "{0}페타바이트",
        (Unit::Terabit, Display::Long) => "{0}테라비트",
        (Unit::Terabyte, Display::Long) => "{0}테라바이트",

        (Unit::Bit, Display::Short | Display::Narrow) => "{0}bit",
        (Unit::Byte, Display::Short | Display::Narrow) => "{0}byte",
        (Unit::Celsius, Display::Short | Display::Narrow) => "{0}°C",
        (Unit::Degree, Display::Short | Display::Narrow) => "{0}°",
        (Unit::Fahrenheit, Display::Short | Display::Narrow) => "{0}°F",
        (Unit::Gigabit, Display::Short | Display::Narrow) => "{0}Gb",
        (Unit::Gigabyte, Display::Short | Display::Narrow) => "{0}GB",
        (Unit::Kilobit, Display::Short | Display::Narrow) => "{0}kb",
        (Unit::Kilobyte, Display::Short | Display::Narrow) => "{0}kB",
        (Unit::Megabit, Display::Short | Display::Narrow) => "{0}Mb",
        (Unit::Megabyte, Display::Short | Display::Narrow) => "{0}MB",
        (Unit::Percent, Display::Short | Display::Narrow) => "{0}%",
        (Unit::Petabyte, Display::Short | Display::Narrow) => "{0}PB",
        (Unit::Terabit, Display::Short | Display::Narrow) => "{0}Tb",
        (Unit::Terabyte, Display::Short | Display::Narrow) => "{0}TB",
        _ => return None,
    };
    number_unit_pattern_from_placeholder(&raw.replace("{0}", "\u{fdd0}"))
}

#[derive(Clone, Copy)]
enum ChineseUnitVariant {
    Hans,
    Hant,
}

fn chinese_unit_variant(locale: &str) -> Option<ChineseUnitVariant> {
    let base = locale.split_once("-u-").map_or(locale, |(base, _)| base);
    let mut subtags = base.split('-');
    (subtags.next() == Some("zh")).then_some(())?;
    let subtags = subtags.collect::<Vec<_>>();
    if subtags.contains(&"Hant")
        || subtags
            .iter()
            .any(|subtag| matches!(*subtag, "HK" | "MO" | "TW"))
    {
        Some(ChineseUnitVariant::Hant)
    } else {
        Some(ChineseUnitVariant::Hans)
    }
}

/// Returns the pinned Chinese CLDR records for simple units absent from the
/// ICU4X typed marker inventory.
///
/// The CLDR parent `zh` supplies the simplified-Han values, while the
/// traditional-Han branch has observable spacing and leading temperature
/// labels. Resolve script and conventional region before selecting a record,
/// rather than applying the simplified parent to `zh-TW` or `zh-HK`.
fn cldr_chinese_additional_unit_pattern(
    locale: &str,
    unit: crate::NumberFormatUnit,
    display: crate::NumberUnitDisplay,
    _plural: crate::PluralCategory,
) -> Option<NumberUnitPattern> {
    use crate::{NumberFormatUnit as Unit, NumberUnitDisplay as Display};

    let variant = chinese_unit_variant(locale)?;
    let (hans, hant) = match display {
        Display::Long => match unit {
            Unit::Bit => ("{0}比特", "{0} bit"),
            Unit::Byte => ("{0}字节", "{0} byte"),
            Unit::Celsius => ("{0}摄氏度", "攝氏 {0} 度"),
            Unit::Degree => ("{0}度", "{0} 度"),
            Unit::Fahrenheit => ("{0}华氏度", "華氏 {0} 度"),
            Unit::Gigabit => ("{0}吉比特", "{0} Gb"),
            Unit::Gigabyte => ("{0}吉字节", "{0} GB"),
            Unit::Kilobit => ("{0}千比特", "{0} kb"),
            Unit::Kilobyte => ("{0}千字节", "{0} kB"),
            Unit::Megabit => ("{0}兆比特", "{0} Mb"),
            Unit::Megabyte => ("{0}兆字节", "{0} MB"),
            Unit::Percent => ("{0}%", "{0}%"),
            Unit::Petabyte => ("{0}拍字节", "{0} PB"),
            Unit::Terabit => ("{0}太比特", "{0} Tb"),
            Unit::Terabyte => ("{0}太字节", "{0} TB"),
            _ => return None,
        },
        Display::Short => match unit {
            Unit::Bit => ("{0} b", "{0} bit"),
            Unit::Byte => ("{0} B", "{0} byte"),
            Unit::Celsius => ("{0}°C", "{0}°C"),
            Unit::Degree => ("{0}°", "{0} 度"),
            Unit::Fahrenheit => ("{0}°F", "{0}°F"),
            Unit::Gigabit => ("{0} Gb", "{0} Gb"),
            Unit::Gigabyte => ("{0} GB", "{0} GB"),
            Unit::Kilobit => ("{0} kb", "{0} kb"),
            Unit::Kilobyte => ("{0} kB", "{0} kB"),
            Unit::Megabit => ("{0} Mb", "{0} Mb"),
            Unit::Megabyte => ("{0} MB", "{0} MB"),
            Unit::Percent => ("{0}%", "{0}%"),
            Unit::Petabyte => ("{0} PB", "{0} PB"),
            Unit::Terabit => ("{0} Tb", "{0} Tb"),
            Unit::Terabyte => ("{0} TB", "{0} TB"),
            _ => return None,
        },
        Display::Narrow => match unit {
            Unit::Bit => ("{0} b", "{0}bit"),
            Unit::Byte => ("{0} B", "{0}byte"),
            Unit::Celsius => ("{0}°C", "{0}°C"),
            Unit::Degree => ("{0}°", "{0}°"),
            Unit::Fahrenheit => ("{0}°F", "{0}°F"),
            Unit::Gigabit => ("{0} Gb", "{0}Gb"),
            Unit::Gigabyte => ("{0} GB", "{0}GB"),
            Unit::Kilobit => ("{0} kb", "{0}kb"),
            Unit::Kilobyte => ("{0} kB", "{0}kB"),
            Unit::Megabit => ("{0} Mb", "{0}Mb"),
            Unit::Megabyte => ("{0} MB", "{0}MB"),
            Unit::Percent => ("{0}%", "{0}%"),
            Unit::Petabyte => ("{0} PB", "{0}PB"),
            Unit::Terabit => ("{0} Tb", "{0}Tb"),
            Unit::Terabyte => ("{0} TB", "{0}TB"),
            _ => return None,
        },
    };
    let raw = match variant {
        ChineseUnitVariant::Hans => hans,
        ChineseUnitVariant::Hant => hant,
    };
    number_unit_pattern_from_placeholder(&raw.replace("{0}", "\u{fdd0}"))
}

/// Returns the pinned German CLDR digital and percentage unit patterns.
///
/// Selected singular and abbreviated forms have a non-breaking separator,
/// which must remain visible to `formatToParts` rather than falling back to
/// an English label.
fn cldr_german_digital_unit_pattern(
    locale: &str,
    unit: crate::NumberFormatUnit,
    display: crate::NumberUnitDisplay,
    plural: crate::PluralCategory,
) -> Option<NumberUnitPattern> {
    use crate::{NumberFormatUnit as Unit, NumberUnitDisplay as Display, PluralCategory};

    if locale
        .split_once("-u-")
        .map_or(locale, |(base, _)| base)
        .split('-')
        .next()
        != Some("de")
    {
        return None;
    }
    let one = plural == PluralCategory::One;
    let raw = match display {
        Display::Long => match unit {
            Unit::Bit => {
                if one {
                    "{0}\u{a0}Bit"
                } else {
                    "{0} Bit"
                }
            }
            Unit::Byte => {
                if one {
                    "{0}\u{a0}Byte"
                } else {
                    "{0} Byte"
                }
            }
            Unit::Gigabit => {
                if one {
                    "{0}\u{a0}Gigabit"
                } else {
                    "{0} Gigabit"
                }
            }
            Unit::Gigabyte => {
                if one {
                    "{0}\u{a0}Gigabyte"
                } else {
                    "{0} Gigabyte"
                }
            }
            Unit::Kilobit => {
                if one {
                    "{0}\u{a0}Kilobit"
                } else {
                    "{0} Kilobit"
                }
            }
            Unit::Kilobyte => {
                if one {
                    "{0}\u{a0}Kilobyte"
                } else {
                    "{0} Kilobyte"
                }
            }
            Unit::Megabit => {
                if one {
                    "{0}\u{a0}Megabit"
                } else {
                    "{0} Megabit"
                }
            }
            Unit::Megabyte => {
                if one {
                    "{0}\u{a0}Megabyte"
                } else {
                    "{0} Megabyte"
                }
            }
            Unit::Petabyte => "{0} Petabyte",
            Unit::Terabit => {
                if one {
                    "{0}\u{a0}Terabit"
                } else {
                    "{0} Terabit"
                }
            }
            Unit::Terabyte => {
                if one {
                    "{0}\u{a0}Terabyte"
                } else {
                    "{0} Terabyte"
                }
            }
            Unit::Percent => "{0} Prozent",
            _ => return None,
        },
        Display::Short => match unit {
            Unit::Bit => {
                if one {
                    "{0}\u{a0}Bit"
                } else {
                    "{0} Bit"
                }
            }
            Unit::Byte => {
                if one {
                    "{0}\u{a0}Byte"
                } else {
                    "{0} Byte"
                }
            }
            Unit::Gigabit => "{0}\u{a0}Gb",
            Unit::Gigabyte => "{0}\u{a0}GB",
            Unit::Kilobit => "{0} kb",
            Unit::Kilobyte => "{0} kB",
            Unit::Megabit => "{0} Mb",
            Unit::Megabyte => "{0} MB",
            Unit::Petabyte => "{0} PB",
            Unit::Terabit => "{0} Tb",
            Unit::Terabyte => "{0} TB",
            Unit::Percent => "{0} %",
            _ => return None,
        },
        Display::Narrow => match unit {
            Unit::Bit => "{0} b",
            Unit::Byte => "{0} B",
            Unit::Gigabit => "{0}\u{a0}Gb",
            Unit::Gigabyte => "{0}\u{a0}GB",
            Unit::Kilobit => "{0} kb",
            Unit::Kilobyte => "{0} kB",
            Unit::Megabit => "{0} Mb",
            Unit::Megabyte => "{0} MB",
            Unit::Petabyte => "{0} PB",
            Unit::Terabit => "{0} Tb",
            Unit::Terabyte => "{0} TB",
            Unit::Percent => "{0} %",
            _ => return None,
        },
    };
    number_unit_pattern_from_placeholder(&raw.replace("{0}", "\u{fdd0}"))
}

/// Returns the pinned Portuguese CLDR records for the unit categories that
/// ICU4X cannot query through a typed marker.
fn cldr_portuguese_additional_unit_pattern(
    locale: &str,
    unit: crate::NumberFormatUnit,
    display: crate::NumberUnitDisplay,
    plural: crate::PluralCategory,
) -> Option<NumberUnitPattern> {
    use crate::{NumberFormatUnit as Unit, NumberUnitDisplay as Display, PluralCategory};

    if locale
        .split_once("-u-")
        .map_or(locale, |(base, _)| base)
        .split('-')
        .next()
        != Some("pt")
    {
        return None;
    }
    let singular = plural == PluralCategory::One;
    let raw = match display {
        Display::Long => match unit {
            Unit::Bit => {
                if singular {
                    "{0} bit"
                } else {
                    "{0} bits"
                }
            }
            Unit::Byte => {
                if singular {
                    "{0} byte"
                } else {
                    "{0} bytes"
                }
            }
            Unit::Celsius => {
                if singular {
                    "{0} grau Celsius"
                } else {
                    "{0} graus Celsius"
                }
            }
            Unit::Degree => {
                if singular {
                    "{0} grau"
                } else {
                    "{0} graus"
                }
            }
            Unit::Fahrenheit => {
                if singular {
                    "{0} grau Fahrenheit"
                } else {
                    "{0} graus Fahrenheit"
                }
            }
            Unit::Gigabit => {
                if singular {
                    "{0} gigabit"
                } else {
                    "{0} gigabits"
                }
            }
            Unit::Gigabyte => {
                if singular {
                    "{0} gigabyte"
                } else {
                    "{0} gigabytes"
                }
            }
            Unit::Kilobit => {
                if singular {
                    "{0} kilobit"
                } else {
                    "{0} kilobits"
                }
            }
            Unit::Kilobyte => {
                if singular {
                    "{0} kilobyte"
                } else {
                    "{0} kilobytes"
                }
            }
            Unit::Megabit => {
                if singular {
                    "{0} megabit"
                } else {
                    "{0} megabits"
                }
            }
            Unit::Megabyte => {
                if singular {
                    "{0} megabyte"
                } else {
                    "{0} megabytes"
                }
            }
            Unit::Percent => "{0} por cento",
            Unit::Petabyte => {
                if singular {
                    "{0} petabyte"
                } else {
                    "{0} petabytes"
                }
            }
            Unit::Terabit => {
                if singular {
                    "{0} terabit"
                } else {
                    "{0} terabits"
                }
            }
            Unit::Terabyte => {
                if singular {
                    "{0} terabyte"
                } else {
                    "{0} terabytes"
                }
            }
            _ => return None,
        },
        Display::Short => match unit {
            Unit::Bit => "{0} bits",
            Unit::Byte => "{0} bytes",
            Unit::Celsius => "{0} °C",
            Unit::Degree => "{0} °",
            Unit::Fahrenheit => "{0} °F",
            Unit::Gigabit => "{0} Gb",
            Unit::Gigabyte => "{0} GB",
            Unit::Kilobit => "{0} kb",
            Unit::Kilobyte => "{0} kB",
            Unit::Megabit => "{0} Mb",
            Unit::Megabyte => "{0} MB",
            Unit::Percent => "{0}%",
            Unit::Petabyte => "{0} PB",
            Unit::Terabit => "{0} Tb",
            Unit::Terabyte => "{0} TB",
            _ => return None,
        },
        Display::Narrow => match unit {
            Unit::Bit => {
                if singular {
                    "{0} bit"
                } else {
                    "{0} bits"
                }
            }
            Unit::Byte => "{0} B",
            Unit::Celsius => "{0} °C",
            Unit::Degree => "{0} °",
            Unit::Fahrenheit => "{0} °F",
            Unit::Gigabit => "{0} Gb",
            Unit::Gigabyte => "{0} GB",
            Unit::Kilobit => "{0} kb",
            Unit::Kilobyte => "{0} kB",
            Unit::Megabit => "{0} Mb",
            Unit::Megabyte => "{0} MB",
            Unit::Percent => "{0}%",
            Unit::Petabyte => "{0} PB",
            Unit::Terabit => "{0} Tb",
            Unit::Terabyte => "{0} TB",
            _ => return None,
        },
    };
    number_unit_pattern_from_placeholder(&raw.replace("{0}", "\u{fdd0}"))
}

/// Returns the pinned Italian CLDR records for categories absent from ICU4X's
/// typed unit marker inventory.
fn cldr_italian_additional_unit_pattern(
    locale: &str,
    unit: crate::NumberFormatUnit,
    display: crate::NumberUnitDisplay,
    plural: crate::PluralCategory,
) -> Option<NumberUnitPattern> {
    use crate::{NumberFormatUnit as Unit, NumberUnitDisplay as Display, PluralCategory};

    if locale
        .split_once("-u-")
        .map_or(locale, |(base, _)| base)
        .split('-')
        .next()
        != Some("it")
    {
        return None;
    }
    let singular = plural == PluralCategory::One;
    let raw = match display {
        Display::Long => match unit {
            Unit::Bit => "{0} bit",
            Unit::Byte => "{0} byte",
            Unit::Celsius => {
                if singular {
                    "{0} grado Celsius"
                } else {
                    "{0} gradi Celsius"
                }
            }
            Unit::Degree => {
                if singular {
                    "{0} grado"
                } else {
                    "{0} gradi"
                }
            }
            Unit::Fahrenheit => {
                if singular {
                    "{0} grado Fahrenheit"
                } else {
                    "{0} gradi Fahrenheit"
                }
            }
            Unit::Gigabit => "{0} gigabit",
            Unit::Gigabyte => "{0} gigabyte",
            Unit::Kilobit => "{0} kilobit",
            Unit::Kilobyte => "{0} kilobyte",
            Unit::Megabit => "{0} megabit",
            Unit::Megabyte => "{0} megabyte",
            Unit::Percent => "{0} percento",
            Unit::Petabyte => "{0} petabyte",
            Unit::Terabit => "{0} terabit",
            Unit::Terabyte => "{0} terabyte",
            _ => return None,
        },
        Display::Short => match unit {
            Unit::Bit => "{0} bit",
            Unit::Byte => "{0} byte",
            Unit::Celsius => "{0} °C",
            Unit::Degree => "{0}°",
            Unit::Fahrenheit => "{0} °F",
            Unit::Gigabit => "{0} Gb",
            Unit::Gigabyte => "{0} GB",
            Unit::Kilobit => "{0} kb",
            Unit::Kilobyte => "{0} kB",
            Unit::Megabit => "{0} Mb",
            Unit::Megabyte => "{0} MB",
            Unit::Percent => "{0}%",
            Unit::Petabyte => "{0} PB",
            Unit::Terabit => "{0} Tb",
            Unit::Terabyte => "{0} TB",
            _ => return None,
        },
        Display::Narrow => match unit {
            Unit::Bit => "{0}bit",
            Unit::Byte => "{0}B",
            Unit::Celsius => "{0}°C",
            Unit::Degree => "{0}°",
            Unit::Fahrenheit => "{0}°F",
            Unit::Gigabit => "{0}Gb",
            Unit::Gigabyte => "{0}GB",
            Unit::Kilobit => "{0}kb",
            Unit::Kilobyte => "{0}kB",
            Unit::Megabit => "{0}Mb",
            Unit::Megabyte => "{0}MB",
            Unit::Percent => "{0}%",
            Unit::Petabyte => "{0}PB",
            Unit::Terabit => "{0}Tb",
            Unit::Terabyte => "{0}TB",
            _ => return None,
        },
    };
    number_unit_pattern_from_placeholder(&raw.replace("{0}", "\u{fdd0}"))
}

/// Returns pinned Dutch CLDR unit records for categories without a typed
/// ICU4X marker. Dutch distinguishes singular digital-bit and several
/// temperature and angular long forms, so the provider's English fallback
/// cannot faithfully stand in for these values.
fn cldr_dutch_additional_unit_pattern(
    locale: &str,
    unit: crate::NumberFormatUnit,
    display: crate::NumberUnitDisplay,
    plural: crate::PluralCategory,
) -> Option<NumberUnitPattern> {
    use crate::{NumberFormatUnit as Unit, NumberUnitDisplay as Display, PluralCategory};

    if locale
        .split_once("-u-")
        .map_or(locale, |(base, _)| base)
        .split('-')
        .next()
        != Some("nl")
    {
        return None;
    }
    let singular = plural == PluralCategory::One;
    let raw = match display {
        Display::Long => match unit {
            Unit::Bit => {
                if singular {
                    "{0} bit"
                } else {
                    "{0} bits"
                }
            }
            Unit::Byte => "{0} byte",
            Unit::Celsius => {
                if singular {
                    "{0} graad Celsius"
                } else {
                    "{0} graden Celsius"
                }
            }
            Unit::Degree => {
                if singular {
                    "{0} booggraad"
                } else {
                    "{0} booggraden"
                }
            }
            Unit::Fahrenheit => {
                if singular {
                    "{0} graad Fahrenheit"
                } else {
                    "{0} graden Fahrenheit"
                }
            }
            Unit::Gigabit => {
                if singular {
                    "{0} gigabit"
                } else {
                    "{0} gigabits"
                }
            }
            Unit::Gigabyte => "{0} gigabyte",
            Unit::Kilobit => {
                if singular {
                    "{0} kilobit"
                } else {
                    "{0} kilobits"
                }
            }
            Unit::Kilobyte => "{0} kilobyte",
            Unit::Megabit => {
                if singular {
                    "{0} megabit"
                } else {
                    "{0} megabits"
                }
            }
            Unit::Megabyte => "{0} megabyte",
            Unit::Percent => "{0} procent",
            Unit::Petabyte => "{0} petabyte",
            Unit::Terabit => {
                if singular {
                    "{0} terabit"
                } else {
                    "{0} terabits"
                }
            }
            Unit::Terabyte => "{0} terabyte",
            _ => return None,
        },
        Display::Short => match unit {
            Unit::Bit => {
                if singular {
                    "{0} bit"
                } else {
                    "{0} bits"
                }
            }
            Unit::Byte => "{0} byte",
            Unit::Celsius => "{0}°C",
            Unit::Degree => "{0}°",
            Unit::Fahrenheit => "{0}°F",
            Unit::Gigabit => "{0} Gb",
            Unit::Gigabyte => "{0} GB",
            Unit::Kilobit => "{0} kb",
            Unit::Kilobyte => "{0} kB",
            Unit::Megabit => "{0} Mb",
            Unit::Megabyte => "{0} MB",
            Unit::Percent => "{0}%",
            Unit::Petabyte => "{0} PB",
            Unit::Terabit => "{0} Tb",
            Unit::Terabyte => "{0} TB",
            _ => return None,
        },
        Display::Narrow => match unit {
            Unit::Bit => {
                if singular {
                    "{0} bit"
                } else {
                    "{0} bits"
                }
            }
            Unit::Byte => "{0} byte",
            Unit::Celsius => "{0}°",
            Unit::Degree => "{0}°",
            Unit::Fahrenheit => "{0}°F",
            Unit::Gigabit => "{0} Gb",
            Unit::Gigabyte => "{0} GB",
            Unit::Kilobit => "{0} kb",
            Unit::Kilobyte => "{0} kB",
            Unit::Megabit => "{0} Mb",
            Unit::Megabyte => "{0} MB",
            Unit::Percent => "{0}%",
            Unit::Petabyte => "{0} PB",
            Unit::Terabit => "{0} Tb",
            Unit::Terabyte => "{0} TB",
            _ => return None,
        },
    };
    number_unit_pattern_from_placeholder(&raw.replace("{0}", "\u{fdd0}"))
}

/// Returns the pinned Russian CLDR records for digital and percentage units.
///
/// The missing typed ICU4X category cannot be approximated with English
/// singular/plural forms: Russian uses `one`, `few`, `many`, and `other`
/// suffixes. Long forms below are the corresponding `unitPattern-count-*`
/// values from the pinned CLDR input; short and narrow preserve its localized
/// abbreviations and percentage spacing.
fn cldr_russian_digital_unit_pattern(
    locale: &str,
    unit: crate::NumberFormatUnit,
    display: crate::NumberUnitDisplay,
    plural: crate::PluralCategory,
) -> Option<NumberUnitPattern> {
    use crate::{NumberFormatUnit as Unit, NumberUnitDisplay as Display, PluralCategory};

    if locale
        .split_once("-u-")
        .map_or(locale, |(base, _)| base)
        .split('-')
        .next()
        != Some("ru")
    {
        return None;
    }

    let (suffix_separator, suffix) = match display {
        Display::Long => {
            let (one, few, many) = match unit {
                Unit::Bit => ("бит", "бита", "бит"),
                Unit::Byte => ("байт", "байта", "байт"),
                Unit::Gigabit => ("гигабит", "гигабита", "гигабит"),
                Unit::Gigabyte => ("гигабайт", "гигабайта", "гигабайт"),
                Unit::Kilobit => ("килобит", "килобита", "килобит"),
                Unit::Kilobyte => ("килобайт", "килобайта", "килобайт"),
                Unit::Megabit => ("мегабит", "мегабита", "мегабит"),
                Unit::Megabyte => ("мегабайт", "мегабайта", "мегабайт"),
                Unit::Petabyte => ("петабайт", "петабайта", "петабайт"),
                Unit::Terabit => ("терабит", "терабита", "терабит"),
                Unit::Terabyte => ("терабайт", "терабайта", "терабайт"),
                Unit::Percent => ("процент", "процента", "процентов"),
                _ => return None,
            };
            (
                " ",
                match plural {
                    PluralCategory::One => one,
                    PluralCategory::Two | PluralCategory::Few | PluralCategory::Other => few,
                    PluralCategory::Zero | PluralCategory::Many => many,
                },
            )
        }
        Display::Short | Display::Narrow => {
            let suffix = match unit {
                Unit::Bit => match plural {
                    PluralCategory::One | PluralCategory::Zero | PluralCategory::Many => "бит",
                    PluralCategory::Two | PluralCategory::Few | PluralCategory::Other => "бита",
                },
                Unit::Byte => "Б",
                Unit::Gigabit => "Гбит",
                Unit::Gigabyte => "ГБ",
                Unit::Kilobit => "кбит",
                Unit::Kilobyte => "кБ",
                Unit::Megabit => "Мбит",
                Unit::Megabyte => "МБ",
                Unit::Petabyte => "ПБ",
                Unit::Terabit => "Тбит",
                Unit::Terabyte => "ТБ",
                Unit::Percent => "%",
                _ => return None,
            };
            (
                if unit == Unit::Percent && display == Display::Narrow {
                    ""
                } else {
                    " "
                },
                suffix,
            )
        }
    };
    Some(NumberUnitPattern {
        prefix: String::new(),
        prefix_separator: String::new(),
        suffix_separator: suffix_separator.into(),
        suffix: suffix.into(),
        hides_number: false,
    })
}

/// Returns the pinned Arabic CLDR records for digital and percentage units.
///
/// The typed ICU4X unit-name markers omit these categories. Arabic is kept
/// separate from a transliterated fallback because its CLDR records retain the
/// Arabic percent sign, narrow digital abbreviations, and the long-width
/// one/other percentage distinction.
fn cldr_arabic_digital_unit_pattern(
    locale: &str,
    unit: crate::NumberFormatUnit,
    display: crate::NumberUnitDisplay,
    plural: crate::PluralCategory,
) -> Option<NumberUnitPattern> {
    use crate::{NumberFormatUnit as Unit, NumberUnitDisplay as Display, PluralCategory};

    if locale
        .split_once("-u-")
        .map_or(locale, |(base, _)| base)
        .split('-')
        .next()
        != Some("ar")
    {
        return None;
    }

    let suffix = match (unit, display) {
        (Unit::Bit, _) => "بت",
        (Unit::Byte, Display::Long | Display::Short) => "بايت",
        (Unit::Byte, Display::Narrow) => "ب",
        (Unit::Gigabit, Display::Long | Display::Short) => "غيغابت",
        (Unit::Gigabit, Display::Narrow) => "غ.بت",
        (Unit::Gigabyte, Display::Long) => "غيغابايت",
        (Unit::Gigabyte, Display::Short | Display::Narrow) => "غ.ب",
        (Unit::Kilobit, Display::Long | Display::Short) => "كيلوبت",
        (Unit::Kilobit, Display::Narrow) => "ك.بت",
        (Unit::Kilobyte, Display::Long | Display::Short) => "كيلوبايت",
        (Unit::Kilobyte, Display::Narrow) => "ك.ب",
        (Unit::Megabit, Display::Long | Display::Short) => "ميغابت",
        (Unit::Megabit, Display::Narrow) => "م.بت",
        (Unit::Megabyte, Display::Long | Display::Short) => "ميغابايت",
        (Unit::Megabyte, Display::Narrow) => "م.ب",
        (Unit::Percent, Display::Long)
            if matches!(plural, PluralCategory::One | PluralCategory::Other) =>
        {
            "بالمائة"
        }
        (Unit::Percent, _) => "٪",
        (Unit::Petabyte, _) => "بيتابايت",
        (Unit::Terabit, Display::Long | Display::Short) => "تيرابت",
        (Unit::Terabit, Display::Narrow) => "ت.بت",
        (Unit::Terabyte, Display::Long | Display::Short) => "تيرابايت",
        (Unit::Terabyte, Display::Narrow) => "ت.ب",
        _ => return None,
    };
    Some(NumberUnitPattern {
        prefix: String::new(),
        prefix_separator: String::new(),
        suffix_separator: " ".into(),
        suffix: suffix.into(),
        hides_number: false,
    })
}

/// Returns the pinned French digital and percentage unit patterns omitted by
/// ICU4X's typed unit-name categories.
fn cldr_french_digital_unit_pattern(
    locale: &str,
    unit: crate::NumberFormatUnit,
    display: crate::NumberUnitDisplay,
    plural: crate::PluralCategory,
) -> Option<NumberUnitPattern> {
    use crate::{NumberFormatUnit as Unit, NumberUnitDisplay as Display, PluralCategory};

    if locale
        .split_once("-u-")
        .map_or(locale, |(base, _)| base)
        .split('-')
        .next()
        != Some("fr")
    {
        return None;
    }
    let one = plural == PluralCategory::One;
    let (suffix, suffix_separator) = match display {
        Display::Long => {
            let (singular, plural) = match unit {
                Unit::Bit => ("bit", "bits"),
                Unit::Byte => ("octet", "octets"),
                Unit::Gigabit => ("gigabit", "gigabits"),
                Unit::Gigabyte => ("gigaoctet", "gigaoctets"),
                Unit::Kilobit => ("kilobit", "kilobits"),
                Unit::Kilobyte => ("kilooctet", "kilooctets"),
                Unit::Megabit => ("mégabit", "mégabits"),
                Unit::Megabyte => ("mégaoctet", "mégaoctets"),
                Unit::Petabyte => ("pétaoctet", "pétaoctets"),
                Unit::Terabit => ("térabit", "térabits"),
                Unit::Terabyte => ("téraoctet", "téraoctets"),
                Unit::Percent => ("pour cent", "pour cent"),
                _ => return None,
            };
            (
                if one { singular } else { plural },
                if unit == Unit::Percent { " " } else { "\u{a0}" },
            )
        }
        Display::Short => {
            let suffix = match unit {
                Unit::Bit => "bit",
                Unit::Byte => "o",
                Unit::Gigabit => "Gbit",
                Unit::Gigabyte => "Go",
                Unit::Kilobit => "kbit",
                Unit::Kilobyte => "ko",
                Unit::Megabit => "Mbit",
                Unit::Megabyte => "Mo",
                Unit::Petabyte => "Po",
                Unit::Terabit => "Tbit",
                Unit::Terabyte => "To",
                Unit::Percent => "%",
                _ => return None,
            };
            (
                suffix,
                if unit == Unit::Percent || (unit == Unit::Terabit && !one) {
                    " "
                } else {
                    "\u{202f}"
                },
            )
        }
        Display::Narrow => {
            let suffix = match unit {
                Unit::Bit => "bit",
                Unit::Byte => "o",
                Unit::Gigabit => "Gbit",
                Unit::Gigabyte => "Go",
                Unit::Kilobit => "kbit",
                Unit::Kilobyte => "ko",
                Unit::Megabit => "Mbit",
                Unit::Megabyte => "Mo",
                Unit::Petabyte => "Po",
                Unit::Terabit => "Tbit",
                Unit::Terabyte => "To",
                Unit::Percent => "%",
                _ => return None,
            };
            (suffix, if unit == Unit::Percent { " " } else { "" })
        }
    };
    Some(NumberUnitPattern {
        prefix: String::new(),
        prefix_separator: String::new(),
        suffix_separator: suffix_separator.into(),
        suffix: suffix.into(),
        hides_number: false,
    })
}

/// Returns the pinned Spanish CLDR records for every sanctioned simple-unit
/// category that ICU4X does not yet expose through a typed unit-name marker.
///
/// This completes Spanish simple-unit coverage rather than mixing generated
/// Spanish area/length/etc. names with English digital, temperature, angle,
/// or percentage labels. The values are the `unitPattern-count-*` records
/// from `cldr-units-full/main/es/units.json` at the provider's pinned source
/// revision.
fn cldr_spanish_additional_unit_pattern(
    locale: &str,
    unit: crate::NumberFormatUnit,
    display: crate::NumberUnitDisplay,
    plural: crate::PluralCategory,
) -> Option<NumberUnitPattern> {
    use crate::{NumberFormatUnit as Unit, NumberUnitDisplay as Display, PluralCategory};

    if locale
        .split_once("-u-")
        .map_or(locale, |(base, _)| base)
        .split('-')
        .next()
        != Some("es")
    {
        return None;
    }

    let one = plural == PluralCategory::One;
    let (suffix_separator, suffix) = match display {
        Display::Long => {
            let (singular, plural) = match unit {
                Unit::Bit => ("bit", "bits"),
                Unit::Byte => ("byte", "bytes"),
                Unit::Celsius => ("grado Celsius", "grados Celsius"),
                Unit::Degree => ("grado", "grados"),
                Unit::Fahrenheit => ("grado Fahrenheit", "grados Fahrenheit"),
                Unit::Gigabit => ("gigabit", "gigabits"),
                Unit::Gigabyte => ("gigabyte", "gigabytes"),
                Unit::Kilobit => ("kilobit", "kilobits"),
                Unit::Kilobyte => ("kilobyte", "kilobytes"),
                Unit::Megabit => ("megabit", "megabits"),
                Unit::Megabyte => ("megabyte", "megabytes"),
                Unit::Percent => ("por ciento", "por ciento"),
                Unit::Petabyte => ("petabyte", "petabytes"),
                Unit::Terabit => ("terabit", "terabits"),
                Unit::Terabyte => ("terabyte", "terabytes"),
                _ => return None,
            };
            (" ", if one { singular } else { plural })
        }
        Display::Short => {
            let (separator, suffix) = match unit {
                Unit::Bit => (" ", "b"),
                Unit::Byte => (" ", "B"),
                Unit::Celsius => (" ", "°C"),
                Unit::Degree => ("", "°"),
                Unit::Fahrenheit => (" ", "°F"),
                Unit::Gigabit => (" ", "Gb"),
                Unit::Gigabyte => (" ", "GB"),
                Unit::Kilobit => (" ", "kb"),
                Unit::Kilobyte => (" ", "kB"),
                Unit::Megabit => (" ", "Mb"),
                Unit::Megabyte => (" ", "MB"),
                Unit::Percent => ("\u{a0}", "%"),
                Unit::Petabyte => (" ", "PB"),
                Unit::Terabit => (" ", "Tb"),
                Unit::Terabyte => (" ", "TB"),
                _ => return None,
            };
            (separator, suffix)
        }
        Display::Narrow => {
            let suffix = match unit {
                Unit::Bit => "b",
                Unit::Byte => "B",
                Unit::Celsius => "°C",
                Unit::Degree => "°",
                Unit::Fahrenheit => "°F",
                Unit::Gigabit => "Gb",
                Unit::Gigabyte => "GB",
                Unit::Kilobit => "kb",
                Unit::Kilobyte => "kB",
                Unit::Megabit => "Mb",
                Unit::Megabyte => "MB",
                Unit::Percent => "%",
                Unit::Petabyte => "PB",
                Unit::Terabit => "Tb",
                Unit::Terabyte => "TB",
                _ => return None,
            };
            ("", suffix)
        }
    };

    Some(NumberUnitPattern {
        prefix: String::new(),
        prefix_separator: String::new(),
        suffix_separator: suffix_separator.into(),
        suffix: suffix.into(),
        hides_number: false,
    })
}

fn number_unit_pattern_label(pattern: &NumberUnitPattern) -> String {
    format!("{}{}", pattern.prefix, pattern.suffix)
}

fn english_number_unit_pattern(
    unit: crate::NumberFormatUnit,
    display: crate::NumberUnitDisplay,
    singular: bool,
) -> NumberUnitPattern {
    use crate::{NumberFormatUnit as Unit, NumberUnitDisplay};

    let (separator, suffix) = match display {
        NumberUnitDisplay::Long => (
            " ",
            match (unit, singular) {
                (Unit::Acre, true) => "acre",
                (Unit::Acre, false) => "acres",
                (Unit::Bit, true) => "bit",
                (Unit::Bit, false) => "bits",
                (Unit::Byte, true) => "byte",
                (Unit::Byte, false) => "bytes",
                (Unit::Celsius, true) => "degree Celsius",
                (Unit::Celsius, false) => "degrees Celsius",
                (Unit::Centimeter, true) => "centimeter",
                (Unit::Centimeter, false) => "centimeters",
                (Unit::Day, true) => "day",
                (Unit::Day, false) => "days",
                (Unit::Degree, true) => "degree",
                (Unit::Degree, false) => "degrees",
                (Unit::Fahrenheit, true) => "degree Fahrenheit",
                (Unit::Fahrenheit, false) => "degrees Fahrenheit",
                (Unit::FluidOunce, true) => "fluid ounce",
                (Unit::FluidOunce, false) => "fluid ounces",
                (Unit::Foot, true) => "foot",
                (Unit::Foot, false) => "feet",
                (Unit::Gallon, true) => "gallon",
                (Unit::Gallon, false) => "gallons",
                (Unit::Gigabit, true) => "gigabit",
                (Unit::Gigabit, false) => "gigabits",
                (Unit::Gigabyte, true) => "gigabyte",
                (Unit::Gigabyte, false) => "gigabytes",
                (Unit::Gram, true) => "gram",
                (Unit::Gram, false) => "grams",
                (Unit::Hectare, true) => "hectare",
                (Unit::Hectare, false) => "hectares",
                (Unit::Hour, true) => "hour",
                (Unit::Hour, false) => "hours",
                (Unit::Inch, true) => "inch",
                (Unit::Inch, false) => "inches",
                (Unit::Kilobit, true) => "kilobit",
                (Unit::Kilobit, false) => "kilobits",
                (Unit::Kilobyte, true) => "kilobyte",
                (Unit::Kilobyte, false) => "kilobytes",
                (Unit::Kilogram, true) => "kilogram",
                (Unit::Kilogram, false) => "kilograms",
                (Unit::Kilometer, true) => "kilometer",
                (Unit::Kilometer, false) => "kilometers",
                (Unit::Liter, true) => "liter",
                (Unit::Liter, false) => "liters",
                (Unit::Megabit, true) => "megabit",
                (Unit::Megabit, false) => "megabits",
                (Unit::Megabyte, true) => "megabyte",
                (Unit::Megabyte, false) => "megabytes",
                (Unit::Meter, true) => "meter",
                (Unit::Meter, false) => "meters",
                (Unit::Microsecond, true) => "microsecond",
                (Unit::Microsecond, false) => "microseconds",
                (Unit::Mile, true) => "mile",
                (Unit::Mile, false) => "miles",
                (Unit::MileScandinavian, true) => "mile-scandinavian",
                (Unit::MileScandinavian, false) => "miles-scandinavian",
                (Unit::Milliliter, true) => "milliliter",
                (Unit::Milliliter, false) => "milliliters",
                (Unit::Millimeter, true) => "millimeter",
                (Unit::Millimeter, false) => "millimeters",
                (Unit::Millisecond, true) => "millisecond",
                (Unit::Millisecond, false) => "milliseconds",
                (Unit::Minute, true) => "minute",
                (Unit::Minute, false) => "minutes",
                (Unit::Month, true) => "month",
                (Unit::Month, false) => "months",
                (Unit::Nanosecond, true) => "nanosecond",
                (Unit::Nanosecond, false) => "nanoseconds",
                (Unit::Ounce, true) => "ounce",
                (Unit::Ounce, false) => "ounces",
                (Unit::Percent, _) => "percent",
                (Unit::Petabyte, true) => "petabyte",
                (Unit::Petabyte, false) => "petabytes",
                (Unit::Pound, true) => "pound",
                (Unit::Pound, false) => "pounds",
                (Unit::Second, true) => "second",
                (Unit::Second, false) => "seconds",
                (Unit::Stone, true) => "stone",
                (Unit::Stone, false) => "stones",
                (Unit::Terabit, true) => "terabit",
                (Unit::Terabit, false) => "terabits",
                (Unit::Terabyte, true) => "terabyte",
                (Unit::Terabyte, false) => "terabytes",
                (Unit::Week, true) => "week",
                (Unit::Week, false) => "weeks",
                (Unit::Yard, true) => "yard",
                (Unit::Yard, false) => "yards",
                (Unit::Year, true) => "year",
                (Unit::Year, false) => "years",
                (Unit::CompoundPer { .. }, _) => unit.as_str(),
            },
        ),
        NumberUnitDisplay::Short => (
            if unit == Unit::Percent { "" } else { " " },
            match (unit, singular) {
                (Unit::Acre, _) => "ac",
                (Unit::Bit, _) => "bit",
                (Unit::Byte, _) => "byte",
                (Unit::Celsius, _) => "°C",
                (Unit::Centimeter, _) => "cm",
                (Unit::Day, true) => "day",
                (Unit::Day, false) => "days",
                (Unit::Degree, _) => "deg",
                (Unit::Fahrenheit, _) => "°F",
                (Unit::FluidOunce, _) => "fl oz",
                (Unit::Foot, _) => "ft",
                (Unit::Gallon, _) => "gal",
                (Unit::Gigabit, _) => "Gb",
                (Unit::Gigabyte, _) => "GB",
                (Unit::Gram, _) => "g",
                (Unit::Hectare, _) => "ha",
                (Unit::Hour, _) => "hr",
                (Unit::Inch, _) => "in",
                (Unit::Kilobit, _) => "kb",
                (Unit::Kilobyte, _) => "kB",
                (Unit::Kilogram, _) => "kg",
                (Unit::Kilometer, _) => "km",
                (Unit::Liter, _) => "L",
                (Unit::Megabit, _) => "Mb",
                (Unit::Megabyte, _) => "MB",
                (Unit::Meter, _) => "m",
                (Unit::Microsecond, _) => "μs",
                (Unit::Mile, _) => "mi",
                (Unit::MileScandinavian, _) => "smi",
                (Unit::Milliliter, _) => "mL",
                (Unit::Millimeter, _) => "mm",
                (Unit::Millisecond, _) => "ms",
                (Unit::Minute, _) => "min",
                (Unit::Month, true) => "mth",
                (Unit::Month, false) => "mths",
                (Unit::Nanosecond, _) => "ns",
                (Unit::Ounce, _) => "oz",
                (Unit::Percent, _) => "%",
                (Unit::Petabyte, _) => "PB",
                (Unit::Pound, _) => "lb",
                (Unit::Second, _) => "sec",
                (Unit::Stone, _) => "st",
                (Unit::Terabit, _) => "Tb",
                (Unit::Terabyte, _) => "TB",
                (Unit::Week, true) => "wk",
                (Unit::Week, false) => "wks",
                (Unit::Yard, _) => "yd",
                (Unit::Year, true) => "yr",
                (Unit::Year, false) => "yrs",
                (Unit::CompoundPer { .. }, _) => unit.as_str(),
            },
        ),
        NumberUnitDisplay::Narrow => (
            "",
            match unit {
                Unit::Acre => "ac",
                Unit::Bit => "bit",
                Unit::Byte => "B",
                Unit::Celsius => "°C",
                Unit::Centimeter => "cm",
                Unit::Day => "d",
                Unit::Degree => "°",
                Unit::Fahrenheit => "°F",
                Unit::FluidOunce => "fl oz",
                Unit::Foot => "′",
                Unit::Gallon => "gal",
                Unit::Gigabit => "Gb",
                Unit::Gigabyte => "GB",
                Unit::Gram => "g",
                Unit::Hectare => "ha",
                Unit::Hour => "h",
                Unit::Inch => "″",
                Unit::Kilobit => "kb",
                Unit::Kilobyte => "kB",
                Unit::Kilogram => "kg",
                Unit::Kilometer => "km",
                Unit::Liter => "L",
                Unit::Megabit => "Mb",
                Unit::Megabyte => "MB",
                Unit::Meter => "m",
                Unit::Microsecond => "μs",
                Unit::Mile => "mi",
                Unit::MileScandinavian => "smi",
                Unit::Milliliter => "mL",
                Unit::Millimeter => "mm",
                Unit::Millisecond => "ms",
                Unit::Minute => "m",
                Unit::Month => "m",
                Unit::Nanosecond => "ns",
                Unit::Ounce => "oz",
                Unit::Percent => "%",
                Unit::Petabyte => "PB",
                Unit::Pound => "#",
                Unit::Second => "s",
                Unit::Stone => "st",
                Unit::Terabit => "Tb",
                Unit::Terabyte => "TB",
                Unit::Week => "w",
                Unit::Yard => "yd",
                Unit::Year => "y",
                Unit::CompoundPer { .. } => unit.as_str(),
            },
        ),
    };
    NumberUnitPattern {
        prefix: String::new(),
        prefix_separator: String::new(),
        suffix_separator: separator.into(),
        suffix: suffix.into(),
        hides_number: false,
    }
}

const fn contains(values: &[&str], value: &str) -> bool {
    let mut index = 0;
    while index < values.len() {
        if str_eq(values[index], value) {
            return true;
        }
        index += 1;
    }
    false
}

const fn str_eq(left: &str, right: &str) -> bool {
    let left = left.as_bytes();
    let right = right.as_bytes();
    if left.len() != right.len() {
        return false;
    }
    let mut index = 0;
    while index < left.len() {
        if left[index] != right[index] {
            return false;
        }
        index += 1;
    }
    true
}

fn canonical_time_zone(identifier: &str) -> &str {
    match identifier {
        "Etc/GMT" | "Etc/GMT0" | "Etc/UTC" | "GMT" | "GMT0" => "UTC",
        _ => identifier,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn advertised_number_format_simple_unit_provider_matrix_is_total() {
        // Exercise the provider path itself, rather than only the formatter's
        // ICU4X typed-marker fast path. This keeps the raw CLDR supplements
        // and the bounded English compatibility result total for every
        // accepted API plural category.
        const DISPLAYS: [crate::NumberUnitDisplay; 3] = [
            crate::NumberUnitDisplay::Long,
            crate::NumberUnitDisplay::Short,
            crate::NumberUnitDisplay::Narrow,
        ];
        const PLURALS: [crate::PluralCategory; 6] = [
            crate::PluralCategory::Zero,
            crate::PluralCategory::One,
            crate::PluralCategory::Two,
            crate::PluralCategory::Few,
            crate::PluralCategory::Many,
            crate::PluralCategory::Other,
        ];

        let provider = locale_data_provider();
        let mut cells = 0usize;
        for &locale in provider.number_format_locales() {
            for &unit in crate::NumberFormatUnit::ALL {
                for display in DISPLAYS {
                    for plural in PLURALS {
                        let (pattern, _) = provider
                            .number_unit_pattern_with_provenance(locale, unit, display, plural);
                        assert!(
                            !pattern.prefix.contains('\u{fdd0}')
                                && !pattern.suffix.contains('\u{fdd0}'),
                            "{locale} {} {display:?} {plural:?} leaked the private number placeholder",
                            unit.as_str()
                        );
                        cells += 1;
                    }
                }
            }
        }
        assert_eq!(
            cells,
            provider.number_format_locales().len()
                * crate::NumberFormatUnit::ALL.len()
                * DISPLAYS.len()
                * PLURALS.len()
        );
    }
}
