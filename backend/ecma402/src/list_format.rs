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
    let resolution = resolve_locale(IntlService::ListFormat, requested, matcher);
    ListFormatLocaleNegotiation {
        matcher: resolution.matcher(),
        candidates: resolution
            .candidates()
            .iter()
            .map(|candidate| ListFormatLocaleCandidate {
                requested: candidate.requested().clone(),
                supported: candidate.is_supported(),
            })
            .collect(),
        selected: resolution.selected().clone(),
        used_default: resolution.used_default(),
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
    supported_locales(IntlService::ListFormat, requested, matcher)
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
    formatter: ListFormatterBackend,
    negotiation: ListFormatLocaleNegotiation,
    resolved: ResolvedListFormatOptions,
}

enum ListFormatterBackend {
    /// ICU4X retains contextual variants such as Spanish `y` → `e`.
    Icu(IcuListFormatter),
    /// Pinned raw CLDR data fills locales absent from the compact ICU bundle.
    Pinned(crate::locale_data::list_patterns::PinnedListPatterns),
}

impl ListFormatterBackend {
    fn bytes(&self) -> usize {
        match self {
            Self::Icu(_) => 0,
            Self::Pinned(patterns) => patterns.bytes(),
        }
    }
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
        let icu_formatter = match options.list_type {
            ListType::Conjunction => IcuListFormatter::try_new_and(preferences, formatter_options),
            ListType::Disjunction => IcuListFormatter::try_new_or(preferences, formatter_options),
            ListType::Unit => IcuListFormatter::try_new_unit(preferences, formatter_options),
        };
        let provider = crate::locale_data_provider();
        let pinned = || {
            provider
                .list_patterns(selected.as_str(), options.list_type, options.style)
                .map(ListFormatterBackend::Pinned)
                .ok_or(ListFormatError::DataUnavailable)
        };
        let formatter = if provider.supports_language(selected.locale()) {
            match icu_formatter {
                Ok(formatter) => ListFormatterBackend::Icu(formatter),
                Err(_) => pinned()?,
            }
        } else {
            // ICU's compact bundle can incidentally carry individual records
            // outside its declared language inventory. Select the pinned CLDR
            // table for those locales so their observable output is tied to
            // this provider revision rather than a changing ICU subset.
            pinned()?
        };
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
        match &self.formatter {
            ListFormatterBackend::Icu(formatter) => formatter
                .format(values.iter().map(String::as_str))
                .to_string(),
            ListFormatterBackend::Pinned(patterns) => format_pinned_list(patterns, values)
                .into_iter()
                .map(|part| part.value)
                .collect(),
        }
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
        match &self.formatter {
            ListFormatterBackend::Icu(formatter) => {
                let mut collector = ListPartCollector::default();
                formatter
                    .format(values.iter().map(String::as_str))
                    .write_to_parts(&mut collector)
                    .map_err(|_| ListFormatError::FormattingFailed)?;
                Ok(collector.parts)
            }
            ListFormatterBackend::Pinned(patterns) => Ok(format_pinned_list(patterns, values)),
        }
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
        std::mem::size_of::<Self>() + self.resolved.locale.len() + self.formatter.bytes()
    }
}

fn format_pinned_list(
    patterns: &crate::locale_data::list_patterns::PinnedListPatterns,
    values: Vec<String>,
) -> Vec<ListPart> {
    let mut values = values
        .into_iter()
        .map(|value| {
            vec![ListPart {
                kind: ListPartKind::Element,
                value,
            }]
        })
        .collect::<Vec<_>>();
    match values.len() {
        0 => Vec::new(),
        1 => values.pop().expect("one list element remains"),
        2 => join_pinned_list_parts(patterns.two.as_str(), values.remove(0), values.remove(0)),
        _ => {
            let mut result =
                join_pinned_list_parts(patterns.start.as_str(), values.remove(0), values.remove(0));
            while values.len() > 1 {
                result = join_pinned_list_parts(patterns.middle.as_str(), result, values.remove(0));
            }
            join_pinned_list_parts(
                patterns.end.as_str(),
                result,
                values.pop().expect("final list element remains"),
            )
        }
    }
}

