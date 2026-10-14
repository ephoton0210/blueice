// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

//! Original System factories, dependency setters and callback-backed bindings.

use super::*;
use crate::syntax::Token;
use std::collections::BTreeSet;

mod mapping;
mod mutations;

pub(crate) const HELPER_SOURCE: &str = include_str!("system_helpers.v1.js");

pub(super) fn validate(module: &Module) -> Result<(), Diagnostic> {
    for declaration in &module.declarations {
        if let Declaration::ValueExport(export) = declaration {
            if export.export_assignment {
                return Err(Diagnostic::error(
                    DiagnosticCode::UnsupportedSyntax,
                    export.span.clone(),
                    "System does not support export assignment",
                )
                .with_typescript(1218, Vec::new()));
            }
        }
    }
    Ok(())
}

pub(super) fn wrap(
    emitted: EmittedJavaScript,
    module: &Module,
    options: &CompilerOptions,
) -> Result<EmittedJavaScript, Diagnostic> {
    if !module.is_external_module() {
        return Ok(emitted);
    }
    let mut sequence = 0;
    let mut fresh = |kind: &str| loop {
        let name = format!("__bluets_system_{kind}_{sequence}");
        sequence += 1;
        if !emitted.javascript.contains(&name) {
            break name;
        }
    };
    let stem = fresh("binding");
    let helper = HELPER_SOURCE.replace("__SYS", &stem);
    let export = format!("{stem}_export");
    let imports = format!("{stem}_imports");
    let define = format!("{stem}_define");
    let refresh = format!("{stem}_refresh");
    let emitted = mutations::lower(emitted, module, options, &format!("{stem}_changed"))?;
    let tokens = crate::syntax::lex_with_limits(
        &module.id,
        &emitted.javascript,
        options.limits.parser.max_source_bytes,
        options.limits.parser.max_tokens,
    )
    .map_err(|mut diagnostics| diagnostics.remove(0))?;
    let mut dependencies = Vec::new();
    for declaration in &module.declarations {
        let specifier = match declaration {
            Declaration::Import(import) if !import.is_type_only() => Some(&import.specifier),
            Declaration::ValueExport(value) if !value.is_type_only() => value.specifier.as_ref(),
            _ => None,
        };
        if let Some(specifier) = specifier {
            let specifier =
                javascript_specifier(specifier, options.jsx == Some(crate::JsxMode::Preserve));
            if !dependencies.contains(&specifier) {
                dependencies.push(specifier);
            }
        }
    }
    if options.import_helpers && emitted.javascript.contains("require('tslib')") {
        dependencies.push("tslib".to_string());
    }
    let mut names = BTreeSet::new();
    let mut hoisted = Vec::new();
    for declaration in &module.declarations {
        match declaration {
            Declaration::Variable(value) if value.exported && !value.declared => {
                names.insert(value.name.clone());
            }
            Declaration::Function(value)
                if value.exported && !value.declared && !value.overload =>
            {
                let name = if value.default_export {
                    "default"
                } else {
                    &value.name
                };
                names.insert(name.to_string());
                hoisted.push(format!("exports[{name:?}] = {};", value.name));
            }
            Declaration::Class(value) if value.exported => {
                names.insert(value.export_name().to_string());
            }
            Declaration::Enum(value) if value.exported && !value.declared => {
                names.insert(value.name.clone());
            }
            Declaration::Namespace(value) if value.exported => {
                names.insert(value.name.clone());
            }
            Declaration::DefaultExport(_) => {
                names.insert("default".to_string());
            }
            Declaration::ValueExport(value) if !value.is_type_only() => {
                names.extend(
                    value
                        .bindings
                        .iter()
                        .filter(|binding| !binding.type_only)
                        .map(|binding| binding.exported.clone()),
                );
                if let Some(name) = &value.namespace {
                    names.insert(name.clone());
                }
            }
            _ => {}
        }
    }
    let mut variables = BTreeSet::new();
    let mut import_bindings = std::collections::BTreeMap::<String, Vec<String>>::new();
    let mut functions = Vec::new();
    let mut edits = Vec::new();
    let mut depth = 0usize;
    let mut index = 0;
    while index < tokens.len() {
        let token = &tokens[index];
        if depth == 0
            && token.is("function")
            && tokens.get(index + 1).is_some_and(|token| {
                token.kind == crate::syntax::TokenKind::Identifier || token.is("*")
            })
            && index
                .checked_sub(1)
                .is_none_or(|previous| !matches!(tokens[previous].text.as_str(), "=" | ":" | ","))
        {
            let open = (index + 1..tokens.len())
                .find(|cursor| tokens[*cursor].is("{"))
                .ok_or_else(|| unsupported(module, "System function body is missing"))?;
            let end = closing(&tokens, open)
                .ok_or_else(|| unsupported(module, "System function body is unbalanced"))?;
            let start = if index > 0 && tokens[index - 1].is("async") {
                tokens[index - 1].start
            } else {
                token.start
            };
            functions.push((start, tokens[end].end));
            index = end + 1;
            continue;
        }
        if depth == 0 && matches!(token.text.as_str(), "const" | "let" | "var") {
            if let Some(name) = tokens.get(index + 1) {
                if name.kind != crate::syntax::TokenKind::Identifier {
                    return Err(unsupported(
                        module,
                        "System variable binding requires retained names",
                    ));
                }
                variables.insert(name.text.clone());
                if !module.declarations.iter().any(|declaration| {
                    matches!(declaration,
                    Declaration::Variable(value) if value.name == name.text)
                }) && tokens.get(index + 2).is_some_and(|token| token.is("="))
                    && tokens
                        .get(index + 3)
                        .is_some_and(|token| token.is("require"))
                    && tokens.get(index + 4).is_some_and(|token| token.is("("))
                    && tokens.get(index + 6).is_some_and(|token| token.is(")"))
                    && tokens.get(index + 7).is_some_and(|token| token.is(";"))
                {
                    if let Some(dependency) = tokens
                        .get(index + 5)
                        .and_then(crate::syntax::string_contents)
                        .filter(|dependency| dependencies.contains(dependency))
                    {
                        import_bindings
                            .entry(dependency)
                            .or_default()
                            .push(name.text.clone());
                        let end = tokens
                            .get(index + 7)
                            .filter(|token| token.is(";"))
                            .map_or(tokens[index + 6].end, |token| token.end);
                        edits.push(TextEdit {
                            start: token.start,
                            end,
                            replacement: String::new(),
                        });
                        index += 7;
                        continue;
                    }
                }
                edits.push(TextEdit {
                    start: token.start,
                    end: token.end,
                    replacement: String::new(),
                });
            }
        }
        if depth == 0 && token.is("class") {
            if let Some(name) = tokens.get(index + 1) {
                variables.insert(name.text.clone());
                edits.push(TextEdit {
                    start: token.start,
                    end: token.start,
                    replacement: format!("{} = ", name.text),
                });
            }
        }
        if token.is("Object")
            && tokens.get(index + 1).is_some_and(|token| token.is("."))
            && tokens
                .get(index + 2)
                .is_some_and(|token| token.is("defineProperty"))
            && tokens
                .get(index + 4)
                .is_some_and(|token| token.is("exports"))
        {
            edits.push(TextEdit {
                start: token.start,
                end: tokens[index + 2].end,
                replacement: define.clone(),
            });
        }
        match token.text.as_str() {
            "(" | "[" | "{" => depth += 1,
            ")" | "]" | "}" => depth = depth.saturating_sub(1),
            _ => {}
        }
        index += 1;
    }
    // Apply edits before moving function definitions so all original mappings
    // pass through the same mapped transform as ordinary target lowering.
    edits.sort_by_key(|edit| edit.start);
    let shifted = |offset: usize| {
        edits
            .iter()
            .filter(|edit| edit.end <= offset)
            .fold(offset, |position, edit| {
                position + edit.replacement.len() - (edit.end - edit.start)
            })
    };
    let functions = functions
        .into_iter()
        .map(|(start, end)| (shifted(start), shifted(end)))
        .collect::<Vec<_>>();
    let body = targets::mapped_edits(emitted, edits);
    let slots = names
        .iter()
        .map(|name| format!("{stem}_slot({name:?});"))
        .collect::<Vec<_>>()
        .join("\n");
    let variables = if variables.is_empty() {
        String::new()
    } else {
        format!(
            "var {};\n",
            variables.into_iter().collect::<Vec<_>>().join(", ")
        )
    };
    let setters = dependencies
        .iter()
        .map(|dependency| {
            let bindings = import_bindings
                .get(dependency)
                .into_iter()
                .flatten()
                .map(|name| format!("{name} = value;"))
                .collect::<Vec<_>>()
                .join(" ");
            format!(
                "function(value) {{ {imports}[{dependency:?}] = value; {bindings} {refresh}(); }}"
            )
        })
        .collect::<Vec<_>>()
        .join(",\n");
    let prefix = format!("System.register({}, function({export}, {stem}_context) {{\n'use strict';\nvar exports = {{}}, {imports} = Object.create(null);\nfunction require(name) {{ return {imports}[name]; }}\n{helper}\n{variables}", serde_json::to_string(&dependencies).unwrap());
    let execute = format!(
        "{slots}\n{}\nreturn {{setters: [{setters}], execute: function() {{\n",
        hoisted.join("\n")
    );
    let suffix = format!("\n{refresh}();\n}}}};\n}});");
    Ok(mapping::relocate(
        body, &functions, &prefix, &execute, &suffix,
    ))
}

fn closing(tokens: &[Token], open: usize) -> Option<usize> {
    let mut depth = 0;
    for (index, token) in tokens.iter().enumerate().skip(open) {
        if token.is("{") {
            depth += 1;
        }
        if token.is("}") {
            depth -= 1;
            if depth == 0 {
                return Some(index);
            }
        }
    }
    None
}

fn unsupported(module: &Module, message: &str) -> Diagnostic {
    Diagnostic::error(
        DiagnosticCode::UnsupportedSyntax,
        SourceSpan::new(&module.id, 0, 0),
        message,
    )
}
