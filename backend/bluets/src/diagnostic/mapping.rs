// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

//! Base rules use the diagnostic's cause, preserving its original BTS identity.

use super::{DiagnosticCode, SourceSpan, TypeScriptDiagnostic};

pub(super) fn map(
    code: DiagnosticCode,
    span: &SourceSpan,
    message: &str,
) -> Option<TypeScriptDiagnostic> {
    let alias = code.to_string();
    let (_, _, typescript_code) = super::rules::RULES
        .iter()
        .chain(super::source_rules::RULES)
        .find(|(bts, pattern, _)| *bts == alias && matches_pattern(pattern, message))?;
    let mut arguments = quoted_arguments(message);
    if matches!(*typescript_code, 2322 | 2345 | 2375) && message.starts_with("argument ") {
        arguments = [arguments.first(), arguments.last()]
            .into_iter()
            .flatten()
            .cloned()
            .collect();
    }
    build(*typescript_code, span, arguments, message)
}

pub(super) fn build(
    code: u32,
    span: &SourceSpan,
    arguments: Vec<String>,
    fallback: &str,
) -> Option<TypeScriptDiagnostic> {
    let template = super::templates::template(code)?;
    let rendered = render(template, &arguments).unwrap_or_else(|| fallback.to_string());
    Some(TypeScriptDiagnostic {
        code,
        message_template: template,
        arguments,
        message: rendered,
        span: span.clone(),
    })
}

fn matches_pattern(pattern: &str, message: &str) -> bool {
    let mut pattern = pattern;
    let mut message = message;
    loop {
        let next = [pattern.find("{}"), pattern.find("{n}")]
            .into_iter()
            .flatten()
            .min();
        let Some(next) = next else {
            return pattern == message;
        };
        let prefix = &pattern[..next];
        let Some(rest) = message.strip_prefix(prefix) else {
            return false;
        };
        let numeric = pattern[next..].starts_with("{n}");
        pattern = &pattern[next + if numeric { 3 } else { 2 }..];
        if pattern.is_empty() {
            return !rest.is_empty()
                && (!numeric || rest.bytes().all(|byte| byte.is_ascii_digit()));
        }
        let separator_end = pattern.find('{').unwrap_or(pattern.len());
        let separator = &pattern[..separator_end];
        if separator.is_empty() {
            return false;
        }
        let Some(end) = rest.find(separator) else {
            return false;
        };
        if numeric && !rest[..end].bytes().all(|byte| byte.is_ascii_digit()) {
            return false;
        }
        message = &rest[end..];
    }
}

fn quoted_arguments(message: &str) -> Vec<String> {
    message
        .split('`')
        .enumerate()
        .filter(|(index, _)| index % 2 == 1)
        .map(|(_, argument)| argument.to_string())
        .collect()
}

fn render(template: &str, arguments: &[String]) -> Option<String> {
    let mut result = String::new();
    let mut rest = template;
    while let Some(start) = rest.find('{') {
        result.push_str(&rest[..start]);
        let after = &rest[start + 1..];
        let end = after.find('}')?;
        let index: usize = after[..end].parse().ok()?;
        result.push_str(arguments.get(index)?);
        rest = &after[end + 1..];
    }
    result.push_str(rest);
    Some(result)
}

pub(super) fn no_counterpart(code: DiagnosticCode, message: &str) -> Option<&'static str> {
    let alias = code.to_string();
    if let Some((_, _, reason)) = super::blue_only_rules::RULES
        .iter()
        .find(|(bts, pattern, _)| *bts == alias && matches_pattern(pattern, message))
    {
        return Some(reason);
    }
    match code {
        DiagnosticCode::UnsupportedSyntax => {
            Some("BlueTSC deliberately refuses a documented subset boundary.")
        }
        DiagnosticCode::CircularModuleDependency => {
            Some("BlueTSC requires a closed acyclic checked dependency order.")
        }
        DiagnosticCode::ResourceLimit => {
            Some("An owner-selected BlueTSC resource budget was exhausted.")
        }
        DiagnosticCode::InvalidContract => {
            Some("A BlueTSC runtime contract or owner boundary is invalid.")
        }
        DiagnosticCode::InvalidDeclarationFile
            if message.starts_with("ambient host declaration") =>
        {
            Some("An owner-supplied declaration violates the BlueTSC closed-graph boundary.")
        }
        DiagnosticCode::ModuleNotFound if message.contains("module identities must be stable") => {
            Some("The loader returned a different source identity than the authorized request.")
        }
        DiagnosticCode::ModuleNotFound => {
            Some("The owner-controlled module loader or resolver refused an input request.")
        }
        _ => None,
    }
}
