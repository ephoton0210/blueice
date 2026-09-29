// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

//! Destructuring patterns in parameter lists.
//!
//! The supported subset is a flat object pattern (`{ a, b: c, d = 1 }`) or a
//! flat array pattern (`[a, , b = 1]`). Anything else (a nested pattern, a rest
//! element, a computed or string key) is reported as unsupported by the caller.

use super::*;

/// Parses the pattern that opens at `start` (a `{` or `[`), returning it and
/// the index of its closing bracket, or `None` when it is outside the subset.
pub(in crate::parser::implementation) fn parse_binding_pattern(
    tokens: &[Token],
    start: usize,
    module: &str,
) -> Option<(BindingPattern, usize)> {
    let object = tokens.get(start)?.is("{");
    let closing = if object { "}" } else { "]" };
    let mut objects = Vec::new();
    let mut elements = Vec::new();
    let mut index = start + 1;
    loop {
        let token = tokens.get(index)?;
        if token.is(closing) {
            return Some((
                if object {
                    BindingPattern::Object(objects)
                } else {
                    BindingPattern::Array(elements)
                },
                index,
            ));
        }
        if object {
            let key = token;
            if !matches!(key.kind, TokenKind::Identifier | TokenKind::Keyword) {
                return None;
            }
            let (name, after_name) = if tokens.get(index + 1)?.is(":") {
                let name = tokens.get(index + 2)?;
                if name.kind != TokenKind::Identifier {
                    return None;
                }
                (name, index + 3)
            } else {
                (key, index + 1)
            };
            let (default, after) = binding_default(tokens, after_name, closing)?;
            objects.push(ObjectBinding {
                key: key.text.clone(),
                name: name.text.clone(),
                default,
                span: SourceSpan::new(module, key.start, tokens[after - 1].end),
            });
            index = after;
        } else if token.is(",") {
            // A skipped element.
            elements.push(None);
            index += 1;
            continue;
        } else {
            if token.kind != TokenKind::Identifier {
                return None;
            }
            let (default, after) = binding_default(tokens, index + 1, closing)?;
            elements.push(Some(ElementBinding {
                name: token.text.clone(),
                default,
                span: SourceSpan::new(module, token.start, tokens[after - 1].end),
            }));
            index = after;
        }
        // After an element: a comma continues, the closing bracket ends.
        match tokens.get(index)? {
            next if next.is(",") => index += 1,
            next if next.is(closing) => {}
            _ => return None,
        }
    }
}

/// An optional `= expression` after a binding, up to the next `,` or the
/// closing bracket; returns the tokens and the index after them.
fn binding_default(
    tokens: &[Token],
    index: usize,
    closing: &str,
) -> Option<(Option<Vec<Token>>, usize)> {
    if !tokens.get(index)?.is("=") {
        return Some((None, index));
    }
    let end = find_balanced_delimiter(tokens, index + 1, tokens.len() - 1, &[",", closing]);
    if end == index + 1 {
        return None;
    }
    Some((Some(tokens[index + 1..end].to_vec()), end))
}
