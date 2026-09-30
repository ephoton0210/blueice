// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

//! Enum emit.
//!
//! An enum becomes the object TypeScript builds: a variable, then a function
//! that fills it in, called with the object itself or a new one so that
//! declarations of one name merge:
//!
//! ```js
//! var Color;
//! (function (Color) {
//!     Color[Color["Red"] = 0] = "Red";
//!     Color["Label"] = "red";
//! })(Color || (Color = {}));
//! ```
//!
//! A numeric member also maps its value back to its name. A constant member is
//! written as its evaluated value; a computed one keeps its initializer, with
//! the recorded type erasure applied. An ambient (`declare`) enum has no
//! runtime form and is removed. The generated text keeps as many line breaks
//! as the declaration had, so no later source line moves.

use std::collections::{BTreeMap, BTreeSet};

use super::class_lowering::render_tokens;
use super::{Module, TextEdit};
use crate::checker::ExportedEnum;
use crate::compiler::{CompilerOptions, Project};
use crate::diagnostic::Diagnostic;
use crate::enum_eval::{evaluate_enums, js_number_text, EnumValue, EvaluatedMember};
use crate::parser::{Declaration, EnumDeclaration};
use crate::{Token, TokenKind};

/// A `const enum` whose uses are replaced by values.
struct ConstEnum {
    /// The name the comment after an inlined value uses.
    display: String,
    members: Vec<EvaluatedMember>,
}

pub(super) fn lower_enums(
    module: &Module,
    project: &Project,
    exported_enums: &BTreeMap<String, BTreeMap<String, ExportedEnum>>,
    options: &CompilerOptions,
    edits: &mut Vec<TextEdit>,
) -> Result<(), Diagnostic> {
    let inline = options.inlines_const_enums();
    let evaluated = evaluate_enums(module);
    let mut evaluations = evaluated.iter();
    let mut declared_names: BTreeSet<&str> = BTreeSet::new();
    // Enums named in an `export { .. }` are exported too.
    let exported_by_name: BTreeSet<&str> = module
        .declarations
        .iter()
        .filter_map(|declaration| match declaration {
            Declaration::ValueExport(export) => Some(export.bindings.iter()),
            _ => None,
        })
        .flatten()
        .map(|binding| binding.local.as_str())
        .collect();
    let mut consts: BTreeMap<String, ConstEnum> = BTreeMap::new();
    if inline {
        // Uses are replaced before the declarations themselves are rewritten,
        // so a declaration that is removed takes its uses' edits with it.
        for declaration in &module.declarations {
            let Declaration::Enum(enum_declaration) = declaration else {
                continue;
            };
            if !enum_declaration.is_const {
                continue;
            }
            let evaluation = evaluate_enums(module);
            let members: Vec<EvaluatedMember> = evaluation
                .iter()
                .zip(
                    module
                        .declarations
                        .iter()
                        .filter_map(|declaration| match declaration {
                            Declaration::Enum(candidate) => Some(candidate),
                            _ => None,
                        }),
                )
                .filter(|(_, candidate)| candidate.name == enum_declaration.name)
                .flat_map(|(evaluated, _)| evaluated.members.iter().cloned())
                .collect();
            consts.insert(
                enum_declaration.name.clone(),
                ConstEnum {
                    display: enum_declaration.name.clone(),
                    members,
                },
            );
        }
        for declaration in &module.declarations {
            let Declaration::Import(import) = declaration else {
                continue;
            };
            if import.type_only {
                continue;
            }
            let Some(resolved) = project
                .resolutions
                .get(&(module.id.clone(), import.specifier.clone()))
            else {
                continue;
            };
            for binding in &import.bindings {
                let Some(imported) = exported_enums
                    .get(resolved)
                    .and_then(|enums| enums.get(&binding.imported))
                    .filter(|imported| imported.is_const && !imported.declared)
                else {
                    continue;
                };
                consts.insert(
                    binding.local.clone(),
                    ConstEnum {
                        display: binding.imported.clone(),
                        members: imported.members.clone(),
                    },
                );
            }
        }
        if !consts.is_empty() {
            inline_const_enum_uses(module, &consts, edits);
        }
    }
    for declaration in &module.declarations {
        let Declaration::Enum(declaration) = declaration else {
            continue;
        };
        let evaluation = evaluations.next().expect("every enum was evaluated");
        let erase = declaration.declared
            || (declaration.is_const
                && inline
                && !options.preserve_const_enums
                && !declaration.exported
                && !exported_by_name.contains(declaration.name.as_str()));
        let text = if erase {
            String::new()
        } else {
            let first = declared_names.insert(declaration.name.as_str());
            enum_statement(module, declaration, evaluation, first, edits)
        };
        let lines = module.source[declaration.span.start..declaration.span.end]
            .matches('\n')
            .count();
        edits.retain(|edit| {
            !(edit.start >= declaration.span.start && edit.end <= declaration.span.end)
        });
        edits.push(TextEdit {
            start: declaration.span.start,
            end: declaration.span.end,
            replacement: spread_lines(text, lines),
        });
    }
    Ok(())
}

