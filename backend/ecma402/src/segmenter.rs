// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

//! Host-neutral implementation of the ECMA-402 segmentation service.

use super::*;

/// The ECMA-402 segmentation granularity to use.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub enum SegmenterGranularity {
    /// Extended grapheme-cluster boundaries.
    #[default]
    Grapheme,
    /// Word boundaries, including non-word-like punctuation and whitespace.
    Word,
    /// Sentence boundaries.
    Sentence,
}

/// One canonical locale considered during segmenter negotiation.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct SegmenterLocaleCandidate {
    requested: CanonicalLocale,
    supported: bool,
}

impl SegmenterLocaleCandidate {
    /// Returns the canonical locale supplied by the host.
    pub fn requested(&self) -> &CanonicalLocale {
        &self.requested
    }

    /// Returns whether the bundled segmentation data supports this request.
    pub fn is_supported(&self) -> bool {
        self.supported
    }
}

/// A deterministic trace of segmenter locale negotiation.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct SegmenterLocaleNegotiation {
    matcher: LocaleMatcher,
    candidates: Vec<SegmenterLocaleCandidate>,
    selected: CanonicalLocale,
    used_default: bool,
}

impl SegmenterLocaleNegotiation {
    /// Returns the matcher used for this negotiation.
    pub fn matcher(&self) -> LocaleMatcher {
        self.matcher
    }

    /// Returns every requested locale and its data-support decision.
    pub fn candidates(&self) -> &[SegmenterLocaleCandidate] {
        &self.candidates
    }

    /// Returns the locale selected for the segmenter service.
    pub fn selected(&self) -> &CanonicalLocale {
        &self.selected
    }

    /// Returns whether the stable `en-US` service default was selected.
    pub fn used_default(&self) -> bool {
        self.used_default
    }
}

/// Returns whether the bundled segmentation data supports this locale.
pub fn supports_segmenter_locale(locale: &IcuLocale) -> bool {
    locale_data_provider().supports_service_locale(IntlService::Segmenter, locale)
}

/// Negotiates requested locales against the bundled segmenter service.
pub fn negotiate_segmenter_locale(
    requested: &[CanonicalLocale],
    matcher: LocaleMatcher,
) -> SegmenterLocaleNegotiation {
    let resolution = resolve_locale(IntlService::Segmenter, requested, matcher);
    SegmenterLocaleNegotiation {
        matcher: resolution.matcher(),
        candidates: resolution
            .candidates()
            .iter()
            .map(|candidate| SegmenterLocaleCandidate {
                requested: candidate.requested().clone(),
                supported: candidate.is_supported(),
            })
            .collect(),
        selected: resolution.selected().clone(),
        used_default: resolution.used_default(),
    }
}

/// Resolves one requested locale against the bundled segmenter service.
pub fn resolve_segmenter_locale(
    requested: &[CanonicalLocale],
    matcher: LocaleMatcher,
) -> CanonicalLocale {
    negotiate_segmenter_locale(requested, matcher)
        .selected
        .clone()
}

/// Returns requested locales supported by the bundled segmenter service.
pub fn supported_segmenter_locales(
    requested: &[CanonicalLocale],
    matcher: LocaleMatcher,
) -> Vec<CanonicalLocale> {
    supported_locales(IntlService::Segmenter, requested, matcher)
}

/// Host-neutral options for constructing an `Intl.Segmenter` service.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub struct SegmenterOptions {
    /// The requested locale matching policy.
    pub locale_matcher: LocaleMatcher,
    /// The requested segmentation granularity.
    pub granularity: SegmenterGranularity,
}

/// ECMAScript-observable data resolved by a segmenter service.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ResolvedSegmenterOptions {
    /// The negotiated locale.
    pub locale: String,
    /// The selected segmentation granularity.
    pub granularity: SegmenterGranularity,
}

/// One host-neutral `Intl.Segmenter` segment result.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct SegmenterSegment {
    /// The corresponding input substring.
    pub segment: String,
    /// Its index in the original input, counted in UTF-16 code units.
    pub index_utf16: usize,
    /// Whether this is word-like. It is only meaningful for word granularity.
    pub is_word_like: Option<bool>,
}

/// A failure while constructing a segmenter.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum SegmenterError {
    /// The selected locale's segmentation data was unavailable.
    DataUnavailable,
}

