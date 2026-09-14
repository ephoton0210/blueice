// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

//! Host-neutral implementation of the ECMA-402 collation service.

use super::*;
use std::cmp::Ordering;

/// The `usage` option of an ECMA-402 collator.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub enum CollatorUsage {
    /// A sorting collator.
    #[default]
    Sort,
    /// A text-search collator.
    Search,
}

/// The `caseFirst` option of an ECMA-402 collator.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum CaseFirst {
    /// Sort uppercase before lowercase where the tailoring supports it.
    Upper,
    /// Sort lowercase before uppercase where the tailoring supports it.
    Lower,
    /// Use the locale default case ordering.
    False,
}

/// The `sensitivity` option of an ECMA-402 collator.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub enum Sensitivity {
    /// Compare base characters only.
    Base,
    /// Compare base characters and accents.
    Accent,
    /// Compare base characters and case.
    Case,
    /// Compare all supported collation distinctions.
    #[default]
    Variant,
}

/// Host-neutral input for constructing a collation service.
#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub struct CollatorOptions {
    /// The desired locale matching policy.
    pub locale_matcher: LocaleMatcher,
    /// Whether the collator is used for sorting or searching.
    pub usage: CollatorUsage,
    /// An optional UTS 35 collation type. Unsupported values resolve to the
    /// locale default, per ECMA-402.
    pub collation: Option<String>,
    /// Overrides the locale's `kn` extension when present.
    pub numeric: Option<bool>,
    /// Overrides the locale's `kf` extension when present.
    pub case_first: Option<CaseFirst>,
    /// Controls collation strength and case level.
    pub sensitivity: Sensitivity,
    /// Overrides the locale default punctuation handling when present.
    pub ignore_punctuation: Option<bool>,
}

/// ECMAScript-observable data resolved by a collation service.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ResolvedCollatorOptions {
    /// The negotiated locale with only supported, non-overridden Unicode keys.
    pub locale: String,
    /// The selected usage.
    pub usage: CollatorUsage,
    /// The selected sensitivity.
    pub sensitivity: Sensitivity,
    /// Whether punctuation is ignored.
    pub ignore_punctuation: bool,
    /// The selected collation type, or `default`.
    pub collation: String,
    /// Whether numeric collation is enabled.
    pub numeric: bool,
    /// The selected case ordering.
    pub case_first: CaseFirst,
}

/// A failure while constructing a collator from the bundled ICU4X data.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum CollatorError {
    /// The selected built-in collation data was unavailable.
    DataUnavailable,
}

impl std::fmt::Display for CollatorError {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::DataUnavailable => formatter.write_str("collation data is unavailable"),
        }
    }
}

impl std::error::Error for CollatorError {}

/// A host-neutral UTF-16 collation service backed by ICU4X.
///
/// The service accepts and compares UTF-16 code units directly. Embedders are
/// responsible only for their own input coercion and error presentation.
pub struct Collator {
    algorithm: CollatorBorrowed<'static>,
    negotiation: CollationLocaleNegotiation,
    resolved: ResolvedCollatorOptions,
    german_search: bool,
}

