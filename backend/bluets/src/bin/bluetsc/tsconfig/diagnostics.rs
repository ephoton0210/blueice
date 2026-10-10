// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

//! Config diagnostics address bytes already read under Reader's authority.
use super::*;
use blueice_bluets::{Diagnostic, DiagnosticCode, TypeScriptDiagnostic};

pub(crate) fn error(message: &str, path: &Path, bytes: Option<&[u8]>) -> Option<Diagnostic> {
    let source = bytes.and_then(|bytes| std::str::from_utf8(bytes).ok());
    let name = message.split('`').nth(1);
    let code = if message == "no inputs were found in the tsconfig file" {
        18003
    } else if message == "config `files` must not be an empty array" {
        18002
    } else if message.starts_with("removed compiler option") {
        5102
    } else if message == "compiler option `mapRoot` requires sourceMap or declarationMap" {
        5069
    } else if matches!(
        message,
        "compiler option `inlineSources` requires sourceMap"
            | "compiler option `sourceRoot` requires sourceMap"
    ) {
        5051
    } else if message.starts_with("unsupported compiler option `newLine` value") {
        6046
    } else if message.starts_with("unknown or unsupported compiler option") {
        if TypeScriptDiagnostic::is_known_compiler_option(name?) {
            return None;
        }
        5023
    } else if message.starts_with("compiler option") && message.contains("requires a ") {
        5024
    } else if message.starts_with("cannot read config ")
        || message.starts_with("cannot find extended config ")
    {
        5083
    } else {
        return None;
    };
    let key = if code == 18002 {
        "files"
    } else {
        name.unwrap_or("")
    };
    let range = source
        .and_then(|source| property_range(source, key, !matches!(code, 5023 | 5051 | 5069 | 5102)));
    let module = path.file_name()?.to_string_lossy().into_owned();
    let span = range.map_or_else(
        || SourceSpan::new(&module, 0, 0),
        |(start, end)| SourceSpan::new(&module, start, end),
    );
    let arguments = match code {
        18002 => vec![path.display().to_string()],
        18003 => vec![path.display().to_string(), "[]".into(), "[]".into()],
        5023 | 5051 | 5102 => vec![key.into()],
        5069 => vec![key.into(), "sourceMap".into(), "declarationMap".into()],
        6046 => vec!["--newLine".into(), "'crlf', 'lf'".into()],
        5024 => vec![
            key.into(),
            if message.ends_with("boolean") {
                "boolean"
            } else {
                "string"
            }
            .into(),
        ],
        5083 => vec![if message.starts_with("cannot find extended config ") {
            clean_path(&path.parent()?.join(name?))
                .display()
                .to_string()
        } else {
            path.display().to_string()
        }],
        _ => Vec::new(),
    };
    let mut diagnostic = Diagnostic::error(DiagnosticCode::ParseError, span, message)
        .with_typescript(code, arguments);
    if code == 5102 {
        if let Some(counterpart) = &mut diagnostic.typescript {
            counterpart
                .message
                .push_str("\n  Use 'verbatimModuleSyntax' instead.");
        }
    }
    if let (Some(source), Some(_)) = (source, range) {
        diagnostic = diagnostic.with_source_position(source);
    }
    Some(diagnostic)
}

fn property_range(source: &str, key: &str, value: bool) -> Option<(usize, usize)> {
    let tokens = blueice_bluets::lex("tsconfig.json", source).ok()?;
    let index = tokens.windows(2).position(|pair| {
        serde_json::from_str::<String>(&pair[0].text)
            .ok()
            .as_deref()
            == Some(key)
            && pair[1].is(":")
    })?;
    let token = tokens.get(index + if value { 2 } else { 0 })?;
    let end = if token.is("[") && tokens.get(index + 3).is_some_and(|token| token.is("]")) {
        tokens[index + 3].end
    } else {
        token.end
    };
    Some((token.start, end))
}

pub(crate) fn invalid_json(message: &str, path: &Path, source: &str) -> Option<Diagnostic> {
    // An invalid include array supplies no inputs to --showConfig. Other JSONC
    // parser refusals keep their explicit BlueTSC cause until measured.
    let tokens = blueice_bluets::lex("tsconfig.json", source).ok()?;
    let include = tokens.windows(3).position(|tokens| {
        tokens[0].text == "\"include\"" && tokens[1].is(":") && tokens[2].is("[")
    })?;
    if !tokens.get(include + 3).is_some_and(|token| token.is("}")) {
        return None;
    }
    let diagnostic = Diagnostic::error(
        DiagnosticCode::ParseError,
        SourceSpan::new(path.file_name()?.to_string_lossy(), 0, 0),
        message,
    )
    .with_typescript(
        18003,
        vec![path.display().to_string(), "[]".into(), "[]".into()],
    );
    Some(diagnostic)
}

pub(crate) fn missing_project(message: &str, path: &Path) -> Diagnostic {
    Diagnostic::error(
        DiagnosticCode::ModuleNotFound,
        SourceSpan::new(path.to_string_lossy(), 0, 0),
        message,
    )
    .with_typescript(5058, vec![path.display().to_string()])
}