impl std::fmt::Display for SegmenterError {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::DataUnavailable => formatter.write_str("segmentation data is unavailable"),
        }
    }
}

impl std::error::Error for SegmenterError {}

/// A host-neutral `Intl.Segmenter` service backed by ICU4X.
///
/// Input coercion and iterator-object mechanics remain embedding concerns. The
/// service returns fully materialized segments so a host can map them directly
/// into ECMA-402 `Segments` iterator results without retaining its input.
pub struct Segmenter {
    backend: SegmenterBackend,
    negotiation: SegmenterLocaleNegotiation,
    resolved: ResolvedSegmenterOptions,
}

enum SegmenterBackend {
    Grapheme(GraphemeClusterSegmenterBorrowed<'static>),
    Word(Box<WordSegmenter>),
    Sentence(Box<SentenceSegmenter>),
}

impl Segmenter {
    /// Constructs a segmenter after locale negotiation and option resolution.
    pub fn try_new(
        requested: &[CanonicalLocale],
        options: SegmenterOptions,
    ) -> Result<Self, SegmenterError> {
        let negotiation = negotiate_segmenter_locale(requested, options.locale_matcher);
        let selected = negotiation.selected.clone();
        let backend = match options.granularity {
            SegmenterGranularity::Grapheme => {
                SegmenterBackend::Grapheme(GraphemeClusterSegmenter::new())
            }
            SegmenterGranularity::Word => {
                let language = selected.locale().id.clone();
                let mut word_options = WordBreakOptions::default();
                word_options.content_locale = Some(&language);
                SegmenterBackend::Word(Box::new(
                    WordSegmenter::try_new_auto(word_options)
                        .map_err(|_| SegmenterError::DataUnavailable)?,
                ))
            }
            SegmenterGranularity::Sentence => {
                let language = selected.locale().id.clone();
                let mut sentence_options = SentenceBreakOptions::default();
                sentence_options.content_locale = Some(&language);
                SegmenterBackend::Sentence(Box::new(
                    SentenceSegmenter::try_new(sentence_options)
                        .map_err(|_| SegmenterError::DataUnavailable)?,
                ))
            }
        };
        Ok(Self {
            backend,
            negotiation,
            resolved: ResolvedSegmenterOptions {
                locale: selected.as_str().into(),
                granularity: options.granularity,
            },
        })
    }

    /// Segments an already-coerced string at the selected granularity.
    pub fn segment(&self, input: &str) -> Vec<SegmenterSegment> {
        let input_utf16 = input.encode_utf16().collect::<Vec<_>>();
        match &self.backend {
            SegmenterBackend::Grapheme(segmenter) => segmenter_segments(
                &input_utf16,
                segmenter.segment_utf16(&input_utf16).map(|end| (end, None)),
            ),
            SegmenterBackend::Word(segmenter) => segmenter_segments(
                &input_utf16,
                segmenter
                    .as_borrowed()
                    .segment_utf16(&input_utf16)
                    .iter_with_word_type()
                    .map(|(end, word_type)| (end, Some(word_type.is_word_like()))),
            ),
            SegmenterBackend::Sentence(segmenter) => segmenter_segments(
                &input_utf16,
                segmenter
                    .as_borrowed()
                    .segment_utf16(&input_utf16)
                    .map(|end| (end, None)),
            ),
        }
    }

    /// Returns the data selected during construction.
    pub fn resolved_options(&self) -> &ResolvedSegmenterOptions {
        &self.resolved
    }

    /// Returns the locale-negotiation trace produced during construction.
    pub fn negotiation(&self) -> &SegmenterLocaleNegotiation {
        &self.negotiation
    }

    /// Returns the heap storage directly owned by this service.
    pub fn bytes(&self) -> usize {
        std::mem::size_of::<Self>() + self.resolved.locale.len()
    }
}

pub(crate) fn segmenter_segments<I>(input_utf16: &[u16], boundaries: I) -> Vec<SegmenterSegment>
where
    I: IntoIterator<Item = (usize, Option<bool>)>,
{
    let mut start = 0;
    let mut segments = Vec::new();
    for (end, is_word_like) in boundaries {
        if end == start {
            continue;
        }
        segments.push(SegmenterSegment {
            segment: String::from_utf16(&input_utf16[start..end])
                .expect("ICU4X boundaries preserve valid UTF-16"),
            index_utf16: start,
            is_word_like,
        });
        start = end;
    }
    segments
}
