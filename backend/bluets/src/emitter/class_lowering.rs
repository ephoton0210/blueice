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

use std::collections::{BTreeMap, BTreeSet};

use super::private_lowering::{
    helper_definitions, refuse_helper_name_collisions, Helper, PrivateNames,
};
use super::{apply_edits, EmittedJavaScript, Module, TextEdit};
use crate::compiler::{CompilerOptions, EcmaTarget};
use crate::diagnostic::{Diagnostic, DiagnosticCode, SourceSpan};
use crate::parser::{ClassDeclaration, ClassField, ClassMemberShell, Declaration};
use crate::Token;

mod callable_ends;
pub(super) mod expressions;
mod heritage;
mod origins;
mod replacements;
pub(in crate::emitter) use origins::auto_accessors::mapped_generated_class_edits;

pub(super) fn has_wrapped_class_expressions(module: &Module) -> bool {
    module
        .class_expressions
        .values()
        .any(|expression| expressions::needs_wrapper(&expression.class))
}

/// The emitted form of a class's fields for one target and option set.
#[derive(Clone, Copy)]
struct FieldEmit {
    es5: bool,
    es2022: bool,
    define: bool,
    preserve_accessors: bool,
    standard_output: bool,
    static_after_instance: bool,
    static_expression: bool,
    file_private_captures: usize,
}

#[derive(Clone, Copy)]
struct FieldOrigins<'a> {
    statics: &'a BTreeMap<usize, usize>,
    auto_accessors: &'a BTreeMap<usize, origins::auto_accessors::AutoField>,
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
    wrapper: crate::ModuleKind,
) -> Result<(), Diagnostic> {
    heritage::lower(module, edits);
    lower_classes(
        module,
        options,
        edits,
        false,
        wrapper,
        FieldOrigins {
            statics: &BTreeMap::new(),
            auto_accessors: &BTreeMap::new(),
        },
        &BTreeMap::new(),
    )?;
    callable_ends::retain(module, options, edits);
    Ok(())
}

pub(super) fn lower_generated_class_members(
    module: &Module,
    options: &CompilerOptions,
    edits: &mut Vec<TextEdit>,
    original: &Module,
    previous: &EmittedJavaScript,
) -> Result<(), Diagnostic> {
    let origins = if options.target >= EcmaTarget::Es2022 && !options.defines_class_fields() {
        origins::decorated_static_blocks(
            original,
            module,
            previous,
            !options.experimental_decorators,
        )
    } else {
        BTreeMap::new()
    };
    let auto = if options.target < EcmaTarget::Es2022 {
        origins::auto_accessors::collect(
            original,
            module,
            previous,
            options.experimental_decorators,
        )
    } else {
        BTreeMap::new()
    };
    let replacements = if !options.experimental_decorators && options.target < EcmaTarget::Es2022 {
        replacements::collect(original, module)
    } else {
        BTreeMap::new()
    };
    lower_classes(
        module,
        options,
        edits,
        true,
        options.module_kind,
        FieldOrigins {
            statics: &origins,
            auto_accessors: &auto,
        },
        &replacements,
    )?;
    origins::auto_accessors::retain_publication_order(original, module, previous, options, edits);
    Ok(())
}

