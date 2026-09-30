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

use std::collections::BTreeSet;

use super::class_lowering::render_tokens;
use super::{Module, TextEdit};
use crate::diagnostic::Diagnostic;
use crate::enum_eval::{evaluate_enums, js_number_text, EnumValue};
use crate::parser::{Declaration, EnumDeclaration};

pub(super) fn lower_enums(module: &Module, edits: &mut Vec<TextEdit>) -> Result<(), Diagnostic> {
    let evaluated = evaluate_enums(module);
    let mut evaluations = evaluated.iter();
    let mut declared_names: BTreeSet<&str> = BTreeSet::new();
    for declaration in &module.declarations {
        let Declaration::Enum(declaration) = declaration else {
            continue;
        };
        let evaluation = evaluations.next().expect("every enum was evaluated");
        let text = if declaration.declared {
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
