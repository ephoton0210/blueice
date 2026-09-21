// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

//! Version-pinned, host-neutral locale data used by the ECMA-402 services.
//!
//! ICU4X's compiled data supplies the localized algorithms. This provider is
//! the single place where BlueIce declares the subset it exposes through
//! ECMA-402 and the deterministic fallbacks around that data. It deliberately
//! does not negotiate locales: `ResolveLocale` remains a separate layer.

use fixed_decimal::{Decimal, FloatPrecision};
use icu_experimental::{
    dimension::{
        currency::CurrencyType,
        provider::{currency::fractions::CurrencyFractionsV1, percent::PercentEssentialsV1},
    },
    provider::Baked as ExperimentalData,
};
use icu_locale_core::Locale as IcuLocale;
use icu_plurals::{
    PluralCategory as IcuPluralCategory, PluralRules as IcuPluralRules,
    PluralRulesWithRanges as IcuPluralRulesWithRanges,
};
use icu_provider::{DataIdentifierBorrowed, DataProvider, DataRequest};
use writeable::Writeable;

mod compact_patterns;
mod currency_patterns;
mod date_time_formats;
mod decimal_symbols;
mod display_names;
pub(crate) mod list_patterns;
mod locale_information_data;
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

/// ISO 4217 codes with pinned CLDR `Intl.DisplayNames` and NumberFormat data.
///
/// This covers all 307 canonical code records carried by the co-pinned CLDR
/// 48 currency-name data, including historical, fund, and special-purpose
/// codes. `Intl.supportedValuesOf("currency")` must expose every code for
/// which both services provide functionality, not merely current tender.
pub const SUPPORTED_CURRENCIES: &[&str] = &[
    "ADP", "AED", "AFA", "AFN", "ALK", "ALL", "AMD", "ANG", "AOA", "AOK", "AON", "AOR", "ARA",
    "ARL", "ARM", "ARP", "ARS", "ATS", "AUD", "AWG", "AZM", "AZN", "BAD", "BAM", "BAN", "BBD",
    "BDT", "BEC", "BEF", "BEL", "BGL", "BGM", "BGN", "BGO", "BHD", "BIF", "BMD", "BND", "BOB",
    "BOL", "BOP", "BOV", "BRB", "BRC", "BRE", "BRL", "BRN", "BRR", "BRZ", "BSD", "BTN", "BUK",
    "BWP", "BYB", "BYN", "BYR", "BZD", "CAD", "CDF", "CHE", "CHF", "CHW", "CLE", "CLF", "CLP",
    "CNH", "CNX", "CNY", "COP", "COU", "CRC", "CSD", "CSK", "CUC", "CUP", "CVE", "CYP", "CZK",
    "DDM", "DEM", "DJF", "DKK", "DOP", "DZD", "ECS", "ECV", "EEK", "EGP", "ERN", "ESA", "ESB",
    "ESP", "ETB", "EUR", "FIM", "FJD", "FKP", "FRF", "GBP", "GEK", "GEL", "GHC", "GHS", "GIP",
    "GMD", "GNF", "GNS", "GQE", "GRD", "GTQ", "GWE", "GWP", "GYD", "HKD", "HNL", "HRD", "HRK",
    "HTG", "HUF", "IDR", "IEP", "ILP", "ILR", "ILS", "INR", "IQD", "IRR", "ISJ", "ISK", "ITL",
    "JMD", "JOD", "JPY", "KES", "KGS", "KHR", "KMF", "KPW", "KRH", "KRO", "KRW", "KWD", "KYD",
    "KZT", "LAK", "LBP", "LKR", "LRD", "LSL", "LTL", "LTT", "LUC", "LUF", "LUL", "LVL", "LVR",
    "LYD", "MAD", "MAF", "MCF", "MDC", "MDL", "MGA", "MGF", "MKD", "MKN", "MLF", "MMK", "MNT",
    "MOP", "MRO", "MRU", "MTL", "MTP", "MUR", "MVP", "MVR", "MWK", "MXN", "MXP", "MXV", "MYR",
    "MZE", "MZM", "MZN", "NAD", "NGN", "NIC", "NIO", "NLG", "NOK", "NPR", "NZD", "OMR", "PAB",
    "PEI", "PEN", "PES", "PGK", "PHP", "PKR", "PLN", "PLZ", "PTE", "PYG", "QAR", "RHD", "ROL",
    "RON", "RSD", "RUB", "RUR", "RWF", "SAR", "SBD", "SCR", "SDD", "SDG", "SDP", "SEK", "SGD",
    "SHP", "SIT", "SKK", "SLE", "SLL", "SOS", "SRD", "SRG", "SSP", "STD", "STN", "SUR", "SVC",
    "SYP", "SZL", "THB", "TJR", "TJS", "TMM", "TMT", "TND", "TOP", "TPE", "TRL", "TRY", "TTD",
    "TWD", "TZS", "UAH", "UAK", "UGS", "UGX", "USD", "USN", "USS", "UYI", "UYP", "UYU", "UYW",
    "UZS", "VEB", "VED", "VEF", "VES", "VND", "VNN", "VUV", "WST", "XAF", "XAG", "XAU", "XBA",
    "XBB", "XBC", "XBD", "XCD", "XCG", "XDR", "XEU", "XFO", "XFU", "XOF", "XPD", "XPF", "XPT",
    "XRE", "XSU", "XTS", "XUA", "XXX", "YDD", "YER", "YUD", "YUM", "YUN", "YUR", "ZAL", "ZAR",
    "ZMK", "ZMW", "ZRN", "ZRZ", "ZWD", "ZWG", "ZWL", "ZWR",
];

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

    /// Returns every resolved CLDR locale whose decimal data is embedded.
    ///
    /// Unlike [`Self::number_format_locales`], which is the legacy
    /// language-level ICU inventory retained for compatibility checks, this
    /// contains every locale/script/region record in the pinned provider and
    /// is the required basis for full provider-coverage matrices.
    pub fn number_format_resolved_locales(self) -> Vec<String> {
        decimal_symbols::resolved_locales()
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
            // DurationFormat consumes the same decimal digits, plural rules,
            // and complete CLDR simple-unit records as NumberFormat. Do not
            // narrow it to the older hand-maintained language list.
            IntlService::NumberFormat | IntlService::DurationFormat => {
                self.supports_decimal_locale(locale)
            }
            // DateTimeFormat requires both its raw CLDR available-format
            // records and localized decimal symbols for date/time digits.
            IntlService::DateTimeFormat => {
                date_time_formats::has_locale(&locale.to_string())
                    && self.supports_decimal_locale(locale)
            }
            IntlService::DisplayNames => display_names::has_locale(&locale.to_string()),
            IntlService::ListFormat => list_patterns::has_locale(&locale.to_string()),
            IntlService::RelativeTimeFormat => {
                display_names::has_relative_time_locale(&locale.to_string())
            }
            _ => self.supports_language(locale),
        }
    }

    /// Returns the exact pinned CLDR list-pattern quartet selected by this
    /// locale, relation, and width.
    pub(crate) fn list_patterns(
        self,
        locale: &str,
        list_type: crate::ListType,
        style: crate::ListStyle,
    ) -> Option<list_patterns::PinnedListPatterns> {
        list_patterns::patterns(locale, list_type, style)
    }

    /// Returns the complete ordered raw CLDR Gregorian `availableFormats`
    /// record list used by DateTimeFormat's BasicFormatMatcher.
    pub(crate) fn date_time_format_records(self, locale: &str) -> Vec<crate::DateTimeFormatRecord> {
        date_time_formats::basic_format_records(locale)
    }

    /// Returns the locale's pinned CLDR pattern and localized field label for
    /// appending a missing DateTimeFormat field to the closest skeleton.
    pub(crate) fn date_time_append_item(
        self,
        locale: &str,
        field: &str,
    ) -> Option<date_time_formats::AppendItemPattern> {
        date_time_formats::append_item_pattern(locale, field)
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

    /// Returns the primary time zone identifiers of the pinned TZDB bundle
    /// (`AvailablePrimaryTimeZoneIdentifiers`), in ECMA-402 supported-values
    /// order: every Zone or Link name that is its own primary identifier, so a
    /// backward-compatibility alias such as `Asia/Calcutta` or `Etc/UTC` is not
    /// listed but `Africa/Accra` (a `zone.tab` name) is.
    pub fn time_zones(self) -> Vec<String> {
        let mut values = jiff_tzdb::available()
            .filter(|name| crate::is_primary_time_zone_identifier(name))
            .map(str::to_owned)
            .collect::<Vec<_>>();
        values.sort_unstable();
        values
    }

    /// Returns DateTimeFormat's default calendar for a supported locale.
    pub fn default_calendar(self, locale: &IcuLocale) -> &'static str {
        locale_information_data::default_calendar(
            &crate::locale_information::locale_preference_region(locale),
        )
    }

    /// Returns the locale's default decimal numbering system.
    pub fn default_numbering_system(self, locale: &IcuLocale) -> &'static str {
        decimal_symbols::default_numbering_system(&locale.to_string()).unwrap_or("latn")
    }

    /// Returns the locale's default hour cycle.
    pub fn default_hour_cycle(self, locale: &IcuLocale) -> &'static str {
        let preference_region = crate::locale_information::locale_preference_region(locale);
        locale_information_data::hour_cycle_for_locale(
            locale.id.language.as_str(),
            &preference_region,
        )
    }

    pub(crate) fn calendars_for_region(self, region: &str) -> &'static [String] {
        locale_information_data::calendars_for_region(region)
    }

    pub(crate) fn collations_for_language(self, language: &str) -> &'static [&'static str] {
        if language == "und" || ("qfz"..="qtz").contains(&language) {
            &["emoji", "eor"]
        } else {
            &["emoji"]
        }
    }

    pub(crate) fn hour_cycle_for_locale(self, language: &str, region: &str) -> &'static str {
        locale_information_data::hour_cycle_for_locale(language, region)
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

    pub(crate) fn time_zones_for_region(self, region: &str) -> &'static [String] {
        locale_information_data::time_zones_for_region(region)
    }

    pub(crate) fn week_data_for_region(self, region: &str) -> (u8, &'static [u8]) {
        locale_information_data::week_data_for_region(region)
    }

    /// Looks up one localized name in the pinned DisplayNames data bundle.
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
        fallback: crate::DisplayNamesFallback,
        code: &str,
    ) -> Option<String> {
        if display_type == crate::DisplayNamesType::Currency {
            return currency_patterns::currency_display_name(locale, code);
        }
        display_names::display_name(
            locale,
            display_type,
            style,
            language_display,
            fallback,
            code,
        )
    }

    /// Returns the localized DurationFormat unit pattern for `locale`.
    ///
    /// DurationFormat and NumberFormat select the same pinned CLDR
    /// simple-unit record. This preserves plural-sensitive forms, labels that
    /// precede the number, and singular forms which deliberately omit it.
    pub(crate) fn duration_unit_pattern(
        self,
        locale: &str,
        unit: crate::DurationUnit,
        style: crate::DurationUnitStyle,
        plural: crate::PluralCategory,
    ) -> NumberUnitPattern {
        let unit = match unit {
            crate::DurationUnit::Years => crate::NumberFormatUnit::Year,
            crate::DurationUnit::Months => crate::NumberFormatUnit::Month,
            crate::DurationUnit::Weeks => crate::NumberFormatUnit::Week,
            crate::DurationUnit::Days => crate::NumberFormatUnit::Day,
            crate::DurationUnit::Hours => crate::NumberFormatUnit::Hour,
            crate::DurationUnit::Minutes => crate::NumberFormatUnit::Minute,
            crate::DurationUnit::Seconds => crate::NumberFormatUnit::Second,
            crate::DurationUnit::Milliseconds => crate::NumberFormatUnit::Millisecond,
            crate::DurationUnit::Microseconds => crate::NumberFormatUnit::Microsecond,
            crate::DurationUnit::Nanoseconds => crate::NumberFormatUnit::Nanosecond,
        };
        let display = match style {
            crate::DurationUnitStyle::Long => crate::NumberUnitDisplay::Long,
            crate::DurationUnitStyle::Short => crate::NumberUnitDisplay::Short,
            crate::DurationUnitStyle::Narrow => crate::NumberUnitDisplay::Narrow,
            // Callers route numeric styles to FormatNumericUnits; retain a
            // total, visibly empty pattern for defensive internal callers.
            crate::DurationUnitStyle::Numeric | crate::DurationUnitStyle::TwoDigit => {
                return NumberUnitPattern {
                    prefix: String::new(),
                    prefix_separator: String::new(),
                    suffix_separator: String::new(),
                    suffix: String::new(),
                    hides_number: false,
                };
            }
        };
        self.number_unit_pattern(locale, unit, display, plural)
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
        // This table has an exact raw CLDR simple-unit record for every
        // resolved locale/unit/width/plural cell. It is the sole localized
        // provider: keeping legacy, per-language fallback chains after this
        // complete lookup would leave unreachable records that could silently
        // diverge from the pinned CLDR source.
        if let Some(pattern) = unit_patterns::cldr_full_unit_pattern(locale, unit, display, plural)
        {
            return (pattern, true);
        }
        (
            english_number_unit_pattern(unit, display, plural == crate::PluralCategory::One),
            false,
        )
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

    /// Returns the full pinned CLDR relative-time pattern for this service
    /// cell. Every advertised RelativeTimeFormat locale carries at least an
    /// `other` pattern for both directions and each ECMA-402 unit.
    pub(crate) fn relative_time_pattern(
        self,
        locale: &str,
        style: crate::RelativeTimeStyle,
        unit: crate::RelativeTimeUnit,
        past: bool,
        plural: crate::PluralCategory,
    ) -> Option<String> {
        display_names::relative_time_pattern(locale, style, unit, past, plural)
    }

    /// Returns the locale's qualitative `numeric: "auto"` relative-time
    /// term when CLDR provides one for the exact offset.
    pub(crate) fn relative_time_qualitative_term(
        self,
        locale: &str,
        style: crate::RelativeTimeStyle,
        numeric: crate::RelativeTimeNumeric,
        value: f64,
        unit: crate::RelativeTimeUnit,
    ) -> Option<String> {
        if numeric != crate::RelativeTimeNumeric::Auto {
            return None;
        }
        let offset = if value == 0.0 {
            0
        } else if value == 1.0 {
            1
        } else if value == -1.0 {
            -1
        } else {
            return None;
        };
        display_names::relative_time_term(locale, style, unit, offset)
    }

    /// Selects a cardinal plural category from the selected relative-time
    /// locale. Missing compact ICU rule data visibly falls back to `other`,
    /// whose CLDR pattern is mandatory for every raw provider cell.
    pub(crate) fn relative_time_plural_category(
        self,
        locale: &str,
        value: f64,
    ) -> crate::PluralCategory {
        let Ok(locale) = crate::canonicalize(locale) else {
            return crate::PluralCategory::Other;
        };
        // ICU4X's compact cardinal bundle has one explicit CLDR supplement
        // and two script-record parent lookups. RelativeTimeFormat must share
        // the same rules as Intl.PluralRules and NumberFormat so it selects a
        // raw relative-time pattern from the same category in every service.
        if locale.locale().id.language.as_str() == "gv" {
            return crate::plural_rules::SupplementalPluralRules::Manx
                .select(&value.abs().to_string());
        }
        let Ok(decimal) = Decimal::try_from_f64(value.abs(), FloatPrecision::RoundTrip) else {
            return crate::PluralCategory::Other;
        };
        let data_locale = if locale.as_str().starts_with("sr-Latn") {
            crate::canonicalize("sr")
                .expect("the Serbian plural-rule parent is structurally valid")
                .locale()
                .clone()
        } else if locale.as_str().starts_with("bs-Cyrl") {
            crate::canonicalize("bs")
                .expect("the Bosnian plural-rule parent is structurally valid")
                .locale()
                .clone()
        } else {
            locale.locale().clone()
        };
        let Ok(rules) = IcuPluralRules::try_new_cardinal(data_locale.into()) else {
            return crate::PluralCategory::Other;
        };
        plural_category_from_icu(rules.category_for(&decimal))
    }
}

mod helpers;

use helpers::*;

#[cfg(test)]
#[path = "locale_data/tests.rs"]
mod tests;
