// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

//! JSX diagnostics use the existing tree's original-source names.
use crate::diagnostic::{Diagnostic, SourceSpan};
use crate::jsx::JsxAttribute;

pub(super) fn refine(diagnostic: &Diagnostic, source: &str) -> Option<SourceSpan> {
    let counterpart = diagnostic.typescript.as_ref()?;
    let span = &counterpart.span;
    let range = match counterpart.code {
        1382 if diagnostic.message.contains("JSX text") => (span.start, span.start.checked_add(1)?),
        17002 => {
            let opening = source.get(..span.start)?.rfind("</")? + 2;
            (opening, name_end(source, opening))
        }
        17008 => {
            let opening = source.get(..span.start)?.rfind('<')? + 1;
            (opening, name_end(source, opening))
        }
        2322 | 2786 | 2874 | 2304 => {
            let is_jsx = diagnostic.message.starts_with("attribute ")
                || diagnostic.message.starts_with("spread attribute ")
                || diagnostic.message.starts_with("missing required props")
                || diagnostic.message.starts_with("this element has children")
                || diagnostic.message.starts_with("the element's children")
                || counterpart.code == 2786
                || counterpart.code == 2874
                || diagnostic
                    .message
                    .starts_with("cannot find the JSX tag name");
            if !is_jsx {
                return None;
            }
            if diagnostic.message.starts_with("the element's children")
                && source.get(span.start..)?.starts_with('<')
            {
                return Some(span.clone());
            }
            let opening = if source.get(span.start..)?.starts_with('<') {
                span.start
            } else {
                crate::syntax::lex(&span.module, source)
                    .ok()?
                    .into_iter()
                    .filter(|token| token.kind == crate::syntax::TokenKind::JsxElement)
                    .filter_map(|token| {
                        crate::syntax::parse_jsx(&span.module, source, token.start).ok()
                    })
                    .find_map(|element| enclosing_start(&element, span))?
            };
            let element = crate::syntax::parse_jsx(&span.module, source, opening).ok()?;
            if diagnostic.message.starts_with("attribute ") {
                let name = diagnostic.message.split('`').nth(1)?;
                let attribute =
                    element
                        .attributes
                        .iter()
                        .find_map(|attribute| match attribute {
                            JsxAttribute::Named { name: actual, .. } if actual.text == name => {
                                Some(actual)
                            }
                            _ => None,
                        })?;
                (attribute.start, attribute.end)
            } else {
                let name = element.name?;
                (name.start, name.end)
            }
        }
        _ => return None,
    };
    Some(SourceSpan::new(&span.module, range.0, range.1))
}

fn name_end(source: &str, start: usize) -> usize {
    source[start..]
        .char_indices()
        .find(|(_, ch)| !ch.is_alphanumeric() && !matches!(ch, '_' | '$' | '-' | '.' | ':'))
        .map_or(source.len(), |(offset, _)| start + offset)
}

fn enclosing_start(element: &crate::jsx::JsxElement, span: &SourceSpan) -> Option<usize> {
    if span.start < element.start || element.end < span.end {
        return None;
    }
    for attribute in &element.attributes {
        if let JsxAttribute::Named {
            value: Some(crate::jsx::JsxValue::Element(inner)),
            ..
        } = attribute
        {
            if let Some(start) = enclosing_start(inner, span) {
                return Some(start);
            }
        }
    }
    for child in &element.children {
        if let crate::jsx::JsxChild::Element(inner) = child {
            if let Some(start) = enclosing_start(inner, span) {
                return Some(start);
            }
        }
    }
    Some(element.start)
}
