// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

//! Host-neutral implementation of the ECMA-402 list-format service.

use super::*;

/// The kind of relation joined by an `Intl.ListFormat` service.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub enum ListType {
    /// Join alternatives with a localized equivalent of "and".
    #[default]
    Conjunction,
    /// Join alternatives with a localized equivalent of "or".
    Disjunction,
    /// Join units without a conjunction.
    Unit,
}

/// The CLDR list-pattern width selected by `Intl.ListFormat`.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub enum ListStyle {
    /// The normal full-width list pattern.
    #[default]
    Wide,
    /// A compact list pattern.
    Short,
    /// The narrowest list pattern.
    Narrow,
}

impl From<ListStyle> for IcuListLength {
    fn from(value: ListStyle) -> Self {
        match value {
            ListStyle::Wide => Self::Wide,
            ListStyle::Short => Self::Short,
            ListStyle::Narrow => Self::Narrow,
        }
    }
}

/// One canonical locale considered during list-format negotiation.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ListFormatLocaleCandidate {
    requested: CanonicalLocale,
    supported: bool,
}

impl ListFormatLocaleCandidate {
    /// Returns the canonical locale supplied by the host.
    pub fn requested(&self) -> &CanonicalLocale {
        &self.requested
    }

    /// Returns whether the bundled list-pattern data supports this request.
    pub fn is_supported(&self) -> bool {
        self.supported
    }
}

/// A deterministic trace of list-format locale negotiation.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ListFormatLocaleNegotiation {
    matcher: LocaleMatcher,
    candidates: Vec<ListFormatLocaleCandidate>,
    selected: CanonicalLocale,
    used_default: bool,
}

impl ListFormatLocaleNegotiation {
    /// Returns the matcher used for this negotiation.
    pub fn matcher(&self) -> LocaleMatcher {
        self.matcher
    }

    /// Returns every requested locale and its data-support decision.
    pub fn candidates(&self) -> &[ListFormatLocaleCandidate] {
        &self.candidates
    }

    /// Returns the locale selected for list formatting.
    pub fn selected(&self) -> &CanonicalLocale {
        &self.selected
    }

    /// Returns whether the stable `en-US` service default was selected.
    pub fn used_default(&self) -> bool {
        self.used_default
    }
}

/// Negotiates a requested locale list against bundled list-pattern data.
pub fn negotiate_list_format_locale(
    requested: &[CanonicalLocale],
    matcher: LocaleMatcher,
) -> ListFormatLocaleNegotiation {
    let candidates = requested
        .iter()
        .cloned()
        .map(|requested| ListFormatLocaleCandidate {
            supported: locale_data_provider()
                .supports_service_locale(IntlService::ListFormat, requested.locale()),
            requested,
        })
        .collect::<Vec<_>>();
    let selected = candidates
        .iter()
        .find(|candidate| candidate.supported)
        .map(|candidate| candidate.requested.clone());
    let used_default = selected.is_none();
    ListFormatLocaleNegotiation {
        matcher,
        candidates,
        selected: selected
            .unwrap_or_else(|| canonicalize("en-US").expect("the default locale is valid")),
        used_default,
    }
}

/// Resolves one requested locale against bundled list-pattern data.
pub fn resolve_list_format_locale(
    requested: &[CanonicalLocale],
    matcher: LocaleMatcher,
) -> CanonicalLocale {
    negotiate_list_format_locale(requested, matcher)
        .selected
        .clone()
}

/// Returns the requested locales supported by the bundled list service.
pub fn supported_list_format_locales(
    requested: &[CanonicalLocale],
    matcher: LocaleMatcher,
) -> Vec<CanonicalLocale> {
    negotiate_list_format_locale(requested, matcher)
        .candidates
        .into_iter()
        .filter(|candidate| candidate.supported)
        .map(|candidate| candidate.requested)
        .collect()
}

/// Host-neutral options for constructing an `Intl.ListFormat` service.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub struct ListFormatOptions {
    /// The requested locale matching policy.
    pub locale_matcher: LocaleMatcher,
    /// The relation joined by the list patterns.
    pub list_type: ListType,
    /// The requested CLDR list-pattern width.
    pub style: ListStyle,
}

/// ECMAScript-observable data resolved by a list-format service.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ResolvedListFormatOptions {
    /// The negotiated locale.
    pub locale: String,
    /// The selected list type.
    pub list_type: ListType,
    /// The selected list style.
    pub style: ListStyle,
}

/// The `type` field of one `Intl.ListFormat.prototype.formatToParts` result.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum ListPartKind {
    /// An input list element.
    Element,
    /// A locale-provided list literal such as a comma or conjunction.
    Literal,
}

/// One host-neutral `Intl.ListFormat.prototype.formatToParts` result.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ListPart {
    /// Whether this is an input element or a locale-provided literal.
    pub kind: ListPartKind,
    /// The corresponding UTF-8 string segment.
    pub value: String,
}