fn join_pinned_list_parts(
    pattern: &str,
    first: Vec<ListPart>,
    second: Vec<ListPart>,
) -> Vec<ListPart> {
    let mut result = Vec::new();
    let mut remainder = pattern;
    let mut first = Some(first);
    let mut second = Some(second);
    while let Some(index) = remainder.find(['{', '}']) {
        if index > 0 {
            push_list_part(&mut result, ListPartKind::Literal, &remainder[..index]);
        }
        let placeholder = remainder
            .get(index..index + 3)
            .expect("validated CLDR list pattern has complete placeholder");
        let parts = match placeholder {
            "{0}" => first.take().expect("CLDR list pattern contains {{0}} once"),
            "{1}" => second
                .take()
                .expect("CLDR list pattern contains {{1}} once"),
            _ => unreachable!("validated CLDR list pattern only has {{0}}/{{1}} placeholders"),
        };
        for part in parts {
            push_list_part(&mut result, part.kind, &part.value);
        }
        remainder = &remainder[index + 3..];
    }
    if !remainder.is_empty() {
        push_list_part(&mut result, ListPartKind::Literal, remainder);
    }
    debug_assert!(first.is_none() && second.is_none());
    result
}

fn push_list_part(parts: &mut Vec<ListPart>, kind: ListPartKind, value: &str) {
    if value.is_empty() {
        return;
    }
    // ECMA-402 `formatToParts` must retain an individual element boundary for
    // every input.  Narrow CLDR patterns such as Chinese's `{0}{1}` have no
    // literal between the placeholders, so coalescing neighbouring elements
    // would incorrectly turn two inputs into one part.
    if kind == ListPartKind::Literal {
        if let Some(previous) = parts
            .last_mut()
            .filter(|previous| previous.kind == ListPartKind::Literal)
        {
            previous.value.push_str(value);
            return;
        }
    }
    parts.push(ListPart {
        kind,
        value: value.into(),
    });
}

#[derive(Default)]
pub(crate) struct ListPartCollector {
    pub(crate) parts: Vec<ListPart>,
    stack: Vec<ListPartKind>,
    /// The next write inside an ICU element part begins a distinct ECMA-402
    /// element, even when it immediately follows another element.
    next_element_write_starts_part: bool,
}

impl std::fmt::Write for ListPartCollector {
    fn write_str(&mut self, value: &str) -> std::fmt::Result {
        if value.is_empty() {
            return Ok(());
        }
        let kind = self.stack.last().copied().unwrap_or(ListPartKind::Literal);
        let must_start_element = kind == ListPartKind::Element
            && std::mem::take(&mut self.next_element_write_starts_part);
        if !must_start_element {
            if let Some(part) = self.parts.last_mut().filter(|part| part.kind == kind) {
                part.value.push_str(value);
                return Ok(());
            }
        }
        self.parts.push(ListPart {
            kind,
            value: value.into(),
        });
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
        if kind == ListPartKind::Element {
            self.next_element_write_starts_part = true;
        }
        self.stack.push(kind);
        let result = write(self);
        self.stack.pop();
        result
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn pinned_patterns_preserve_adjacent_input_boundaries() {
        let patterns =
            crate::locale_data::list_patterns::patterns("zh", ListType::Unit, ListStyle::Wide)
                .expect("Chinese unit list patterns are pinned");

        assert_eq!(patterns.two, "{0}{1}");
        assert_eq!(
            format_pinned_list(&patterns, vec!["A".into(), "B".into()]),
            vec![
                ListPart {
                    kind: ListPartKind::Element,
                    value: "A".into(),
                },
                ListPart {
                    kind: ListPartKind::Element,
                    value: "B".into(),
                },
            ]
        );
    }
}
