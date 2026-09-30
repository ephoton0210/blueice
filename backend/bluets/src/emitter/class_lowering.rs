// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

//! Target- and option-dependent class emit.
//!
//! ES2022 with `useDefineForClassFields` emits class fields natively: the
//! source text is kept and only the parameter properties add text. Every other
//! combination lowers them the way TypeScript does, by moving text:
//!
//! * assign semantics (`useDefineForClassFields: false`) turns an instance
//!   field into `this.x = init;` at the start of the constructor (after
//!   `super(...)` in a derived class) and drops a field with no initializer;
//! * define semantics on ES2020 turns it into an `Object.defineProperty`
//!   statement with the same value, `undefined` when there is no initializer;
//! * a static field is `static { this.x = init; }` on ES2022 with assign
//!   semantics, and a statement after the class on ES2020, where a static
//!   block also becomes `(function () { .. }).call(Class);` so that its `this`
//!   is the class without rewriting any token.
//!
//! A class with no constructor gets one. Moved text is the original source
//! with the recorded type-erasing edits applied to it; everything is inserted
//! on the line it lands in, so no later source line moves.

use super::{apply_edits, Module, TextEdit};
use crate::compiler::{CompilerOptions, EcmaTarget};
use crate::diagnostic::{Diagnostic, DiagnosticCode, SourceSpan};
use crate::parser::{ClassDeclaration, ClassField, ClassMemberShell, Declaration};
use crate::Token;

/// The emitted form of a class's fields for one target and option set.
#[derive(Clone, Copy)]
struct FieldEmit {
    es2022: bool,
    define: bool,
}

impl FieldEmit {
    /// Fields stay class fields in the output.
    fn native(self) -> bool {
        self.es2022 && self.define
    }
}

/// Adds the edits every class needs for the selected target and options to
/// `edits`, removing recorded edits that fall in text that moves or vanishes.
pub(super) fn lower_class_members(
    module: &Module,
    options: &CompilerOptions,
    edits: &mut Vec<TextEdit>,
) -> Result<(), Diagnostic> {
    let emit = FieldEmit {
        es2022: options.target == EcmaTarget::Es2022,
        define: options.defines_class_fields(),
    };
    for declaration in &module.declarations {
        let Declaration::Class(class) = declaration else {
            continue;
        };
        if !emit.es2022 {
            refuse_es2022_syntax(class)?;
        }
        if emit.native() {
            native_parameter_properties(class, edits)?;
        } else {
            lower_class(module, class, emit, edits)?;
        }
    }
    Ok(())
}

/// Private names are ES2022 syntax the ES2020 lowering does not cover yet.
fn refuse_es2022_syntax(class: &ClassDeclaration) -> Result<(), Diagnostic> {
    let private = class.members.iter().find(|member| {
        member
            .name
            .as_deref()
            .is_some_and(|name| name.starts_with('#'))
    });
    if let Some(member) = private {
        return Err(Diagnostic::error(
            DiagnosticCode::UnsupportedSyntax,
            member.span.clone(),
            "private names need the ES2022 target; lowering them for ES2020 is not supported yet",
        ));
    }
    Ok(())
}

/// The native form: a field declaration for each parameter property at the
/// start of the class body, and `this.p = p;` after `super(...)` (or at the
/// start of the constructor body in a base class).
fn native_parameter_properties(
    class: &ClassDeclaration,
    edits: &mut Vec<TextEdit>,
) -> Result<(), Diagnostic> {
    let properties = class.parameter_property_fields();
    if properties.is_empty() {
        return Ok(());
    }
    let constructor = implementation(class).expect("parameter properties need a constructor");
    let Some(insertion) = constructor.prologue_insertion else {
        return Err(missing_super(constructor.span.clone()));
    };
    let declarations: String = properties
        .iter()
        .map(|field| format!(" {};", field.name))
        .collect();
    insert(edits, class.body_span.start + 1, declarations);
    let assignments: String = properties
        .iter()
        .map(|field| format!(" this.{0} = {0};", field.name))
        .collect();
    insert(edits, insertion.offset, assignments);
    Ok(())
}

