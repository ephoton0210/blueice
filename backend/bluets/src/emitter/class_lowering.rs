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

use std::collections::BTreeSet;

use super::private_lowering::{
    helper_definitions, refuse_helper_name_collisions, Helper, PrivateNames,
};
use super::{apply_edits, Module, TextEdit};
use crate::compiler::{CompilerOptions, EcmaTarget};
use crate::diagnostic::{Diagnostic, DiagnosticCode, SourceSpan};
use crate::parser::{ClassDeclaration, ClassField, ClassMemberShell, Declaration};
use crate::Token;

mod expressions;

/// The emitted form of a class's fields for one target and option set.
#[derive(Clone, Copy)]
struct FieldEmit {
    es2022: bool,
    define: bool,
    preserve_accessors: bool,
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
    lower_classes(module, options, edits, false)
}

pub(super) fn lower_generated_class_members(
    module: &Module,
    options: &CompilerOptions,
    edits: &mut Vec<TextEdit>,
) -> Result<(), Diagnostic> {
    lower_classes(module, options, edits, true)
}

fn lower_classes(
    module: &Module,
    options: &CompilerOptions,
    edits: &mut Vec<TextEdit>,
    standard_output: bool,
) -> Result<(), Diagnostic> {
    let emit = FieldEmit {
        es2022: options.target >= EcmaTarget::Es2022,
        define: options.defines_class_fields(),
        preserve_accessors: options.target == EcmaTarget::EsNext,
    };
    let mut needed: BTreeSet<Helper> = BTreeSet::new();
    let mut first_private_class: Option<usize> = None;
    let mut classes = super::runtime_declarations(&module.declarations)
        .into_iter()
        .filter_map(|(declaration, nested)| {
            if let Declaration::Class(class) = declaration {
                Some((class, nested))
            } else {
                None
            }
        })
        .chain(
            module
                .class_expressions
                .values()
                .map(|expression| (&expression.class, false)),
        )
        .collect::<Vec<_>>();
    // Lower children before a field initializer or private method moves them.
    classes.sort_by_key(|(class, _)| std::cmp::Reverse(class.span.start));
    for (class, nested) in classes {
        // Standard decorators first build an ES2022 class. Its generated fields
        // are lowered in a separate owned-output pass, after decorator initialization.
        if !standard_output
            && !options.experimental_decorators
            && super::decorators::lowers_class(class, options)
        {
            continue;
        }
        if emit.native() {
            native_parameter_properties(class, edits)?;
            continue;
        }
        if !emit.es2022 {
            if let Some(expression) = module.class_expression(class.span.start) {
                if expressions::needs_wrapper(class) {
                    expressions::lower(module, expression, emit, edits)?;
                    continue;
                }
            }
        }
        // Below ES2022 a private name is lowered to external state; on ES2022
        // with assign semantics it stays native beside the lowered fields.
        let private = if emit.es2022 {
            None
        } else {
            PrivateNames::collect(module, class)?
        };
        if private.is_some() && nested {
            // The helpers and state would have to be placed in the namespace's
            // own function, once per scope.
            return Err(Diagnostic::error(
                DiagnosticCode::UnsupportedSyntax,
                class.name_span.clone(),
                "a private name in a class inside a namespace is not lowered for this target yet",
            ));
        }
        if let Some(private) = &private {
            refuse_helper_name_collisions(module)?;
            private.rewrite_accesses(module, edits, &mut needed)?;
            insert(
                edits,
                class.span.start,
                format!("var {}; ", private.variables().join(", ")),
            );
            first_private_class =
                Some(first_private_class.map_or(class.span.start, |at| at.min(class.span.start)));
        }
        lower_class(module, class, emit, private.as_ref(), edits)?;
    }
    if let Some(at) = first_private_class {
        let definitions = helper_definitions(&needed);
        if !definitions.is_empty() {
            insert(edits, at, definitions);
        }
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
    private: Option<&PrivateNames<'_>>,
    edits: &mut Vec<TextEdit>,
) -> Result<(), Diagnostic> {
    let source = &module.source;
    let class_name = &class.name;
    let keys = super::computed_fields::capture(module, class, emit.es2022, edits)?;

    // Instance side: the private brand, then parameter properties, then
    // declared fields in source order.
    let mut prologue = private
        .map(PrivateNames::brand_statement)
        .unwrap_or_default();
    for property in class.parameter_property_fields() {
        prologue.push_str(&store_this(
            emit,
            &property.name,
            Some(property.name.clone()),
        ));
    }
    for member in &class.members {
        if member.abstract_modifier.is_some()
            || member.field.as_ref().is_some_and(|field| field.declared)
        {
            continue;
        }
        let Some(field) = member.field.as_ref().filter(|field| !field.is_static) else {
            continue;
        };
        if emit.preserve_accessors && field.accessor {
            continue;
        }
        let value = field
            .initializer
            .as_deref()
            .map(|tokens| render_tokens(source, edits, tokens));
        if is_private_name(field) {
            // Below ES2022 it is stored outside the object; otherwise the
            // native private field stays where it is.
            if let Some(private) = private {
                prologue.push_str(&private.field_store(&field.name, value));
                erase_member(edits, member);
            }
            continue;
        }
        if value.is_some() || emit.define {
            prologue.push_str(&store_field(
                emit,
                "this",
                &field.name,
                keys.get(&member.span.start),
                value,
            ));
        }
        erase_member(edits, member);
    }
    if !prologue.is_empty() {
        insert_prologue(class, &prologue, edits)?;
    }

    // After the class: the private storage and functions first (a static
    // initializer may use them), then the static members in source order.
    let mut after_class = private
        .map(PrivateNames::instance_setup)
        .unwrap_or_default();
    let mut statics = String::new();
    for member in &class.members {
        if member.abstract_modifier.is_some()
            || member.field.as_ref().is_some_and(|field| field.declared)
        {
            continue;
        }
        if let Some(field) = member.field.as_ref().filter(|field| field.is_static) {
            if emit.preserve_accessors && field.accessor {
                continue;
            }
            let value = field
                .initializer
                .as_deref()
                .map(|tokens| render_tokens(source, edits, tokens));
            if is_private_name(field) {
                let Some(private) = private else {
                    continue;
                };
                let value = value
                    .map(|value| bind_static_this(class_name, tokens_of(field), &value))
                    .transpose()
                    .map_err(|()| unsupported_super(&member.span))?;
                statics.push_str(&private.static_field_store(&field.name, value));
                erase_member(edits, member);
            } else if emit.es2022 {
                // ES2022 with assign semantics: a static block in place.
                replace_member(
                    edits,
                    member,
                    value
                        .map(|value| {
                            format!(
                                "static {{{} }}",
                                store_field(
                                    emit,
                                    "this",
                                    &field.name,
                                    keys.get(&member.span.start),
                                    Some(value)
                                )
                            )
                        })
                        .unwrap_or_default(),
                );
            } else {
                if value.is_some() || emit.define {
                    let value = value
                        .map(|value| bind_static_this(class_name, tokens_of(field), &value))
                        .transpose()
                        .map_err(|()| unsupported_super(&member.span))?;
                    statics.push_str(&store_field(
                        emit,
                        class_name,
                        &field.name,
                        keys.get(&member.span.start),
                        value,
                    ));
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
            statics.push_str(&format!(" (function () {{{inner}}}).call({class_name});"));
            erase_member(edits, member);
        } else if let Some(private) = private {
            let name = member
                .method
                .as_ref()
                .map(|method| method.name.as_str())
                .or_else(|| {
                    member
                        .accessor
                        .as_ref()
                        .map(|accessor| accessor.name.as_str())
                });
            if name.is_some_and(|name| name.starts_with('#')) {
                let rendered = render_span(source, edits, member.span.start, member.span.end);
                after_class.push_str(&private.function_definition(member, &rendered)?);
                erase_member(edits, member);
            }
        }
    }
    after_class.push_str(&statics);
    if !after_class.is_empty() {
        insert(edits, class.span.end, after_class);
    }
    Ok(())
}

/// `this.name = value;` or its `Object.defineProperty` form.
fn store_this(emit: FieldEmit, name: &str, value: Option<String>) -> String {
    store_field(emit, "this", name, None, value)
}

fn store_field(
    emit: FieldEmit,
    receiver: &str,
    name: &str,
    key: Option<&String>,
    value: Option<String>,
) -> String {
    let literal = format!("{name:?}");
    let key_text = key.map_or(literal.as_str(), String::as_str);
    if emit.define {
        format!(
            " Object.defineProperty({receiver}, {key_text}, {});",
            descriptor(value)
        )
    } else {
        let access = if let Some(key) = key {
            format!("[{key}]")
        } else if name
            .chars()
            .all(|c| c.is_alphanumeric() || matches!(c, '_' | '$'))
            && !name.chars().next().is_some_and(|c| c.is_ascii_digit())
        {
            format!(".{name}")
        } else {
            format!("[{literal}]")
        };
        format!(
            " {receiver}{access} = {};",
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
pub(super) fn render_tokens(source: &str, edits: &[TextEdit], tokens: &[Token]) -> String {
    match (tokens.first(), tokens.last()) {
        (Some(first), Some(last)) => render_span(source, edits, first.start, last.end),
        _ => String::new(),
    }
}

pub(super) fn render_span(source: &str, edits: &[TextEdit], start: usize, end: usize) -> String {
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