fn lower_classes(
    module: &Module,
    options: &CompilerOptions,
    edits: &mut Vec<TextEdit>,
    standard_output: bool,
    wrapper: crate::ModuleKind,
    origins: FieldOrigins<'_>,
    replacements: &BTreeMap<usize, replacements::Replacement>,
) -> Result<(), Diagnostic> {
    let auto_origins = origins.auto_accessors;
    let emit = FieldEmit {
        es5: options.target == EcmaTarget::Es5,
        es2022: options.target >= EcmaTarget::Es2022,
        define: options.defines_class_fields(),
        preserve_accessors: options.target == EcmaTarget::EsNext,
        standard_output: standard_output && !options.experimental_decorators,
        static_after_instance: false,
        static_expression: false,
        file_private_captures: expressions::names::file_captures(module, options),
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
        let replacement = replacements.get(&class.span.start);
        if !standard_output
            && options.experimental_decorators
            && options.target != EcmaTarget::EsNext
            && class
                .members
                .iter()
                .any(|member| member.field.as_ref().is_some_and(|field| field.accessor))
        {
            // Legacy auto-accessors first become backing stores and descriptors;
            // the owned-output pass then lowers those actual generated members.
            continue;
        }
        // Standard decorators first build an ES2022 class. Its generated fields
        // are lowered in a separate owned-output pass, after decorator initialization.
        if !standard_output
            && !options.experimental_decorators
            && super::decorators::lowers_class(class, options)
        {
            native_parameter_properties(class, edits)?;
            continue;
        }
        if emit.native() {
            native_parameter_properties(class, edits)?;
            continue;
        }
        if !emit.es2022 {
            if let Some(expression) = module.class_expression(class.span.start) {
                if expressions::needs_wrapper(class) {
                    expressions::lower(module, expression, emit, edits, auto_origins, replacement)?;
                    continue;
                }
            }
        }
        // Below ES2022 a private name is lowered to external state; on ES2022
        // with assign semantics it stays native beside the lowered fields.
        let private = if emit.es2022 {
            None
        } else {
            PrivateNames::collect_with_origins(
                module,
                class,
                !standard_output,
                wrapper != crate::ModuleKind::System,
            )?
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
        lower_class_with_static_origins(
            module,
            class,
            emit,
            private.as_ref(),
            edits,
            origins,
            replacement,
        )?;
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
        .map(|field| parameter_assignment(class, field))
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
    auto_origins: &BTreeMap<usize, origins::auto_accessors::AutoField>,
    replacement: Option<&replacements::Replacement>,
) -> Result<(), Diagnostic> {
    lower_class_with_static_origins(
        module,
        class,
        emit,
        private,
        edits,
        FieldOrigins {
            statics: &BTreeMap::new(),
            auto_accessors: auto_origins,
        },
        replacement,
    )
}

fn lower_class_with_static_origins(
    module: &Module,
    class: &ClassDeclaration,
    mut emit: FieldEmit,
    private: Option<&PrivateNames<'_>>,
    edits: &mut Vec<TextEdit>,
    origins: FieldOrigins<'_>,
    replacement: Option<&replacements::Replacement>,
) -> Result<(), Diagnostic> {
    let static_origins = origins.statics;
    let auto_origins = origins.auto_accessors;
    let source = &module.source;
    let class_name = &class.name;
    emit.static_expression = !emit.es2022
        && module
            .class_expression(class.span.start)
            .is_some_and(|expression| !expression.statement);
    emit.static_after_instance = !emit.standard_output
        && super::legacy_decorators::lowers_class(class)
        || (!emit.standard_output || emit.es2022)
            && (class.members.iter().any(|member| {
                member.field.as_ref().is_some_and(|field| {
                    !field.is_static
                        && !field.declared
                        && (emit.define || field.initializer.is_some())
                })
            }) || emit.define && !class.parameter_property_fields().is_empty());
    let keys = super::computed_fields::capture(module, class, emit.es2022, edits)?;

    // Instance side: the private brand, then parameter properties, then
    // declared fields in source order.
    let mut prologue = private
        .map(PrivateNames::brand_statement)
        .unwrap_or_default();
    for property in class.parameter_property_fields() {
        prologue.push_str(&if !emit.define {
            parameter_assignment(class, &property)
        } else {
            store_this(emit, &property.name, Some(property.name.clone()))
        });
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
        if is_private_name(field) {
            // Below ES2022 it is stored outside the object; otherwise the
            // native private field stays where it is.
            if let Some(private) = private {
                let (statement, nested) =
                    if let Some(origin) = auto_origins.get(&field.name_span.start) {
                        origins::auto_accessors::instance(source, edits, field, private, *origin)
                    } else {
                        private.field_store(field, source, edits)
                    };
                prologue.push_str(&statement);
                if auto_origins.contains_key(&field.name_span.start) {
                    erase_generated_member(source, edits, member);
                } else {
                    erase_member(edits, member);
                }
                edits.extend(nested);
            }
            continue;
        }
        if field.initializer.is_some() || emit.define {
            prologue.push_str(&origins::field_statement(
                source,
                edits,
                emit,
                field,
                keys.get(&member.span.start),
            ));
        }
        if emit.standard_output {
            erase_generated_member(source, edits, member);
        } else {
            erase_member(edits, member);
        }
    }
    if !prologue.is_empty() {
        insert_prologue(
            source,
            class,
            &prologue,
            edits,
            emit.define,
            emit.es5,
            private,
        )?;
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
            if replacement.is_some_and(|replacement| replacement.omits_field(field)) {
                erase_generated_member(source, edits, member);
                continue;
            }
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
                let nested = if let Some(origin) = auto_origins.get(&field.name_span.start) {
                    let (statement, nested) = origins::auto_accessors::static_statement(
                        source, edits, field, private, class_name, *origin,
                    )
                    .map_err(|()| unsupported_super(&member.span))?;
                    statics.push_str(&statement);
                    nested
                } else {
                    let value = value
                        .map(|value| bind_static_this(class_name, tokens_of(field), &value))
                        .transpose()
                        .map_err(|()| unsupported_super(&member.span))?;
                    statics.push_str(&private.static_field_store(&field.name, value));
                    Vec::new()
                };
                if auto_origins.contains_key(&field.name_span.start) {
                    erase_generated_member(source, edits, member);
                } else {
                    erase_member(edits, member);
                }
                edits.extend(nested);
            } else if emit.es2022 {
                // ES2022 with assign semantics: a static block in place.
                let replacement = value
                    .as_ref()
                    .map(|_| {
                        format!(
                            "{}static {{{} }}",
                            static_origins
                                .get(&field.name_span.start)
                                .copied()
                                .or_else(|| (!member.decorators.is_empty()).then_some(
                                    if emit.standard_output {
                                        field.span.start
                                    } else {
                                        field.name_span.start
                                    }
                                ))
                                .map(super::relocations::source_anchor)
                                .unwrap_or_else(|| super::relocations::unmapped().to_string()),
                            origins::static_statement(
                                source,
                                edits,
                                emit,
                                field,
                                keys.get(&member.span.start),
                                "this",
                                false,
                            )
                        )
                    })
                    .unwrap_or_default();
                if emit.standard_output && replacement.is_empty() {
                    erase_generated_member(source, edits, member);
                } else {
                    replace_member(edits, member, replacement);
                }
            } else {
                if value.is_some() || emit.define {
                    let _visible_bound_value = value
                        .map(|value| bind_static_this(class_name, tokens_of(field), &value))
                        .transpose()
                        .map_err(|()| unsupported_super(&member.span))?;
                    let replacement_receiver =
                        replacement.and_then(|replacement| replacement.field_receiver(field));
                    let field_emit = if replacement_receiver.is_some() {
                        FieldEmit {
                            static_expression: false,
                            static_after_instance: true,
                            ..emit
                        }
                    } else {
                        emit
                    };
                    statics.push_str(&origins::static_statement(
                        source,
                        edits,
                        field_emit,
                        field,
                        keys.get(&member.span.start),
                        replacement_receiver.unwrap_or(class_name),
                        tokens_of(field).iter().any(|token| token.is("this")),
                    ));
                }
                if emit.standard_output {
                    erase_generated_member(source, edits, member);
                } else {
                    erase_member(edits, member);
                }
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
            let tokens = &class.body[member.token_start..member.token_end];
            let (Some(open), Some(close)) = (
                tokens.iter().find(|token| token.is("{")),
                tokens.last().filter(|token| token.is("}")),
            ) else {
                return Err(unsupported_super(&member.span));
            };
            let (inner, nested) = render_relocated_span(source, edits, open.end, close.start);
            let receiver = replacement
                .and_then(|replacement| replacement.block_receiver(member.span.start))
                .unwrap_or(class_name);
            statics.push_str(&format!(
                " {}(function () {{{inner}}}{}).call({receiver});",
                super::relocations::source_anchor(block.span.start),
                super::relocations::source_anchor(close.end)
            ));
            if emit.standard_output {
                erase_generated_member(source, edits, member);
            } else {
                erase_member(edits, member);
            }
            edits.extend(nested);
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
                if emit.standard_output {
                    erase_generated_member(source, edits, member);
                } else {
                    erase_member(edits, member);
                }
            }
        }
    }
    after_class.push_str(&statics);
    if !emit.es5
        && !emit.es2022
        && !emit.standard_output
        && module
            .class_expression(class.span.start)
            .is_none_or(|expression| expression.statement)
        && class
            .members
            .iter()
            .any(|member| member.static_block.is_some())
        && !class.members.iter().any(|member| {
            member.field.as_ref().is_some_and(|field| {
                field.is_static && !field.declared && field.initializer.is_some()
            })
        })
    {
        if let Some(rest) = after_class.strip_prefix(' ') {
            // The original class completes before its moved block. Reuse the
            // existing space as its carrier, leaving the generated IIFE U.
            after_class = format!(
                "{} {}{rest}",
                super::relocations::source_anchor(class.span.end),
                super::relocations::unmapped()
            );
        }
    }
    if !after_class.is_empty() {
        insert(edits, class.span.end, after_class);
    }
    Ok(())
}