/// A failure while constructing a list formatter.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum ListFormatError {
    /// The selected list-pattern data was unavailable.
    DataUnavailable,
    /// ICU4X could not write the formatted list to the host-neutral collector.
    FormattingFailed,
}

impl std::fmt::Display for ListFormatError {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::DataUnavailable => formatter.write_str("list-pattern data is unavailable"),
            Self::FormattingFailed => formatter.write_str("could not collect list-format parts"),
        }
    }
}

impl std::error::Error for ListFormatError {}

/// A host-neutral `Intl.ListFormat` service backed by ICU4X.
///
/// Input item coercion remains an embedding concern. The service formats the
/// resulting strings using the negotiated CLDR list patterns and exposes their
/// literal/element boundaries without needing a JavaScript Realm.
pub struct ListFormat {
    formatter: IcuListFormatter,
    negotiation: ListFormatLocaleNegotiation,
    resolved: ResolvedListFormatOptions,
}

impl ListFormat {
    /// Constructs a list formatter after locale negotiation and option
    /// resolution.
    pub fn try_new(
        requested: &[CanonicalLocale],
        options: ListFormatOptions,
    ) -> Result<Self, ListFormatError> {
        let negotiation = negotiate_list_format_locale(requested, options.locale_matcher);
        let selected = negotiation.selected.clone();
        let preferences: ListFormatterPreferences = selected.locale().into();
        let formatter_options =
            IcuListFormatterOptions::default().with_length(options.style.into());
        let formatter = match options.list_type {
            ListType::Conjunction => IcuListFormatter::try_new_and(preferences, formatter_options),
            ListType::Disjunction => IcuListFormatter::try_new_or(preferences, formatter_options),
            ListType::Unit => IcuListFormatter::try_new_unit(preferences, formatter_options),
        }
        .map_err(|_| ListFormatError::DataUnavailable)?;
        Ok(Self {
            formatter,
            negotiation,
            resolved: ResolvedListFormatOptions {
                locale: selected.as_str().into(),
                list_type: options.list_type,
                style: options.style,
            },
        })
    }

    /// Formats a sequence of already-coerced string items.
    pub fn format<I, S>(&self, values: I) -> String
    where
        I: IntoIterator<Item = S>,
        S: AsRef<str>,
    {
        let values = values
            .into_iter()
            .map(|value| value.as_ref().to_owned())
            .collect::<Vec<_>>();
        self.formatter
            .format(values.iter().map(String::as_str))
            .to_string()
    }

    /// Formats a sequence of already-coerced string items into ECMA-402 parts.
    pub fn format_to_parts<I, S>(&self, values: I) -> Result<Vec<ListPart>, ListFormatError>
    where
        I: IntoIterator<Item = S>,
        S: AsRef<str>,
    {
        let values = values
            .into_iter()
            .map(|value| value.as_ref().to_owned())
            .collect::<Vec<_>>();
        let mut collector = ListPartCollector::default();
        self.formatter
            .format(values.iter().map(String::as_str))
            .write_to_parts(&mut collector)
            .map_err(|_| ListFormatError::FormattingFailed)?;
        Ok(collector.parts)
    }

    /// Returns the data selected during construction.
    pub fn resolved_options(&self) -> &ResolvedListFormatOptions {
        &self.resolved
    }

    /// Returns the locale-negotiation trace produced during construction.
    pub fn negotiation(&self) -> &ListFormatLocaleNegotiation {
        &self.negotiation
    }

    /// Returns the heap storage directly owned by this service.
    pub fn bytes(&self) -> usize {
        std::mem::size_of::<Self>() + self.resolved.locale.len()
    }
}

#[derive(Default)]
pub(crate) struct ListPartCollector {
    pub(crate) parts: Vec<ListPart>,
    stack: Vec<ListPartKind>,
}

impl std::fmt::Write for ListPartCollector {
    fn write_str(&mut self, value: &str) -> std::fmt::Result {
        if value.is_empty() {
            return Ok(());
        }
        let kind = self.stack.last().copied().unwrap_or(ListPartKind::Literal);
        if let Some(part) = self.parts.last_mut().filter(|part| part.kind == kind) {
            part.value.push_str(value);
        } else {
            self.parts.push(ListPart {
                kind,
                value: value.into(),
            });
        }
        Ok(())
    }
}

impl PartsWrite for ListPartCollector {
    type SubPartsWrite = Self;

    fn with_part(
        &mut self,
        part: Part,
        mut write: impl FnMut(&mut Self::SubPartsWrite) -> std::fmt::Result,
    ) -> std::fmt::Result {
        let kind = if part == icu_list::parts::ELEMENT {
            ListPartKind::Element
        } else {
            ListPartKind::Literal
        };
        self.stack.push(kind);
        let result = write(self);
        self.stack.pop();
        result
    }
}
