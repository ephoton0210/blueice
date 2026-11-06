// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

//! Source accessors share one descriptor and retain their distinct body ends.
use super::*;
use crate::parser::ClassMemberShell;

pub(super) struct Member {
    pub(super) end: usize,
    pub(super) text: String,
}

fn key(token: &Token) -> Option<String> {
    match token.kind {
        TokenKind::Identifier | TokenKind::Keyword => Some(token.text.clone()),
        TokenKind::String if !token.text.contains('\\') => crate::syntax::string_contents(token),
        // Equal spellings are safe even when the string contains escapes. A
        // computed expression is never included in this stable-key grammar.
        TokenKind::String => Some(format!("literal:{}", token.text)),
        TokenKind::Number => {
            let text = token.text.replace('_', "");
            let number = if text.starts_with("0x") || text.starts_with("0X") {
                u64::from_str_radix(&text[2..], 16).ok()? as f64
            } else if text.starts_with("0b") || text.starts_with("0B") {
                u64::from_str_radix(&text[2..], 2).ok()? as f64
            } else if text.starts_with("0o") || text.starts_with("0O") {
                u64::from_str_radix(&text[2..], 8).ok()? as f64
            } else {
                text.parse::<f64>().ok()?
            };
            number.is_finite().then(|| number.to_string())
        }
        _ => None,
    }
}

pub(super) fn read(
    emitted: &EmittedJavaScript,
    tokens: &[Token],
    range: (usize, usize),
    module: &Module,
    options: &CompilerOptions,
    base: &str,
) -> Result<BTreeMap<usize, Member>, Diagnostic> {
    let (start, close) = range;
    let offset = tokens[start].start;
    let Ok(owned) = crate::parser::parse_emitted_module(
        &module.id,
        &emitted.javascript[offset..tokens[close].end],
        &options.limits.parser,
    ) else {
        return Ok(BTreeMap::new());
    };
    let Some(Declaration::Class(class)) = owned.declarations.first() else {
        return Ok(BTreeMap::new());
    };
    let extends = class.extends_name.is_some();
    let mut groups = BTreeMap::<(bool, String), Vec<&ClassMemberShell>>::new();
    for member in &class.members {
        let Some(accessor) = &member.accessor else {
            continue;
        };
        if let [name] = member.key.as_slice() {
            if let Some(name) = key(name) {
                groups
                    .entry((accessor.is_static, name))
                    .or_default()
                    .push(member);
            }
        }
    }
    let mut result = BTreeMap::new();
    for ((is_static, _), members) in groups {
        if members.len() > 2
            || members.len() == 2
                && members[0].accessor.as_ref().unwrap().getter
                    == members[1].accessor.as_ref().unwrap().getter
        {
            continue;
        }
        let first = members[0];
        let owner = if is_static {
            class.name.clone()
        } else {
            format!("{}.prototype", class.name)
        };
        let name = &first.key[0];
        let quoted = if matches!(name.kind, TokenKind::String | TokenKind::Number) {
            name.text.clone()
        } else {
            serde_json::to_string(&name.text).unwrap()
        };
        let mut entries = String::new();
        for getter in [true, false] {
            let Some(member) = members
                .iter()
                .find(|member| member.accessor.as_ref().unwrap().getter == getter)
            else {
                continue;
            };
            let cursor = tokens.partition_point(|token| token.start < offset + member.span.start);
            let property =
                tokens.partition_point(|token| token.start < offset + member.key[0].start);
            let parameters = property + 1;
            let parameters_end = exponentiation::matching_close(tokens, parameters).unwrap();
            let body = parameters_end + 1;
            let end = exponentiation::matching_close(tokens, body).unwrap();
            let parameter_origin = if parameters_end == parameters + 1 {
                tokens[cursor].start
            } else {
                tokens[parameters_end].start
            };
            let arguments = relocations::relocated_verbatim(
                tokens[parameters].end,
                tokens[parameters_end].start,
            );
            let parameter_end = relocations::source_anchor(parameter_origin);
            let text = if extends {
                super_members::body(
                    &emitted.javascript,
                    tokens,
                    (body, end, parameter_origin),
                    base,
                    is_static,
                    module,
                )?
            } else {
                let first_body = tokens[body + 1].start;
                format!(
                    "{parameter_end}{{{}{}",
                    &emitted.javascript[tokens[body].end..first_body],
                    relocations::relocated_verbatim(first_body, tokens[end].end)
                )
            };
            entries.push_str(&format!(
                "{}{}: {}function ({arguments}{parameter_end}) {text}{}, ",
                relocations::unmapped(),
                if getter { "get" } else { "set" },
                relocations::source_anchor(tokens[cursor].start),
                relocations::source_anchor(tokens[end].end)
            ));
        }
        let generated = relocations::unmapped();
        let text = format!("{}Object.defineProperty({}{owner}, {quoted}{}, {{ {entries}{generated}enumerable: false, configurable: true }});{}\n{generated}", relocations::source_anchor(offset + first.span.start), relocations::source_anchor(offset + name.start), relocations::source_anchor(offset + name.end), relocations::source_anchor(offset + first.span.end));
        result.insert(
            offset + first.span.start,
            Member {
                end: offset + first.span.end,
                text,
            },
        );
        for member in members.iter().skip(1) {
            result.insert(
                offset + member.span.start,
                Member {
                    end: offset + member.span.end,
                    text: String::new(),
                },
            );
        }
    }
    Ok(result)
}
