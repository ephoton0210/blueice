// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

//! Delimiter roles determine layout without interpreting or evaluating code.

use super::*;

#[derive(Clone, Copy, PartialEq, Eq)]
pub(super) enum Kind {
    Root,
    Class,
    Constructor,
    Factory,
    Namespace,
    Control,
    Switch,
    Function { inline: bool, expression: bool },
    Object { inline: bool },
}
impl Kind {
    pub(super) fn multiline(self) -> bool {
        matches!(
            self,
            Self::Class
                | Self::Constructor
                | Self::Factory
                | Self::Namespace
                | Self::Control
                | Self::Switch
                | Self::Function { inline: false, .. }
                | Self::Object { inline: false }
        )
    }
    pub(super) fn object(self) -> bool {
        matches!(self, Self::Object { .. })
    }
}

#[derive(Clone, Copy)]
pub(super) struct Context {
    pub(super) kind: Kind,
    pub(super) indent: usize,
    pub(super) in_for: bool,
}
pub(super) struct Roles {
    pub(super) openings: Vec<Option<Kind>>,
    pub(super) closings: Vec<Option<Kind>>,
    pub(super) before: Vec<Context>,
}

impl Roles {
    pub(super) fn new(tokens: &[Token], text: &str, module: &Module) -> Self {
        let mut pairs = vec![None; tokens.len()];
        let mut delimiters = Vec::new();
        for (i, token) in tokens.iter().enumerate() {
            if matches!(token.text.as_str(), "(" | "[" | "{") {
                delimiters.push(i);
            } else if matches!(token.text.as_str(), ")" | "]" | "}") {
                if let Some(open) = delimiters.pop() {
                    pairs[i] = Some(open);
                    pairs[open] = Some(i);
                }
            }
        }
        fn names(declarations: &[Declaration], out: &mut BTreeSet<String>) {
            for declaration in declarations {
                match declaration {
                    Declaration::Namespace(n) => {
                        out.insert(n.name.clone());
                        names(&n.body, out);
                    }
                    Declaration::Enum(e) => {
                        out.insert(e.name.clone());
                    }
                    _ => {}
                }
            }
        }
        let mut namespaces = BTreeSet::new();
        names(&module.declarations, &mut namespaces);
        let mut result = Self {
            openings: vec![None; tokens.len()],
            closings: vec![None; tokens.len()],
            before: Vec::with_capacity(tokens.len()),
        };
        let mut braces = Vec::<Kind>::new();
        let mut parens = Vec::<bool>::new();
        let mut indent = 0;
        for (i, token) in tokens.iter().enumerate() {
            let parent = braces.last().copied().unwrap_or(Kind::Root);
            result.before.push(Context {
                kind: parent,
                indent,
                in_for: parens.iter().any(|v| *v),
            });
            if token.is("(") {
                parens.push(i > 0 && tokens[i - 1].is("for"));
            }
            if token.is(")") {
                parens.pop();
            }
            if token.is("{") {
                let kind = classify(tokens, i, &pairs, text, parent, &namespaces);
                result.openings[i] = Some(kind);
                braces.push(kind);
                indent += usize::from(kind.multiline());
            } else if token.is("}") {
                if let Some(kind) = braces.pop() {
                    result.closings[i] = Some(kind);
                    indent = indent.saturating_sub(usize::from(kind.multiline()));
                }
            }
        }
        result
    }
}

fn classify(
    tokens: &[Token],
    i: usize,
    pairs: &[Option<usize>],
    text: &str,
    parent: Kind,
    namespaces: &BTreeSet<String>,
) -> Kind {
    let close = pairs[i].unwrap_or(i);
    let multiline = text[tokens[i].end..tokens[close].start].contains(['\r', '\n']);
    let prev = i.checked_sub(1).map(|j| &tokens[j]);
    if prev.is_some_and(|p| p.is("export") || p.is("import")) {
        return Kind::Object { inline: true };
    }
    let mut j = i;
    while j > 0 {
        j -= 1;
        if tokens[j].is("class") {
            return Kind::Class;
        }
        if matches!(tokens[j].text.as_str(), ";" | "{" | "}" | "=" | "=>") {
            break;
        }
        if tokens[j].is(")") {
            break;
        }
    }
    if prev.is_some_and(|p| p.is(")")) {
        if let Some(open) = pairs[i - 1] {
            let before = open.checked_sub(1).map(|j| &tokens[j]);
            if before.is_some_and(|p| p.is("switch")) {
                return Kind::Switch;
            }
            if before.is_some_and(|p| {
                matches!(
                    p.text.as_str(),
                    "if" | "for" | "while" | "switch" | "with" | "catch"
                )
            }) {
                return Kind::Control;
            }
            if parent == Kind::Class && before.is_some_and(|p| p.is("constructor")) {
                return Kind::Constructor;
            }
            let function = if before.is_some_and(|p| p.is("function")) {
                open.checked_sub(1)
            } else {
                open.checked_sub(2).filter(|j| tokens[*j].is("function"))
            };
            if let Some(function) = function {
                let prefix = &text[..tokens[function].start];
                if prefix.rfind("/** @class */").is_some_and(|at| {
                    prefix[at + 13..]
                        .chars()
                        .all(|c| c.is_whitespace() || c == '(')
                }) {
                    return Kind::Factory;
                }
                if open + 2 == i - 1 && namespaces.contains(&tokens[open + 1].text) {
                    return Kind::Namespace;
                }
                if parent == Kind::Factory && open > function + 1 {
                    return Kind::Constructor;
                }
                let expression = function > 0
                    && matches!(
                        tokens[function - 1].text.as_str(),
                        "=" | "(" | "," | ":" | "return" | "["
                    );
                return Kind::Function {
                    inline: !multiline,
                    expression,
                };
            }
            return Kind::Function {
                inline: !multiline,
                expression: true,
            };
        }
    }
    if prev.is_some_and(|p| p.is("=>")) {
        return Kind::Function {
            inline: !multiline,
            expression: true,
        };
    }
    if prev.is_some_and(|p| {
        matches!(
            p.text.as_str(),
            "=" | "(" | "," | "[" | ":" | "return" | "?"
        )
    }) {
        return Kind::Object { inline: !multiline };
    }
    Kind::Control
}
