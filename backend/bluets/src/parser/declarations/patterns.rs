// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

//! Destructuring patterns in parameter lists.
//!
//! Object and array targets retain nested binding boundaries.
//! The supported subset is an object pattern (`{ a, b: c, d = 1 }`) or a
//! array pattern (`[a, , b = 1]`). Anything else (a rest
//! element, a computed or string key) is reported as unsupported by the caller.

use super::*;

/// Parses the pattern that opens at `start` (a `{` or `[`), returning it and
/// the index of its closing bracket, or `None` when it is outside the subset.
pub(in crate::parser::implementation) fn parse_binding_pattern(
    tokens: &[Token],
    start: usize,
    module: &str,
    depth: usize,
) -> Option<(BindingPattern, usize)> {
    parse(tokens, start, module, false, depth).map(|(pattern, _, close)| (pattern, close))
}

pub(crate) fn parse_variable_pattern(
    tokens: &[Token],
    start: usize,
    module: &str,
    depth: usize,
) -> Option<(VariableBindingPattern, usize)> {
    parse(tokens, start, module, true, depth)
        .map(|(pattern, rest, close)| (VariableBindingPattern { pattern, rest }, close))
}

fn parse(
    tokens: &[Token],
    start: usize,
    module: &str,
    allow_rest: bool,
    depth: usize,
) -> Option<(BindingPattern, Option<ElementBinding>, usize)> {
    if depth == 0 {
        return None;
    }
    let mut rest = None;
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
                rest,
                index,
            ));
        }
        if allow_rest && token.is("...") {
            let name = tokens.get(index + 1)?;
            if name.kind != TokenKind::Identifier || !tokens.get(index + 2)?.is(closing) {
                return None;
            }
            rest = Some(ElementBinding {
                name: name.text.clone(),
                nested: None,
                default: None,
                span: SourceSpan::new(module, token.start, name.end),
            });
            index += 2;
            continue;
        }
        if object {
            let key = token;
            if !matches!(key.kind, TokenKind::Identifier | TokenKind::Keyword) {
                return None;
            }
            let target = if tokens.get(index + 1)?.is(":") {
                index + 2
            } else {
                index
            };
            let (name, nested, after_name) =
                if matches!(tokens.get(target)?.text.as_str(), "{" | "[") {
                    let (pattern, rest, close) =
                        parse(tokens, target, module, allow_rest, depth - 1)?;
                    (
                        String::new(),
                        Some(Box::new(VariableBindingPattern { pattern, rest })),
                        close + 1,
                    )
                } else {
                    let name = tokens.get(target)?;
                    if !matches!(name.kind, TokenKind::Identifier | TokenKind::Keyword) {
                        return None;
                    }
                    (name.text.clone(), None, target + 1)
                };
            let (default, after) = binding_default(tokens, after_name, closing)?;
            objects.push(ObjectBinding {
                key: key.text.clone(),
                name,
                nested,
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
            let (name, nested, after_name) = if matches!(token.text.as_str(), "{" | "[") {
                let (pattern, rest, close) = parse(tokens, index, module, allow_rest, depth - 1)?;
                (
                    String::new(),
                    Some(Box::new(VariableBindingPattern { pattern, rest })),
                    close + 1,
                )
            } else {
                if token.kind != TokenKind::Identifier {
                    return None;
                }
                (token.text.clone(), None, index + 1)
            };
            let (default, after) = binding_default(tokens, after_name, closing)?;
            elements.push(Some(ElementBinding {
                name,
                nested,
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

impl VariableBindingPattern {
    pub(crate) fn names(&self) -> Vec<(&str, &SourceSpan)> {
        let mut names = self.pattern.names();
        if let Some(rest) = &self.rest {
            if let Some(nested) = &rest.nested {
                names.extend(nested.names());
            } else {
                names.push((&rest.name, &rest.span));
            }
        }
        names
    }
}
impl BindingPattern {
    pub(crate) fn names(&self) -> Vec<(&str, &SourceSpan)> {
        let mut names = Vec::new();
        match self {
            Self::Object(bindings) => {
                for binding in bindings {
                    if let Some(nested) = &binding.nested {
                        names.extend(nested.names());
                    } else {
                        names.push((binding.name.as_str(), &binding.span));
                    }
                }
            }
            Self::Array(bindings) => {
                for binding in bindings.iter().flatten() {
                    if let Some(nested) = &binding.nested {
                        names.extend(nested.names());
                    } else {
                        names.push((binding.name.as_str(), &binding.span));
                    }
                }
            }
        }
        names
    }
}