/// `this.name = value;` or its `Object.defineProperty` form.
fn store_this(emit: FieldEmit, name: &str, value: Option<String>) -> String {
    store_field(emit, "this", name, None, value)
}

fn parameter_assignment(class: &ClassDeclaration, property: &ClassField) -> String {
    let start = property.name_span.start;
    let end = class
        .body
        .iter()
        .find(|token| token.start == start)
        .map_or(property.name_span.end, |token| token.end);
    let start = super::relocations::source_anchor(start);
    let end = super::relocations::source_anchor(end);
    format!(" {start}this.{0}{end} = {start}{0}{end};", property.name)
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
    if mentions(value, "super") {
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
    let Ok(tokens) = crate::lex("<class-initializer>", text) else {
        return true;
    };
    let mut index = 0;
    while index < tokens.len() {
        if word == "super" && tokens[index].is("class") {
            let mut cursor = index + 1;
            while cursor < tokens.len() && !tokens[cursor].is("{") {
                if tokens[cursor].is("super") {
                    return true;
                }
                if tokens[cursor].is("(") || tokens[cursor].is("[") {
                    let Some(end) = super::decorators::super_members::close(&tokens, cursor) else {
                        return true;
                    };
                    if tokens[cursor..=end].iter().any(|token| token.is("super")) {
                        return true;
                    }
                    cursor = end + 1;
                } else {
                    cursor += 1;
                }
            }
            if let Some(end) = super::decorators::super_members::close(&tokens, cursor) {
                // A nested class retains its own home. Computed keys still need
                // the outer lexical context, so keep their conservative boundary.
                let body = &tokens[cursor..=end];
                if body.iter().any(|token| token.is("["))
                    && body.iter().any(|token| token.is("super"))
                {
                    return true;
                }
                index = end + 1;
                continue;
            }
        }
        let token = &tokens[index];
        if (token.is(word)
            && !index
                .checked_sub(1)
                .is_some_and(|before| tokens[before].is(".") || tokens[before].is("?.")))
            || token.kind == crate::TokenKind::Template && token.text.contains(word)
        {
            return true;
        }
        index += 1;
    }
    false
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
    source: &str,
    class: &ClassDeclaration,
    text: &str,
    edits: &mut Vec<TextEdit>,
    define: bool,
    es5: bool,
    private: Option<&PrivateNames<'_>>,
) -> Result<(), Diagnostic> {
    match implementation(class) {
        Some(constructor) => {
            let Some(insertion) = constructor.prologue_insertion else {
                let mut diagnostic = missing_super(constructor.span.clone());
                if !define {
                    let tokens = class
                        .body
                        .iter()
                        .filter(|token| {
                            constructor.span.start <= token.start
                                && token.end <= constructor.span.end
                        })
                        .collect::<Vec<_>>();
                    if let Some(start) = tokens
                        .windows(2)
                        .position(|tokens| tokens[0].is("super") && tokens[1].is("("))
                    {
                        let mut depth = 0;
                        let mut end = start + 1;
                        for (index, token) in tokens.iter().enumerate().skip(start + 1) {
                            if token.is("(") {
                                depth += 1;
                            } else if token.is(")") {
                                depth -= 1;
                                if depth == 0 {
                                    end = index;
                                    break;
                                }
                            }
                        }
                        diagnostic = diagnostic.with_typescript(2401, Vec::new());
                        if let Some(counterpart) = &mut diagnostic.typescript {
                            counterpart.span = SourceSpan::new(
                                &class.span.module,
                                tokens[start].start,
                                tokens[end].end,
                            );
                        }
                    }
                }
                return Err(diagnostic);
            };
            // Copying the old body's leading trivia at the same generated
            // position would replace the moved field's explicit endpoint.
            // Transport that existing whitespace with the prologue instead;
            // the emitted bytes and complete CRLF pairs stay unchanged.
            let suffix = &source[insertion.offset..];
            let length = suffix
                .find(|character: char| !character.is_ascii_whitespace())
                .unwrap_or(suffix.len());
            let entry = if insertion.item_index == 0 && class.extends_span.is_none() {
                let origin = if constructor.parameters.is_empty() {
                    Some(constructor.span.start)
                } else {
                    class
                        .body
                        .iter()
                        .rev()
                        .find(|token| {
                            token.is(")")
                                && constructor.span.start <= token.start
                                && token.end < insertion.offset
                        })
                        .map(|token| token.start)
                };
                origin
                    .map(super::relocations::source_anchor)
                    .unwrap_or_default()
            } else {
                String::new()
            };
            edits.push(TextEdit {
                start: insertion.offset,
                end: insertion.offset + length,
                replacement: format!("{entry}{text}{}", &suffix[..length]),
            });
        }
        None => {
            let constructor_origin = if !class.decorators.is_empty()
                && !class.members.is_empty()
                && class.members.iter().all(|member| {
                    !member.decorators.is_empty()
                        && member
                            .field
                            .as_ref()
                            .is_some_and(|field| !field.accessor && !field.name.starts_with('#'))
                }) {
                if es5 {
                    crate::lex(
                        &class.span.module,
                        &source[class.span.start..class.name_span.start],
                    )
                    .ok()
                    .and_then(|tokens| {
                        tokens.into_iter().find(|token| {
                            !class.decorators.iter().any(|decorator| {
                                decorator.span.start <= class.span.start + token.start
                                    && class.span.start + token.end <= decorator.span.end
                            })
                        })
                    })
                    .map_or(class.span.start, |token| class.span.start + token.start)
                } else {
                    super::decorators::class_keyword_start(class, source)
                }
            } else {
                class.span.start
            };
            let mut constructor = if class.extends_name.is_some() {
                format!(
                    " {}constructor() {{ {}super(...arguments);{text} {} }}{} ",
                    super::relocations::source_anchor(constructor_origin),
                    super::relocations::unmapped(),
                    super::relocations::source_anchor(class.body_span.end - 1),
                    super::relocations::source_anchor(class.body_span.end)
                )
            } else {
                format!(
                    " {}constructor() {{{text} {} }}{} ",
                    super::relocations::source_anchor(constructor_origin),
                    super::relocations::source_anchor(class.body_span.end - 1),
                    super::relocations::source_anchor(class.body_span.end)
                )
            };
            if let Some(private) = private {
                constructor = private.constructor_header(constructor);
            }
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

/// An erased generated field leaves generated trivia before the synthesized
/// constructor, rather than restoring its moved initializer's source endpoint.
fn erase_generated_member(source: &str, edits: &mut Vec<TextEdit>, member: &ClassMemberShell) {
    let mut end = member.span.end;
    while source
        .as_bytes()
        .get(end)
        .is_some_and(u8::is_ascii_whitespace)
    {
        end += 1;
    }
    let lines = source[member.span.start..member.span.end]
        .chars()
        .filter(|character| matches!(character, '\r' | '\n'))
        .collect::<String>();
    edits.retain(|edit| !(edit.start >= member.span.start && edit.end <= member.span.end));
    edits.push(TextEdit {
        start: member.span.start,
        end,
        replacement: format!(
            "{}{lines}{}{}",
            super::relocations::unmapped(),
            &source[member.span.end..end],
            super::relocations::unmapped()
        ),
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
            // Relocations retain full-module coordinates even when the text
            // consumer needs only this local span and no source provenance.
            replacement: super::relocations::expand(&edit.replacement, source, edits)
                .iter()
                .map(super::relocations::Part::text)
                .collect(),
        })
        .collect();
    apply_edits(&source[start..end], inner).javascript
}

/// Assemble a moved span without discarding its original relocation markers.
pub(super) fn render_relocated_span(
    source: &str,
    edits: &[TextEdit],
    start: usize,
    end: usize,
) -> (String, Vec<TextEdit>) {
    let mut inner = edits
        .iter()
        .filter(|edit| edit.start >= start && edit.end <= end)
        .collect::<Vec<_>>();
    inner.sort_by_key(|edit| (edit.start, edit.end));
    let mut rendered = String::new();
    let mut nested = Vec::new();
    let mut cursor = start;
    for edit in inner {
        if edit.start < cursor {
            // A relocation in the owning replacement can still apply a
            // nested plain edit. Direct edits are already serialized below.
            if !edit.replacement.contains('\0') {
                nested.push(edit.clone());
            }
            continue;
        }
        if edit.end < edit.start {
            continue;
        }
        if cursor < edit.start {
            rendered.push_str(&super::relocations::relocated_verbatim(cursor, edit.start));
        }
        if edit.replacement.is_empty() {
            rendered.push_str(super::relocations::unmapped());
            rendered.extend(
                source[edit.start..edit.end]
                    .chars()
                    .filter(|ch| matches!(ch, '\r' | '\n')),
            );
        } else {
            rendered.push_str(&if edit.start == edit.end {
                super::relocations::unmapped().into()
            } else {
                super::relocations::source_anchor(edit.start)
            });
            rendered.push_str(&edit.replacement);
        }
        cursor = edit.end;
    }
    if cursor < end {
        rendered.push_str(&super::relocations::relocated_verbatim(cursor, end));
    }
    (rendered, nested)
}