/// Replaces `E.A` and `E["A"]` for each const enum `E` with the member's value
/// and a comment naming it. A negative or non-finite value is parenthesized, so
/// `E.N ** 2` and `-E.N` stay valid JavaScript.
fn inline_const_enum_uses(
    module: &Module,
    consts: &BTreeMap<String, ConstEnum>,
    edits: &mut Vec<TextEdit>,
) {
    let Ok(tokens) = crate::lex(&module.id, &module.source) else {
        return;
    };
    scan_uses(module, &tokens, 0, consts, edits);
}

fn scan_uses(
    module: &Module,
    tokens: &[Token],
    base: usize,
    consts: &BTreeMap<String, ConstEnum>,
    edits: &mut Vec<TextEdit>,
) {
    for (index, token) in tokens.iter().enumerate() {
        if token.kind == TokenKind::Template && token.text.contains("${") {
            for (start, end) in template_substitutions(&token.text) {
                let offset = base + token.start + start;
                if let Ok(inner) = crate::lex(&module.id, &token.text[start..end]) {
                    scan_uses(module, &inner, offset, consts, edits);
                }
            }
            continue;
        }
        let Some(enum_) = consts
            .get(&token.text)
            .filter(|_| token.kind == TokenKind::Identifier)
        else {
            continue;
        };
        if index
            .checked_sub(1)
            .and_then(|before| tokens.get(before))
            .is_some_and(|before| before.is("."))
        {
            continue;
        }
        let (member, end, bracket) = match (tokens.get(index + 1), tokens.get(index + 2)) {
            (Some(dot), Some(name))
                if dot.is(".")
                    && matches!(name.kind, TokenKind::Identifier | TokenKind::Keyword) =>
            {
                (name.text.clone(), name.end, false)
            }
            (Some(open), Some(key))
                if open.is("[")
                    && key.kind == TokenKind::String
                    && tokens.get(index + 3).is_some_and(|close| close.is("]")) =>
            {
                let Some(name) = crate::enum_eval::decode_plain_string(&key.text) else {
                    continue;
                };
                (name, tokens[index + 3].end, true)
            }
            _ => continue,
        };
        let Some(EvaluatedMember {
            value: Some(value), ..
        }) = enum_
            .members
            .iter()
            .find(|candidate| candidate.name == member)
        else {
            continue;
        };
        let label = if bracket || !is_identifier_name(&member) {
            format!("{}[{}]", enum_.display, json_string(&member))
        } else {
            format!("{}.{member}", enum_.display)
        };
        let (text, wrap) = match value {
            EnumValue::Number(number) => (
                js_number_text(*number),
                *number < 0.0 || number.is_nan() || number.is_infinite(),
            ),
            EnumValue::Text(text) => (json_string(text), false),
        };
        let replacement = if wrap {
            format!("({text} /* {label} */)")
        } else {
            format!("{text} /* {label} */")
        };
        edits.push(TextEdit {
            start: base + token.start,
            end: base + end,
            replacement,
        });
    }
}

/// The byte ranges of the `${ .. }` substitutions in a template literal's text.
fn template_substitutions(text: &str) -> Vec<(usize, usize)> {
    let bytes = text.as_bytes();
    let mut ranges = Vec::new();
    let mut index = 0;
    while index + 1 < bytes.len() {
        if bytes[index] == b'$' && bytes[index + 1] == b'{' {
            let start = index + 2;
            let mut depth = 1usize;
            let mut cursor = start;
            while cursor < bytes.len() && depth > 0 {
                match bytes[cursor] {
                    b'{' => depth += 1,
                    b'}' => depth -= 1,
                    _ => {}
                }
                cursor += 1;
            }
            if depth == 0 {
                ranges.push((start, cursor - 1));
                index = cursor;
                continue;
            }
            break;
        }
        index += 1;
    }
    ranges
}