impl Collator {
    /// Constructs a collator after locale negotiation and option resolution.
    pub fn try_new(
        requested: &[CanonicalLocale],
        options: CollatorOptions,
    ) -> Result<Self, CollatorError> {
        let negotiation = negotiate_collation_locale(requested, options.locale_matcher);
        let selected = negotiation.selected.clone();
        let selected_locale = selected.locale();
        let mut selected_collation = "default".to_owned();
        let mut resolved_keys = Vec::new();

        for (key, option) in [
            (
                "co",
                options.collation.as_deref().map(str::to_ascii_lowercase),
            ),
            (
                "kf",
                options.case_first.map(|value| match value {
                    CaseFirst::Upper => "upper".to_owned(),
                    CaseFirst::Lower => "lower".to_owned(),
                    CaseFirst::False => "false".to_owned(),
                }),
            ),
            ("kn", options.numeric.map(|value| value.to_string())),
        ] {
            let valid = |value: &str| {
                (key == "co"
                    && options.usage == CollatorUsage::Sort
                    && supports_collation(selected_locale, value))
                    || (key == "kf" && matches!(value, "upper" | "lower" | "false"))
                    || (key == "kn" && matches!(value, "true" | "false"))
            };
            let resolution = resolve_locale_key(&selected, key, option.as_deref(), None, |value| {
                let value = if key == "kn" && value.is_empty() {
                    "true"
                } else {
                    value
                };
                valid(value).then(|| value.to_owned())
            });
            if let Some(value) = resolution.value() {
                if key == "co" {
                    selected_collation = value.to_owned();
                }
            }
            resolved_keys.push((key, resolution));
        }

        let key_references = resolved_keys
            .iter()
            .map(|(key, resolution)| (*key, resolution))
            .collect::<Vec<_>>();
        let resolved_locale = locale_with_resolved_keys(&selected, &key_references);
        let algorithm_locale = resolved_locale.locale().clone();

        let mut preferences: CollatorPreferences = (&algorithm_locale).into();
        if options.usage == CollatorUsage::Search {
            preferences.collation_type = Some(CollationType::Search);
        }
        let mut icu_options = IcuCollatorOptions::default();
        icu_options.strength = Some(match options.sensitivity {
            Sensitivity::Base | Sensitivity::Case => Strength::Primary,
            Sensitivity::Accent => Strength::Secondary,
            Sensitivity::Variant => Strength::Tertiary,
        });
        icu_options.case_level = Some(if options.sensitivity == Sensitivity::Case {
            CaseLevel::On
        } else {
            CaseLevel::Off
        });
        let ignore_punctuation = options
            .ignore_punctuation
            .unwrap_or_else(|| selected_locale.id.language.as_str() == "th");
        icu_options.alternate_handling = Some(if ignore_punctuation {
            AlternateHandling::Shifted
        } else {
            AlternateHandling::NonIgnorable
        });
        icu_options.max_variable = Some(MaxVariable::Punctuation);
        let algorithm = CollatorBorrowed::try_new(preferences, icu_options)
            .map_err(|_| CollatorError::DataUnavailable)?;
        let icu_resolved = algorithm.resolved_options();
        let resolved = ResolvedCollatorOptions {
            locale: resolved_locale.as_str().to_owned(),
            usage: options.usage,
            sensitivity: options.sensitivity,
            ignore_punctuation,
            collation: selected_collation,
            numeric: icu_resolved.numeric == CollationNumericOrdering::True,
            case_first: match icu_resolved.case_first {
                CollationCaseFirst::Upper => CaseFirst::Upper,
                CollationCaseFirst::Lower => CaseFirst::Lower,
                _ => CaseFirst::False,
            },
        };
        let german_search =
            options.usage == CollatorUsage::Search && selected_locale.id.language.as_str() == "de";
        Ok(Self {
            algorithm,
            negotiation,
            resolved,
            german_search,
        })
    }

    /// Compares two UTF-16 strings according to the resolved collation.
    pub fn compare_utf16(&self, left: &[u16], right: &[u16]) -> Ordering {
        if self.german_search {
            let left = german_search_fold(left);
            let right = german_search_fold(right);
            self.algorithm.compare_utf16(&left, &right)
        } else {
            self.algorithm.compare_utf16(left, right)
        }
    }

    /// Returns the data selected during construction.
    pub fn resolved_options(&self) -> &ResolvedCollatorOptions {
        &self.resolved
    }

    /// Returns the locale-negotiation trace produced during construction.
    pub fn negotiation(&self) -> &CollationLocaleNegotiation {
        &self.negotiation
    }

    /// Returns the heap storage directly owned by this service.
    pub fn bytes(&self) -> usize {
        std::mem::size_of::<Self>() + self.resolved.locale.len() + self.resolved.collation.len()
    }
}

fn german_search_fold(value: &[u16]) -> Vec<u16> {
    let mut units = Vec::with_capacity(value.len());
    for unit in value {
        match unit {
            0x00c4 => units.extend([b'A' as u16, b'E' as u16]),
            0x00d6 => units.extend([b'O' as u16, b'E' as u16]),
            0x00dc => units.extend([b'U' as u16, b'E' as u16]),
            0x00df => units.extend([b's' as u16, b's' as u16]),
            0x00e4 => units.extend([b'a' as u16, b'e' as u16]),
            0x00f6 => units.extend([b'o' as u16, b'e' as u16]),
            0x00fc => units.extend([b'u' as u16, b'e' as u16]),
            unit => units.push(*unit),
        }
    }
    units
}
