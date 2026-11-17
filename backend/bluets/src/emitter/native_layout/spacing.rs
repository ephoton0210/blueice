// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

//! Trivia rules preserve opaque literals and newline-sensitive expressions.

use super::*;
use roles::{Kind, Roles};

pub(super) fn between(tokens: &[Token], i: usize, gap: &str, roles: &Roles) -> String {
    let prev = &tokens[i - 1];
    let next = &tokens[i];
    let context = roles.before[i];
    let previous_member = member_name(tokens, i - 1);
    let closing = roles.closings[i];
    let previous = roles.closings[i - 1];
    let indent = context
        .indent
        .saturating_sub(usize::from(closing.is_some_and(Kind::multiline)));
    let line = || format!("\n{}", "    ".repeat(indent));
    if !previous_member
        && gap.contains(['\r', '\n'])
        && matches!(prev.text.as_str(), "return" | "break" | "continue")
    {
        return format!(";{}", line());
    }
    // A line break before an update expression prevents it from becoming
    // a postfix update of the preceding statement's final expression.
    if gap.contains(['\r', '\n']) && matches!(next.text.as_str(), "++" | "--") {
        return line();
    }
    if matches!(next.text.as_str(), ";" | "," | ")" | "]" | "." | "?.") {
        if next.is(".") && prev.kind == TokenKind::Number && !prev.text.contains(['.', 'e', 'E']) {
            return " ".into();
        }
        return String::new();
    }
    if let Some(kind) = closing {
        if kind.multiline() {
            return line();
        }
        if kind.object() && prev.is("{") {
            return String::new();
        }
        return " ".into();
    }
    if let Some(kind) = roles.openings[i - 1] {
        if kind.multiline() {
            return line();
        }
        return " ".into();
    }
    if prev.is(";") {
        if context.in_for
            || matches!(context.kind, Kind::Function { inline: true, .. })
            || context.kind == Kind::Switch && !gap.contains(['\r', '\n'])
        {
            return " ".into();
        }
        return line();
    }
    if let Some(kind) = previous {
        if kind.object() && next.is("from") {
            return " ".into();
        }
        if matches!(next.text.as_str(), "else" | "catch" | "finally") {
            return " ".into();
        }
        if (matches!(
            kind,
            Kind::Function {
                expression: false,
                ..
            }
        ) && matches!(context.kind, Kind::Root | Kind::Namespace))
            || context.kind == Kind::Class
            || (kind.multiline() && !matches!(next.text.as_str(), "(" | "=" | ":"))
        {
            return line();
        }
    }
    if roles.openings[i].is_some() {
        return if matches!(prev.text.as_str(), "(" | "[") {
            String::new()
        } else {
            " ".into()
        };
    }
    if prev.is(",") {
        return " ".into();
    }
    if next.is(":") {
        return String::new();
    }
    if prev.is(":") {
        return " ".into();
    }
    if prev.is(".") || prev.is("?.") || prev.is("...") {
        return String::new();
    }
    // A sign in operand position is a prefix operator. Keep the operand
    // adjacent while preventing two signs from becoming an update token.
    if prefix_sign(tokens, i - 1) {
        return if next.text.starts_with(&prev.text) {
            " ".into()
        } else {
            String::new()
        };
    }
    if next.is("(") {
        return if !previous_member
            && (binary(prev)
                || prev.is("async") && arrow_parameters(tokens, i)
                || matches!(
                    prev.text.as_str(),
                    "if" | "for"
                        | "while"
                        | "switch"
                        | "with"
                        | "catch"
                        | "function"
                        | "return"
                        | "throw"
                        | "new"
                        | "typeof"
                        | "void"
                        | "delete"
                        | "yield"
                        | "await"
                ))
        {
            " ".into()
        } else {
            String::new()
        };
    }
    if next.is("[") {
        return if !previous_member && (binary(prev) || prev.is("return")) {
            " ".into()
        } else {
            String::new()
        };
    }
    if matches!(prev.text.as_str(), "(" | "[") {
        return String::new();
    }
    if (prev.text.ends_with('+') && next.text.starts_with('+'))
        || (prev.text.ends_with('-') && next.text.starts_with('-'))
        || (prev.is("/") && (next.text.starts_with('/') || next.text.starts_with('*')))
    {
        return " ".into();
    }
    if matches!(next.text.as_str(), "++" | "--")
        || matches!(prev.text.as_str(), "++" | "--" | "!" | "~")
    {
        return String::new();
    }
    // A private name following a class generator star is its method key.
    // Multiplication cannot use a bare private name as its right operand.
    if prev.is("*") && context.kind == Kind::Class && next.text.starts_with('#') {
        return String::new();
    }
    if binary(prev) || binary(next) {
        return " ".into();
    }
    // Retain remaining statement boundaries, including `call()\ncall()`.
    // Within parentheses the same line break remains legal trivia.
    if gap.contains(['\r', '\n']) {
        return line();
    }
    if word(prev) && (word(next) || next.is("#")) {
        " ".into()
    } else {
        String::new()
    }
}

fn arrow_parameters(tokens: &[Token], open: usize) -> bool {
    let mut depth = 0usize;
    for (index, token) in tokens.iter().enumerate().skip(open) {
        if token.is("(") {
            depth += 1;
        } else if token.is(")") {
            depth -= 1;
            if depth == 0 {
                return tokens.get(index + 1).is_some_and(|token| token.is("=>"));
            }
        }
    }
    false
}

fn word(token: &Token) -> bool {
    token
        .text
        .chars()
        .next()
        .is_some_and(|c| c.is_alphanumeric() || matches!(c, '_' | '$' | '\'' | '"' | '`'))
}
pub(super) fn member_name(tokens: &[Token], index: usize) -> bool {
    index > 0 && matches!(tokens[index - 1].text.as_str(), "." | "?.")
}

pub(super) fn binary(token: &Token) -> bool {
    matches!(
        token.text.as_str(),
        "=" | "+"
            | "-"
            | "*"
            | "/"
            | "%"
            | "**"
            | "=="
            | "==="
            | "!="
            | "!=="
            | "<"
            | ">"
            | "<="
            | ">="
            | "&&"
            | "||"
            | "??"
            | "&"
            | "|"
            | "^"
            | "<<"
            | ">>"
            | ">>>"
            | "+="
            | "-="
            | "*="
            | "/="
            | "&&="
            | "||="
            | "??="
            | "=>"
            | "?"
    )
}

fn prefix_sign(tokens: &[Token], at: usize) -> bool {
    matches!(tokens[at].text.as_str(), "+" | "-")
        && (at == 0
            || binary(&tokens[at - 1])
            || matches!(
                tokens[at - 1].text.as_str(),
                "(" | "["
                    | "{"
                    | ","
                    | ":"
                    | ";"
                    | "!"
                    | "~"
                    | "return"
                    | "throw"
                    | "yield"
                    | "await"
                    | "typeof"
                    | "void"
                    | "delete"
                    | "case"
                    | "in"
                    | "instanceof"
            ))
}