fn lower_class(
    module: &Module,
    class: &ClassDeclaration,
    emit: FieldEmit,
    edits: &mut Vec<TextEdit>,
) -> Result<(), Diagnostic> {
    let source = &module.source;
    let class_name = &class.name;

    // Instance side: parameter properties first, then declared fields.
    let mut prologue = String::new();
    for property in class.parameter_property_fields() {
        prologue.push_str(&store_this(
            emit,
            &property.name,
            Some(property.name.clone()),
        ));
    }
    for member in &class.members {
        let Some(field) = member.field.as_ref().filter(|field| !field.is_static) else {
            continue;
        };
        if is_private_name(field) {
            continue;
        }
        let value = field
            .initializer
            .as_deref()
            .map(|tokens| render_tokens(source, edits, tokens));
        if value.is_some() || emit.define {
            prologue.push_str(&store_this(emit, &field.name, value));
        }
        erase_member(edits, member);
    }
    if !prologue.is_empty() {
        insert_prologue(class, &prologue, edits)?;
    }

    // Static side.
    let mut after_class = String::new();
    for member in &class.members {
        if let Some(field) = member.field.as_ref().filter(|field| field.is_static) {
            if is_private_name(field) {
                continue;
            }
            let value = field
                .initializer
                .as_deref()
                .map(|tokens| render_tokens(source, edits, tokens));
            if emit.es2022 {
                // ES2022 with assign semantics: a static block in place.
                replace_member(
                    edits,
                    member,
                    value
                        .map(|value| format!("static {{ this.{} = {value}; }}", field.name))
                        .unwrap_or_default(),
                );
            } else {
                if value.is_some() || emit.define {
                    let value = value
                        .map(|value| bind_static_this(class_name, tokens_of(field), &value))
                        .transpose()
                        .map_err(|()| unsupported_super(&member.span))?;
                    after_class.push_str(&store_static(emit, class_name, &field.name, value));
                }
                erase_member(edits, member);
            }
        } else if let Some(block) = &member.static_block {
            if emit.es2022 {
                continue;
            }
            let body = render_span(source, edits, block.span.start, block.span.end);
            let inner = body
                .strip_prefix("static")
                .map(str::trim_start)
                .and_then(|text| text.strip_prefix('{'))
                .and_then(|text| text.strip_suffix('}'))
                .ok_or_else(|| unsupported_super(&member.span))?;
            if mentions(inner, "super") {
                return Err(unsupported_super(&member.span));
            }
            after_class.push_str(&format!(" (function () {{{inner}}}).call({class_name});"));
            erase_member(edits, member);
        }
    }
    if !after_class.is_empty() {
        insert(edits, class.span.end, after_class);
    }
    Ok(())
}

/// `this.name = value;` or its `Object.defineProperty` form.
fn store_this(emit: FieldEmit, name: &str, value: Option<String>) -> String {
    if emit.define {
        format!(
            " Object.defineProperty(this, {name:?}, {});",
            descriptor(value)
        )
    } else {
        format!(
            " this.{name} = {};",
            value.unwrap_or_else(|| "void 0".into())
        )
    }
}

fn store_static(emit: FieldEmit, class_name: &str, name: &str, value: Option<String>) -> String {
    if emit.define {
        format!(
            " Object.defineProperty({class_name}, {name:?}, {});",
            descriptor(value)
        )
    } else {
        format!(
            " {class_name}.{name} = {};",
            value.unwrap_or_else(|| "void 0".into())
        )
    }
}

fn descriptor(value: Option<String>) -> String {
    format!(
        "{{ enumerable: true, configurable: true, writable: true, value: {} }}",
        value.unwrap_or_else(|| "void 0".into())
    )
}

