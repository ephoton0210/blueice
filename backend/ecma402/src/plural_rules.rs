// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

//! Host-neutral implementation of the ECMA-402 plural-rules service.

use super::*;
use fixed_decimal::CompactDecimal;
use icu_decimal::{
    input::{Decimal, FloatPrecision},
    CompactDecimalFormatter,
};
use icu_plurals::{
    PluralCategory as IcuPluralCategory, PluralRuleType as IcuPluralRuleType,
    PluralRules as IcuPluralRules, PluralRulesOptions as IcuPluralRulesOptions,
};

/// The CLDR plural-rule family selected by `Intl.PluralRules`.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub enum PluralRuleType {
    /// Select a category for an ordinary quantity.
    #[default]
    Cardinal,
    /// Select a category for an ordinal position.
    Ordinal,
}

impl From<PluralRuleType> for IcuPluralRuleType {
    fn from(value: PluralRuleType) -> Self {
        match value {
            PluralRuleType::Cardinal => Self::Cardinal,
            PluralRuleType::Ordinal => Self::Ordinal,
        }
    }
}

/// A CLDR plural category returned by `Intl.PluralRules`.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum PluralCategory {
    /// The `zero` category.
    Zero,
    /// The `one` category.
    One,
    /// The `two` category.
    Two,
    /// The `few` category.
    Few,
    /// The `many` category.
    Many,
    /// The `other` catch-all category.
    Other,
}

impl From<IcuPluralCategory> for PluralCategory {
    fn from(value: IcuPluralCategory) -> Self {
        match value {
            IcuPluralCategory::Zero => Self::Zero,
            IcuPluralCategory::One => Self::One,
            IcuPluralCategory::Two => Self::Two,
            IcuPluralCategory::Few => Self::Few,
            IcuPluralCategory::Many => Self::Many,
            IcuPluralCategory::Other => Self::Other,
        }
    }
}

/// One canonical locale considered during plural-rule negotiation.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct PluralRulesLocaleCandidate {
    requested: CanonicalLocale,
    supported: bool,
}

impl PluralRulesLocaleCandidate {
    /// Returns the canonical locale supplied by the host.
    pub fn requested(&self) -> &CanonicalLocale {
        &self.requested
    }

    /// Returns whether the bundled plural-rule data supports this request.
    pub fn is_supported(&self) -> bool {
        self.supported
    }
}

/// A deterministic trace of plural-rule locale negotiation.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct PluralRulesLocaleNegotiation {
    matcher: LocaleMatcher,
    candidates: Vec<PluralRulesLocaleCandidate>,
    selected: CanonicalLocale,
    used_default: bool,
}

impl PluralRulesLocaleNegotiation {
    /// Returns the matcher used for this negotiation.
    pub fn matcher(&self) -> LocaleMatcher {
        self.matcher
    }

    /// Returns every requested locale and its data-support decision.
    pub fn candidates(&self) -> &[PluralRulesLocaleCandidate] {
        &self.candidates
    }

    /// Returns the locale selected for plural-rule evaluation.
    pub fn selected(&self) -> &CanonicalLocale {
        &self.selected
    }

    /// Returns whether the stable `en-US` service default was selected.
    pub fn used_default(&self) -> bool {
        self.used_default
    }
}

/// Negotiates a requested locale list against bundled plural-rule data.
///
/// Plural data follows the shared compiled-language registry. Script and
/// region subtags remain attached to the selected locale for future data
/// tailoring; if none match, the stable service default is `en-US`.
pub fn negotiate_plural_rules_locale(
    requested: &[CanonicalLocale],
    matcher: LocaleMatcher,
) -> PluralRulesLocaleNegotiation {
    let candidates = requested
        .iter()
        .cloned()
        .map(|requested| PluralRulesLocaleCandidate {
            supported: locale_data_provider()
                .supports_service_locale(IntlService::PluralRules, requested.locale()),
            requested,
        })
        .collect::<Vec<_>>();
    let selected = candidates
        .iter()
        .find(|candidate| candidate.supported)
        .map(|candidate| candidate.requested.clone());
    let used_default = selected.is_none();
    PluralRulesLocaleNegotiation {
        matcher,
        candidates,
        selected: selected
            .unwrap_or_else(|| canonicalize("en-US").expect("the default locale is valid")),
        used_default,
    }
}

