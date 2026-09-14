// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

//! Host-neutral implementation of the ECMA-402 display-name service.

use super::*;

/// The `type` option accepted by `Intl.DisplayNames`.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum DisplayNamesType {
    /// BCP 47 language identifiers.
    Language,
    /// ISO 3166-style region identifiers.
    Region,
    /// ISO 15924-style script identifiers.
    Script,
    /// ISO 4217 currency identifiers.
    Currency,
    /// Unicode calendar identifiers.
    Calendar,
    /// The fixed ECMA-402 date-time-field identifiers.
    DateTimeField,
}

/// The width requested from `Intl.DisplayNames`.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub enum DisplayNamesStyle {
    /// The ordinary CLDR display name.
    #[default]
    Long,
    /// An abbreviated CLDR display name.
    Short,
    /// A narrow CLDR display name.
    Narrow,
}

/// How missing display-name data is represented.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub enum DisplayNamesFallback {
    /// Return the canonical code when no localized name is available.
    #[default]
    Code,
    /// Return no result when no localized name is available.
    None,
}

/// The language-name spelling policy requested by `Intl.DisplayNames`.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub enum DisplayNamesLanguageDisplay {
    /// Prefer the language's dialect name where the locale data has one.
    #[default]
    Dialect,
    /// Prefer the standard language name.
    Standard,
}

/// Host-neutral options for constructing `Intl.DisplayNames`.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct DisplayNamesOptions {
    /// The requested locale matching policy.
    pub locale_matcher: LocaleMatcher,
    /// The kind of code to display.
    pub display_type: DisplayNamesType,
    /// The requested display-name width.
    pub style: DisplayNamesStyle,
    /// The result for a valid code that has no bundled data.
    pub fallback: DisplayNamesFallback,
    /// The policy for language names.
    pub language_display: DisplayNamesLanguageDisplay,
}

/// ECMAScript-observable data resolved by a display-name service.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ResolvedDisplayNamesOptions {
    /// The negotiated locale.
    pub locale: String,
    /// The kind of displayed code.
    pub display_type: DisplayNamesType,
    /// The selected display-name width.
    pub style: DisplayNamesStyle,
    /// The missing-data policy.
    pub fallback: DisplayNamesFallback,
    /// The selected language-name policy, only for language display names.
    pub language_display: Option<DisplayNamesLanguageDisplay>,
}

/// A failure from constructing or using `Intl.DisplayNames`.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum DisplayNamesError {
    /// The code is invalid for the service's selected type.
    InvalidCode,
}

impl std::fmt::Display for DisplayNamesError {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::InvalidCode => formatter.write_str("invalid display-name code"),
        }
    }
}

impl std::error::Error for DisplayNamesError {}

/// Returns whether the bundled display-name data supports this locale.
pub fn supports_display_names_locale(locale: &IcuLocale) -> bool {
    locale_data_provider().supports_service_locale(IntlService::DisplayNames, locale)
}

/// Resolves requested locales for the display-name service.
///
/// The stable `en-US` fallback is supported by this service, so successful
/// resolution is also the construction-time data-availability invariant.
pub fn resolve_display_names_locale(
    requested: &[CanonicalLocale],
    _matcher: LocaleMatcher,
) -> CanonicalLocale {
    requested
        .iter()
        .find(|locale| supports_display_names_locale(locale.locale()))
        .cloned()
        .unwrap_or_else(|| canonicalize("en-US").expect("the default locale is valid"))
}

/// Returns requested locales supported by the bundled display-name service.
pub fn supported_display_names_locales(
    requested: &[CanonicalLocale],
    _matcher: LocaleMatcher,
) -> Vec<CanonicalLocale> {
    requested
        .iter()
        .filter(|locale| supports_display_names_locale(locale.locale()))
        .cloned()
        .collect()
}

/// A host-neutral `Intl.DisplayNames` service.
///
/// The standard permits locale-data coverage to be implementation dependent.
/// This service therefore supplies a small deterministic data set and applies
/// the specified `code`/`none` fallback to any canonical code it does not
/// carry. It never treats an invalid code as missing data.
pub struct DisplayNames {
    resolved: ResolvedDisplayNamesOptions,
}

impl DisplayNames {
    /// Constructs a display-name service after locale negotiation.
    pub fn try_new(
        requested: &[CanonicalLocale],
        options: DisplayNamesOptions,
    ) -> Result<Self, DisplayNamesError> {
        let locale = resolve_display_names_locale(requested, options.locale_matcher);
        Ok(Self {
            resolved: ResolvedDisplayNamesOptions {
                locale: locale.as_str().to_owned(),
                display_type: options.display_type,
                style: options.style,
                fallback: options.fallback,
                language_display: (options.display_type == DisplayNamesType::Language)
                    .then_some(options.language_display),
            },
        })
    }

    /// Returns the resolved service data.
    pub fn resolved_options(&self) -> &ResolvedDisplayNamesOptions {
        &self.resolved
    }

    /// Canonicalizes `code` for the selected type and returns its display name.
    ///
    /// `None` is the specified result for a valid but uncovered code when the
    /// service was configured with `fallback: "none"`.
    pub fn of(&self, code: &str) -> Result<Option<String>, DisplayNamesError> {
        let code = canonical_display_name_code(self.resolved.display_type, code)?;
        let localized = locale_data_provider().display_name(
            self.resolved.locale.as_str(),
            self.resolved.display_type,
            self.resolved.style,
            self.resolved.language_display,
            &code,
        );
        Ok(localized.or(match self.resolved.fallback {
            DisplayNamesFallback::Code => Some(code),
            DisplayNamesFallback::None => None,
        }))
    }