fn is_identifier_name(name: &str) -> bool {
    let mut characters = name.chars();
    characters
        .next()
        .is_some_and(|first| first.is_alphabetic() || matches!(first, '_' | '$'))
        && characters.all(|character| character.is_alphanumeric() || matches!(character, '_' | '$'))
}

/// The statements for one declaration. `first` is false for the second and later
/// declarations of a merged enum, which reuse the variable.
fn enum_statement(
    module: &Module,
    declaration: &EnumDeclaration,
    evaluation: &crate::enum_eval::EvaluatedEnum,
    first: bool,
    edits: &[TextEdit],
) -> String {
    let name = &declaration.name;
    let mut parts: Vec<String> = Vec::new();
    if first {
        parts.push(format!(
            "{}var {name};",
            if declaration.exported { "export " } else { "" }
        ));
    }
    parts.push(format!("(function ({name}) {{"));
    for (member, evaluated) in declaration.members.iter().zip(&evaluation.members) {
        let key = json_string(&member.name);
        parts.push(match &evaluated.value {
            Some(EnumValue::Number(number)) => format!(
                "    {name}[{name}[{key}] = {}] = {key};",
                js_number_text(*number)
            ),
            Some(EnumValue::Text(text)) => format!("    {name}[{key}] = {};", json_string(text)),
            None => {
                let value = member
                    .initializer
                    .as_deref()
                    .map(|tokens| render_tokens(&module.source, edits, tokens))
                    .unwrap_or_else(|| "void 0".to_string());
                format!("    {name}[{name}[{key}] = {value}] = {key};")
            }
        });
    }
    parts.push(format!("}})({name} || ({name} = {{}}));"));
    parts.join("\n")
}

/// `text` with its line breaks replaced by spaces, then as many line breaks
/// as the original declaration had put back at the end of its parts.
fn spread_lines(text: String, lines: usize) -> String {
    if text.is_empty() {
        return text;
    }
    let parts: Vec<&str> = text.split('\n').collect();
    let mut output = String::new();
    let mut remaining = lines;
    let mut joined_by_space = false;
    for (index, part) in parts.iter().enumerate() {
        // Indentation only makes sense after a line break.
        output.push_str(if joined_by_space {
            part.trim_start()
        } else {
            part
        });
        if index + 1 < parts.len() {
            if remaining > 0 {
                output.push('\n');
                remaining -= 1;
                joined_by_space = false;
            } else {
                output.push(' ');
                joined_by_space = true;
            }
        }
    }
    for _ in 0..remaining {
        output.push('\n');
    }
    output
}

/// A string as a double-quoted JavaScript literal.
fn json_string(text: &str) -> String {
    let mut output = String::from("\"");
    for character in text.chars() {
        match character {
            '"' => output.push_str("\\\""),
            '\\' => output.push_str("\\\\"),
            '\n' => output.push_str("\\n"),
            '\r' => output.push_str("\\r"),
            '\t' => output.push_str("\\t"),
            control if (control as u32) < 0x20 => {
                output.push_str(&format!("\\u{:04x}", control as u32));
            }
            other => output.push(other),
        }
    }
    output.push('"');
    output
}

/// The declaration file text of one enum: a constant member with its value and a
/// computed one by name alone, as TypeScript prints them.
pub(super) fn emit_enum_declaration(
    declaration: &EnumDeclaration,
    evaluation: &crate::enum_eval::EvaluatedEnum,
) -> String {
    let mut output = format!(
        "{}declare {}enum {} {{\n",
        if declaration.exported { "export " } else { "" },
        if declaration.is_const { "const " } else { "" },
        declaration.name
    );
    let members: Vec<String> = declaration
        .members
        .iter()
        .zip(&evaluation.members)
        .map(|(member, evaluated)| {
            let name = if is_identifier_name(&member.name) {
                member.name.clone()
            } else {
                json_string(&member.name)
            };
            match &evaluated.value {
                Some(EnumValue::Number(number)) => {
                    format!("    {name} = {}", js_number_text(*number))
                }
                Some(EnumValue::Text(text)) => format!("    {name} = {}", json_string(text)),
                None => format!("    {name}"),
            }
        })
        .collect();
    output.push_str(&members.join(",\n"));
    if !members.is_empty() {
        output.push('\n');
    }
    output.push_str("}\n");
    output
}