/// Resolves one requested locale against bundled plural-rule data.
pub fn resolve_plural_rules_locale(
    requested: &[CanonicalLocale],
    matcher: LocaleMatcher,
) -> CanonicalLocale {
    negotiate_plural_rules_locale(requested, matcher)
        .selected
        .clone()
}

/// Returns the requested locales supported by the bundled plural-rule service.
pub fn supported_plural_rules_locales(
    requested: &[CanonicalLocale],
    matcher: LocaleMatcher,
) -> Vec<CanonicalLocale> {
    negotiate_plural_rules_locale(requested, matcher)
        .candidates
        .into_iter()
        .filter(|candidate| candidate.supported)
        .map(|candidate| candidate.requested)
        .collect()
}

/// Host-neutral options for constructing an `Intl.PluralRules` service.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub struct PluralRulesOptions {
    /// The requested locale matching policy.
    pub locale_matcher: LocaleMatcher,
    /// Whether to evaluate cardinal or ordinal rules.
    pub rule_type: PluralRuleType,
}

/// ECMAScript-observable data resolved by a plural-rule service.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ResolvedPluralRulesOptions {
    /// The negotiated locale.
    pub locale: String,
    /// The selected plural-rule family.
    pub rule_type: PluralRuleType,
}

/// A failure while constructing or evaluating plural rules.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum PluralRulesError {
    /// The selected plural-rule data was unavailable.
    DataUnavailable,
    /// A decimal input was not a finite, base-10 decimal string.
    InvalidDecimal,
    /// An IEEE-754 input was `NaN` or infinite.
    NonFiniteNumber,
}

impl std::fmt::Display for PluralRulesError {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::DataUnavailable => formatter.write_str("plural-rule data is unavailable"),
            Self::InvalidDecimal => formatter.write_str("invalid finite decimal input"),
            Self::NonFiniteNumber => formatter.write_str("number must be finite"),
        }
    }
}

impl std::error::Error for PluralRulesError {}

/// CLDR rules not carried by the compact ICU plural marker bundled with this
/// build. Keeping the exceptional rule at the host boundary means both
/// `select` and `resolvedOptions().pluralCategories` see the same data.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum SupplementalPluralRules {
    Manx,
}

impl SupplementalPluralRules {
    pub(crate) fn select(self, value: &str) -> PluralCategory {
        match self {
            // CLDR cardinal rules for `gv`: decimals are `many`; for integer
            // operands, the final digit yields `one`/`two`, and multiples of
            // 20 yield `few`.
            Self::Manx => {
                let value = value.trim_start_matches(['+', '-']);
                if value.contains('.') {
                    return PluralCategory::Many;
                }
                let digits = value.as_bytes();
                let last = digits.iter().rev().find(|byte| byte.is_ascii_digit());
                let penultimate = digits
                    .iter()
                    .rev()
                    .filter(|byte| byte.is_ascii_digit())
                    .nth(1);
                let last = last.map_or(0, |byte| byte - b'0');
                let modulo_hundred = penultimate.map_or(last, |byte| (byte - b'0') * 10 + last);
                if last == 1 {
                    PluralCategory::One
                } else if last == 2 {
                    PluralCategory::Two
                } else if matches!(modulo_hundred, 0 | 20 | 40 | 60 | 80) {
                    PluralCategory::Few
                } else {
                    PluralCategory::Other
                }
            }
        }
    }
}

/// A host-neutral `Intl.PluralRules` service backed by ICU4X.
///
/// A decimal string preserves the visible fraction digits CLDR rules need to
/// distinguish values such as `1` and `1.0`. Embedders retain ECMAScript
/// coercion and later digit-option rounding semantics at their public
/// boundary.
pub struct PluralRules {
    rules: IcuPluralRules,
    supplemental: Option<SupplementalPluralRules>,
    negotiation: PluralRulesLocaleNegotiation,
    resolved: ResolvedPluralRulesOptions,
}