    /// Returns the heap storage directly owned by this service.
    pub fn bytes(&self) -> usize {
        std::mem::size_of::<Self>() + self.resolved.locale.len()
    }
}

fn canonical_display_name_code(
    display_type: DisplayNamesType,
    code: &str,
) -> Result<String, DisplayNamesError> {
    let valid_type = |code: &str| {
        !code.is_empty()
            && code.split('-').all(|part| {
                (3..=8).contains(&part.len())
                    && part.bytes().all(|byte| byte.is_ascii_alphanumeric())
            })
    };
    match display_type {
        DisplayNamesType::Language => is_unicode_language_id(code)
            .then_some(())
            .ok_or(DisplayNamesError::InvalidCode)
            .map(|()| {
                // ICU's data-locale parser deliberately rejects some
                // structurally valid, unregistered language/variant values.
                // ECMA-402 still requires them to be canonical display-name
                // codes, so retain the grammar-derived casing when ICU cannot
                // provide an alias transformation.
                canonicalize(code)
                    .map(|locale| locale.to_string())
                    .unwrap_or_else(|_| canonical_unicode_language_id(code))
            }),
        DisplayNamesType::Region => {
            let valid = (code.len() == 2 && code.bytes().all(|byte| byte.is_ascii_alphabetic()))
                || (code.len() == 3 && code.bytes().all(|byte| byte.is_ascii_digit()));
            valid
                .then(|| code.to_ascii_uppercase())
                .ok_or(DisplayNamesError::InvalidCode)
        }
        DisplayNamesType::Script => (code.len() == 4
            && code.bytes().all(|byte| byte.is_ascii_alphabetic()))
        .then(|| {
            let mut code = code.to_ascii_lowercase();
            code[..1].make_ascii_uppercase();
            code
        })
        .ok_or(DisplayNamesError::InvalidCode),
        DisplayNamesType::Currency => (code.len() == 3
            && code.bytes().all(|byte| byte.is_ascii_alphabetic()))
        .then(|| code.to_ascii_uppercase())
        .ok_or(DisplayNamesError::InvalidCode),
        DisplayNamesType::Calendar => valid_type(code)
            .then(|| code.to_ascii_lowercase())
            .ok_or(DisplayNamesError::InvalidCode),
        DisplayNamesType::DateTimeField => matches!(
            code,
            "era"
                | "year"
                | "quarter"
                | "month"
                | "weekOfYear"
                | "weekday"
                | "day"
                | "dayPeriod"
                | "hour"
                | "minute"
                | "second"
                | "timeZoneName"
        )
        .then(|| code.to_owned())
        .ok_or(DisplayNamesError::InvalidCode),
    }
}

pub(crate) fn is_unicode_language_id(code: &str) -> bool {
    let mut subtags = code.split('-');
    let language = subtags
        .next()
        .expect("split always yields the language slot");
    if !matches!(language.len(), 2..=3 | 5..=8)
        || !language.bytes().all(|byte| byte.is_ascii_alphabetic())
    {
        return false;
    }
    let mut remaining = subtags.peekable();
    if remaining.peek().is_some_and(|subtag| {
        subtag.len() == 4 && subtag.bytes().all(|byte| byte.is_ascii_alphabetic())
    }) {
        remaining.next();
    }
    if remaining.peek().is_some_and(|subtag| {
        (subtag.len() == 2 && subtag.bytes().all(|byte| byte.is_ascii_alphabetic()))
            || (subtag.len() == 3 && subtag.bytes().all(|byte| byte.is_ascii_digit()))
    }) {
        remaining.next();
    }
    let mut variants = std::collections::HashSet::new();
    remaining.all(|subtag| {
        let valid = (5..=8).contains(&subtag.len())
            && subtag.bytes().all(|byte| byte.is_ascii_alphanumeric())
            || subtag.len() == 4
                && subtag.as_bytes()[0].is_ascii_digit()
                && subtag.bytes().all(|byte| byte.is_ascii_alphanumeric());
        valid && variants.insert(subtag.to_ascii_lowercase())
    })
}

pub(crate) fn canonical_unicode_language_id(code: &str) -> String {
    let mut subtags = code.split('-');
    let language = subtags.next().expect("validated language identifier");
    let mut result = vec![language.to_ascii_lowercase()];
    let mut remaining = subtags.peekable();
    if remaining.peek().is_some_and(|subtag| {
        subtag.len() == 4 && subtag.bytes().all(|byte| byte.is_ascii_alphabetic())
    }) {
        let mut script = remaining
            .next()
            .expect("present script")
            .to_ascii_lowercase();
        script[..1].make_ascii_uppercase();
        result.push(script);
    }
    if remaining.peek().is_some_and(|subtag| {
        (subtag.len() == 2 && subtag.bytes().all(|byte| byte.is_ascii_alphabetic()))
            || (subtag.len() == 3 && subtag.bytes().all(|byte| byte.is_ascii_digit()))
    }) {
        result.push(
            remaining
                .next()
                .expect("present region")
                .to_ascii_uppercase(),
        );
    }
    result.extend(remaining.map(str::to_ascii_lowercase));
    result.join("-")
}