/// A static initializer runs after the class, so `this` in it must mean the
/// class. When it mentions `this`, evaluate it as a function called on the
/// class rather than rewriting the token, which keeps a nested method's own
/// `this` intact. A `super` reference has no such function form.
fn bind_static_this(class_name: &str, tokens: &[Token], value: &str) -> Result<String, ()> {
    if tokens.iter().any(|token| token.is("super")) {
        return Err(());
    }
    if tokens.iter().any(|token| token.is("this")) {
        Ok(format!(
            "(function () {{ return {value}; }}).call({class_name})"
        ))
    } else {
        Ok(value.to_string())
    }
}

fn tokens_of(field: &ClassField) -> &[Token] {
    field.initializer.as_deref().unwrap_or(&[])
}

fn is_private_name(field: &ClassField) -> bool {
    field.name.starts_with('#')
}

fn mentions(text: &str, word: &str) -> bool {
    text.match_indices(word).any(|(index, _)| {
        let before = text[..index].chars().next_back();
        let after = text[index + word.len()..].chars().next();
        let identifier = |character: Option<char>| {
            character.is_some_and(|character| {
                character.is_alphanumeric() || matches!(character, '_' | '$')
            })
        };
        !identifier(before) && !identifier(after)
    })
}

fn implementation(class: &ClassDeclaration) -> Option<&crate::parser::ClassConstructor> {
    class
        .members
        .iter()
        .filter_map(|member| member.constructor.as_ref())
        .find(|constructor| constructor.body.is_some())
}

/// Puts `text` at the start of the constructor body, after the `super(...)`
/// statement in a derived class, adding a constructor when there is none.
fn insert_prologue(
    class: &ClassDeclaration,
    text: &str,
    edits: &mut Vec<TextEdit>,
) -> Result<(), Diagnostic> {
    match implementation(class) {
        Some(constructor) => {
            let Some(insertion) = constructor.prologue_insertion else {
                return Err(missing_super(constructor.span.clone()));
            };
            insert(edits, insertion.offset, text.to_string());
        }
        None => {
            let constructor = if class.extends_name.is_some() {
                format!(" constructor() {{ super(...arguments);{text} }}")
            } else {
                format!(" constructor() {{{text} }}")
            };
            insert(edits, class.body_span.start + 1, constructor);
        }
    }
    Ok(())
}

fn missing_super(span: SourceSpan) -> Diagnostic {
    Diagnostic::error(
        DiagnosticCode::UnsupportedSyntax,
        span,
        "a derived constructor whose class needs statements at its start must have a top-level \
         `super(...)` statement to place them after",
    )
}

fn unsupported_super(span: &SourceSpan) -> Diagnostic {
    Diagnostic::error(
        DiagnosticCode::UnsupportedSyntax,
        span.clone(),
        "a static initializer or block that uses `super` cannot be lowered for this target",
    )
}

fn insert(edits: &mut Vec<TextEdit>, offset: usize, text: String) {
    edits.push(TextEdit {
        start: offset,
        end: offset,
        replacement: text,
    });
}

/// Removes a member's text, and the recorded edits inside it, from the output.
fn erase_member(edits: &mut Vec<TextEdit>, member: &ClassMemberShell) {
    replace_member(edits, member, String::new());
}

fn replace_member(edits: &mut Vec<TextEdit>, member: &ClassMemberShell, text: String) {
    edits.retain(|edit| !(edit.start >= member.span.start && edit.end <= member.span.end));
    edits.push(TextEdit {
        start: member.span.start,
        end: member.span.end,
        replacement: text,
    });
}

/// The source text of `tokens` with the recorded edits inside it applied.
fn render_tokens(source: &str, edits: &[TextEdit], tokens: &[Token]) -> String {
    match (tokens.first(), tokens.last()) {
        (Some(first), Some(last)) => render_span(source, edits, first.start, last.end),
        _ => String::new(),
    }
}

fn render_span(source: &str, edits: &[TextEdit], start: usize, end: usize) -> String {
    let inner: Vec<TextEdit> = edits
        .iter()
        .filter(|edit| edit.start >= start && edit.end <= end)
        .map(|edit| TextEdit {
            start: edit.start - start,
            end: edit.end - start,
            replacement: edit.replacement.clone(),
        })
        .collect();
    apply_edits(&source[start..end], inner).javascript
}