impl PluralRules {
    /// Constructs plural rules after locale negotiation and option resolution.
    pub fn try_new(
        requested: &[CanonicalLocale],
        options: PluralRulesOptions,
    ) -> Result<Self, PluralRulesError> {
        let negotiation = negotiate_plural_rules_locale(requested, options.locale_matcher);
        let selected = negotiation.selected.clone();
        let preferences = selected.locale().into();
        let rules = IcuPluralRules::try_new(
            preferences,
            IcuPluralRulesOptions::from(IcuPluralRuleType::from(options.rule_type)),
        )
        .map_err(|_| PluralRulesError::DataUnavailable)?;
        Ok(Self {
            rules,
            supplemental: (selected.locale().id.language.as_str() == "gv")
                .then_some(SupplementalPluralRules::Manx),
            negotiation,
            resolved: ResolvedPluralRulesOptions {
                locale: selected.as_str().into(),
                rule_type: options.rule_type,
            },
        })
    }

    /// Selects a plural category for a finite base-10 decimal string.
    pub fn select_decimal(&self, value: &str) -> Result<PluralCategory, PluralRulesError> {
        let decimal = Decimal::try_from_str(value).map_err(|_| PluralRulesError::InvalidDecimal)?;
        if let Some(supplemental) = self.supplemental {
            return Ok(supplemental.select(value));
        }
        Ok(self.rules.category_for(&decimal).into())
    }

    /// Selects a plural category for a finite IEEE-754 number.
    ///
    /// This representation intentionally has no visible trailing fraction
    /// zeros; callers that need those operands should use [`select_decimal`](Self::select_decimal).
    pub fn select_f64(&self, value: f64) -> Result<PluralCategory, PluralRulesError> {
        let decimal = Decimal::try_from_f64(value, FloatPrecision::RoundTrip)
            .map_err(|_| PluralRulesError::NonFiniteNumber)?;
        if let Some(supplemental) = self.supplemental {
            return Ok(supplemental.select(&value.to_string()));
        }
        Ok(self.rules.category_for(&decimal).into())
    }

    /// Selects a category after compact decimal notation has supplied its
    /// locale-dependent exponent operand.
    ///
    /// ECMA-402's `PluralRuleSelect` carries the compact exponent (`c`) into
    /// CLDR plural evaluation. Passing the original decimal would make, for
    /// example, French `1.5e6` select `other` instead of the compact `many`.
    pub fn select_compact_f64(
        &self,
        value: f64,
        long_display: bool,
    ) -> Result<PluralCategory, PluralRulesError> {
        let decimal = Decimal::try_from_f64(value, FloatPrecision::RoundTrip)
            .map_err(|_| PluralRulesError::NonFiniteNumber)?;
        if let Some(supplemental) = self.supplemental {
            return Ok(supplemental.select(&value.to_string()));
        }
        if value == 0.0 {
            return Ok(self.rules.category_for(&decimal).into());
        }
        let preferences = self.negotiation.selected.locale().into();
        let formatter = if long_display {
            CompactDecimalFormatter::try_new_long(preferences, Default::default())
        } else {
            CompactDecimalFormatter::try_new_short(preferences, Default::default())
        }
        .map_err(|_| PluralRulesError::DataUnavailable)?;
        let exponent = formatter.compact_exponent_for_magnitude(decimal.nonzero_magnitude_start());
        let compact = CompactDecimal::from_significand_and_exponent(decimal, exponent);
        Ok(self.rules.category_for(&compact).into())
    }

    /// Returns the data selected during construction.
    pub fn resolved_options(&self) -> &ResolvedPluralRulesOptions {
        &self.resolved
    }

    /// Returns the locale-negotiation trace produced during construction.
    pub fn negotiation(&self) -> &PluralRulesLocaleNegotiation {
        &self.negotiation
    }

    /// Returns the heap storage directly owned by this service.
    pub fn bytes(&self) -> usize {
        std::mem::size_of::<Self>() + self.resolved.locale.len()
    }
}
