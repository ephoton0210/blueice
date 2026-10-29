// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

//! Legacy (`experimentalDecorators`) decorators and decorator metadata (J.5.4).
//!
//! The legacy semantics are the pre-standard ones and are kept apart from the
//! standard lowering: decorators run when the class is *defined*, as calls
//! after it. A class with decorators on itself or its constructor parameters
//! becomes `let C = class C { .. };` followed by one `__decorate` call per
//! decorated element and `C = __decorate([..], C);`; members keep their text.
//! The calls come in TypeScript's order: every decorated instance member in
//! source order, then every static member, then the class. A member's decorator
//! list is its decorators, its parameter decorators (`__param(i, d)`), and, with
//! `emitDecoratorMetadata`, `design:type`, `design:paramtypes` and
//! `design:returntype`.
//!
//! The three helpers are the specified behavior of `Reflect.decorate` and its
//! fallback, written here and versioned (`bluets-legacy-decorator-helper-v2`).
//! Metadata types are serialized the way TypeScript does for the types BlueTS
//! can resolve (primitives, arrays, functions, local classes, aliases, enums);
//! a type naming something it cannot resolve (an imported class) is refused
//! rather than guessed.

use super::{Module, TextEdit};
use crate::compiler::CompilerOptions;
use crate::diagnostic::{Diagnostic, DiagnosticCode, SourceSpan};
use crate::parser::{
    ClassDeclaration, ClassMemberKind, ClassMemberShell, Declaration, Decorator, Parameter, Type,
};

mod auto_accessors;
mod metadata;

pub(crate) fn metadata_imports(
    module: &Module,
    project: &crate::Project,
) -> std::collections::BTreeSet<String> {
    metadata::runtime_imports(module, project)
}

pub(super) fn prepare_super(
    module: &Module,
    options: &CompilerOptions,
    edits: &mut Vec<TextEdit>,
) -> Result<(), Diagnostic> {
    if !options.experimental_decorators {
        return Ok(());
    }
    let mut used = crate::lex(&module.id, &module.source)
        .unwrap_or_default()
        .iter()
        .map(|token| token.text.trim_start_matches('#').to_string())
        .collect();
    for (declaration, _) in super::runtime_declarations(&module.declarations) {
        let Declaration::Class(class) = declaration else {
            continue;
        };
        if !needs_work(class) {
            continue;
        }
        for shell in &class.members {
            let tokens = if shell.kind == ClassMemberKind::StaticBlock {
                &class.body[shell.token_start..shell.token_end]
            } else if let Some(field) = shell.field.as_ref().filter(|field| field.is_static) {
                let Some(tokens) = &field.initializer else {
                    continue;
                };
                tokens.as_slice()
            } else {
                continue;
            };
            super::decorators::super_members::lower_references(
                module,
                tokens,
                &format!("Object.getPrototypeOf({})", class.name),
                &class.name,
                false,
                &mut used,
                edits,
            )?;
        }
    }
    Ok(())
}

/// The version of the emitted legacy-decorator helper text.
pub const LEGACY_DECORATOR_HELPER_V1_VERSION: &str = "bluets-legacy-decorator-helper-v2";

const DECORATE_HELPER: &str = "var __bluetsDecorate = function (decorators, target, key, desc) { var argc = arguments.length, result = argc < 3 ? target : desc === null ? desc = Object.getOwnPropertyDescriptor(target, key) : desc, decorator; if (typeof Reflect === \"object\" && typeof Reflect.decorate === \"function\") result = Reflect.decorate(decorators, target, key, desc); else for (var i = decorators.length - 1; i >= 0; i--) if (decorator = decorators[i]) result = (argc < 3 ? decorator(result) : argc > 3 ? decorator(target, key, result) : decorator(target, key)) || result; return argc > 3 && result && Object.defineProperty(target, key, result), result; };";
const PARAM_HELPER: &str = "var __bluetsParam = function (index, decorator) { return function (target, key) { decorator(target, key, index); }; };";
const METADATA_HELPER: &str = "var __bluetsMetadata = function (key, value) { if (typeof Reflect === \"object\" && typeof Reflect.metadata === \"function\") return Reflect.metadata(key, value); };";

pub(super) fn is_helper_insertion(edit: &TextEdit) -> bool {
    edit.start == edit.end && edit.replacement.starts_with(DECORATE_HELPER)
}

fn unsupported(span: &SourceSpan, message: impl Into<String>) -> Diagnostic {
    Diagnostic::error(DiagnosticCode::UnsupportedSyntax, span.clone(), message)
}

/// Whether the class is rewritten by this pass.
pub(super) fn lowers_class(class: &ClassDeclaration) -> bool {
    !class.decorators.is_empty() || constructor_has_parameter_decorators(class)
}

fn constructor_has_parameter_decorators(class: &ClassDeclaration) -> bool {
    class.members.iter().any(|member| {
        member.constructor.as_ref().is_some_and(|constructor| {
            constructor
                .parameters
                .iter()
                .any(|p| !p.decorators.is_empty())
        })
    })
}

fn member_is_decorated(shell: &ClassMemberShell) -> bool {
    !shell.decorators.is_empty()
        || shell
            .method
            .as_ref()
            .is_some_and(|method| method.parameters.iter().any(|p| !p.decorators.is_empty()))
}

fn needs_work(class: &ClassDeclaration) -> bool {
    lowers_class(class) || class.members.iter().any(member_is_decorated)
}

pub(super) fn lower_legacy_decorators(
    module: &Module,
    project: &crate::Project,
    options: &CompilerOptions,
    references: &std::collections::BTreeMap<String, super::namespaces::Replacement>,
    edits: &mut Vec<TextEdit>,
) -> Result<(), Diagnostic> {
    auto_accessors::lower(module, options, edits)?;
    let mut first_start: Option<usize> = None;
    let mut need_param = false;
    let mut need_metadata = false;
    for (declaration, _) in super::runtime_declarations(&module.declarations) {
        let Declaration::Class(class) = declaration else {
            continue;
        };
        if !needs_work(class) {
            continue;
        }
        let mut lowerer = Lowerer {
            module,
            project,
            references,
            options,
            edits,
            need_param: false,
        };
        lowerer.class(class)?;
        need_param |= lowerer.need_param;
        need_metadata |= options.emit_decorator_metadata;
        let helper_start = super::decorators::namespace_owner(module, class)
            .map_or(class.span.start, |(_, outer_start)| outer_start);
        first_start = Some(first_start.map_or(helper_start, |at| at.min(helper_start)));
    }
    if let Some(at) = first_start {
        let mut text = format!("{DECORATE_HELPER} ");
        if need_param {
            text.push_str(PARAM_HELPER);
            text.push(' ');
        }
        if need_metadata {
            text.push_str(METADATA_HELPER);
            text.push(' ');
        }
        edits.push(TextEdit {
            start: at,
            end: at,
            replacement: text,
        });
    }
    Ok(())
}

struct Lowerer<'a, 'e> {
    module: &'a Module,
    project: &'a crate::Project,
    references: &'a std::collections::BTreeMap<String, super::namespaces::Replacement>,
    options: &'a CompilerOptions,
    edits: &'e mut Vec<TextEdit>,
    need_param: bool,
}

fn quoted(text: &str) -> String {
    format!("\"{}\"", text.replace('\\', "\\\\").replace('"', "\\\""))
}

impl Lowerer<'_, '_> {
    fn push(&mut self, start: usize, end: usize, replacement: String) {
        self.edits.push(TextEdit {
            start,
            end,
            replacement,
        });
    }

    /// Erases a decorator and the whitespace after it.
    fn erase(&mut self, decorator: &Decorator) {
        let source = &self.module.source;
        let mut end = decorator.span.end;
        while source[end..].starts_with([' ', '\t', '\r', '\n']) {
            end += 1;
        }
        self.push(decorator.span.start, end, String::new());
    }

    fn decorator_text(&self, decorator: &Decorator) -> Result<String, Diagnostic> {
        let first = decorator.tokens.first().expect("a decorator has tokens");
        let last = decorator.tokens.last().expect("a decorator has tokens");
        if self.module.source[first.start..last.end].contains("//") {
            return Err(unsupported(
                &decorator.span,
                "a decorator expression with a line comment is not lowered",
            ));
        }
        Ok(super::decorators::relocated(first.start, last.end))
    }

    fn param_entries(&mut self, parameters: &[Parameter]) -> Result<Vec<String>, Diagnostic> {
        let mut entries = Vec::new();
        for (index, parameter) in parameters.iter().enumerate() {
            for decorator in &parameter.decorators {
                self.need_param = true;
                let anchor = super::relocations::source_anchor(
                    decorator
                        .tokens
                        .first()
                        .expect("a decorator has tokens")
                        .start,
                );
                entries.push(format!(
                    "{anchor}__bluetsParam({index}, {})",
                    self.decorator_text(decorator)?
                ));
                self.erase(decorator);
            }
        }
        Ok(entries)
    }

    fn class(&mut self, class: &ClassDeclaration) -> Result<(), Diagnostic> {
        let mut statements = String::new();
        let constructor = class
            .members
            .iter()
            .find_map(|member| member.constructor.as_ref().map(|ctor| (member, ctor)));

        // ---- members, instance first then static ---------------------------
        let mut done_accessors: Vec<(String, bool)> = Vec::new();
        for want_static in [false, true] {
            for shell in &class.members {
                let (name, is_static, kind) = match shell.kind {
                    ClassMemberKind::Method => match &shell.method {
                        Some(method) if method.body.is_some() => {
                            (method.name.clone(), method.is_static, ElementKind::Method)
                        }
                        _ => continue,
                    },
                    ClassMemberKind::Accessor => match &shell.accessor {
                        Some(accessor) => (
                            accessor.name.clone(),
                            accessor.is_static,
                            if accessor.getter {
                                ElementKind::Getter
                            } else {
                                ElementKind::Setter
                            },
                        ),
                        None => continue,
                    },
                    ClassMemberKind::Field => match &shell.field {
                        Some(field) if !field.accessor => {
                            (field.name.clone(), field.is_static, ElementKind::Field)
                        }
                        Some(field) => (
                            field.name.clone(),
                            field.is_static,
                            ElementKind::AutoAccessor,
                        ),
                        None => continue,
                    },
                    _ => {
                        if let Some(decorator) = shell.decorators.first() {
                            return Err(unsupported(
                                &decorator.span,
                                "this decorated member shape is not lowered",
                            ));
                        }
                        continue;
                    }
                };
                if is_static != want_static || !member_is_decorated(shell) {
                    if is_static == want_static || !member_is_decorated(shell) {
                        // Erase decorators once, in the instance pass or the static one.
                    }
                    if is_static != want_static {
                        continue;
                    }
                    if !member_is_decorated(shell) {
                        continue;
                    }
                }
                if name.starts_with('#') {
                    return Err(unsupported(
                        &shell.span,
                        "decorators on private names are not valid with experimentalDecorators",
                    ));
                }
                // A getter/setter pair is decorated once, through its first member.
                if matches!(kind, ElementKind::Getter | ElementKind::Setter) {
                    let key = (name.clone(), is_static);
                    let sibling_decorated = class.members.iter().any(|other| {
                        !std::ptr::eq(other, shell)
                            && other
                                .accessor
                                .as_ref()
                                .is_some_and(|a| a.name == name && a.is_static == is_static)
                            && !other.decorators.is_empty()
                    });
                    if sibling_decorated && !shell.decorators.is_empty() {
                        return Err(unsupported(
                            &shell.decorators[0].span,
                            "decorators cannot be applied to both the getter and the setter of the same name",
                        ));
                    }
                    if done_accessors.contains(&key) {
                        continue;
                    }
                    done_accessors.push(key);
                }
                let mut entries = Vec::new();
                for decorator in &shell.decorators {
                    entries.push(self.decorator_text(decorator)?);
                    self.erase(decorator);
                }
                let parameters: &[Parameter] = match (&shell.method, &shell.accessor) {
                    (Some(method), _) => &method.parameters,
                    (_, Some(accessor)) => &accessor.parameters,
                    _ => &[],
                };
                entries.extend(self.param_entries(parameters)?);
                if self.options.emit_decorator_metadata {
                    entries.extend(self.member_metadata(class, shell, kind)?);
                }
                let target = if is_static {
                    class.name.clone()
                } else {
                    format!("{}.prototype", class.name)
                };
                let descriptor = if kind == ElementKind::Field {
                    "void 0"
                } else {
                    "null"
                };
                let anchor = super::relocations::source_anchor(
                    shell.key.first().map_or(shell.span.start, |key| key.start),
                );
                let end_anchor = super::relocations::source_anchor(shell.span.end);
                let unmapped = super::relocations::unmapped();
                statements.push_str(&format!(
                    " {anchor}__bluetsDecorate([{}{unmapped}], {target}, {}, {descriptor}){end_anchor};{unmapped}",
                    entries.join(", "),
                    quoted(&name)
                ));
            }
        }

        // ---- the class -----------------------------------------------------
        let class_decorated = lowers_class(class);
        let mut class_entries = Vec::new();
        for decorator in &class.decorators {
            class_entries.push(self.decorator_text(decorator)?);
        }
        if let Some((_, ctor)) = constructor {
            class_entries.extend(self.param_entries(&ctor.parameters)?);
        }
        if let (true, true, Some((_, ctor))) = (
            class_decorated,
            self.options.emit_decorator_metadata,
            constructor,
        ) {
            let types = self.parameter_types(class, &ctor.parameters)?;
            class_entries.push(format!(
                "{}__bluetsMetadata(\"design:paramtypes\", [{}])",
                super::relocations::unmapped(),
                types.join(", ")
            ));
        }
        if class_decorated {
            let name_anchor = super::relocations::source_anchor(class.name_span.start);
            let name_end_anchor = super::relocations::source_anchor(class.name_span.end);
            let class_end_anchor = super::relocations::source_anchor(class.body_span.end);
            let unmapped = super::relocations::unmapped();
            statements.push_str(&format!(
                " {name_anchor}{0} {name_end_anchor}= {name_end_anchor}__bluetsDecorate([{1}{unmapped}], {name_anchor}{0}{name_end_anchor}){class_end_anchor};{unmapped}",
                class.name,
                class_entries.join(", ")
            ));
        }

        // ---- rewrite the class's own text -------------------------------------
        let newlines = |from: usize, to: usize| {
            "\n".repeat(self.module.source[from..to].matches('\n').count())
        };
        if class_decorated {
            let header_end = class.name_span.start;
            let class_anchor = super::relocations::source_anchor(
                super::decorators::class_keyword_start(class, &self.module.source),
            );
            let name_anchor = super::relocations::source_anchor(class.name_span.start);
            let name_end_anchor = super::relocations::source_anchor(class.name_span.end);
            let header = format!(
                "{class_anchor}let {name_anchor}{} {name_end_anchor}= {class_anchor}class {}",
                class.name,
                newlines(class.span.start, header_end)
            );
            self.push(class.span.start, header_end, header);
            let close = class.body_span.end - 1;
            let export_suffix = if class.exported {
                if let Some((namespace, _)) = super::decorators::namespace_owner(self.module, class)
                {
                    format!(" {namespace}.{0} = {0};", class.name)
                } else if self.options.module_kind == crate::compiler::ModuleKind::CommonJs {
                    format!(" exports.{} = {};", class.export_name(), class.name)
                } else if class.default_export {
                    format!(" export default {};", class.name)
                } else {
                    format!(" export {{ {} }};", class.name)
                }
            } else {
                String::new()
            };
            self.push(
                close,
                close + 1,
                format!(
                    "{}{}{};{}",
                    super::relocations::unmapped(),
                    "}",
                    super::relocations::source_anchor(class.body_span.end),
                    super::relocations::unmapped()
                ),
            );
            self.push(
                class.span.end,
                class.span.end,
                format!("{statements}{export_suffix}"),
            );
        } else if !statements.is_empty() {
            // A member-only legacy transform copies the class body, but the
            // closing brace immediately before generated decorator calls is
            // synthetic in TypeScript's emitted map, not a user breakpoint.
            let close = class.body_span.end - 1;
            self.push(
                close,
                close + 1,
                format!("{}{}", super::relocations::unmapped(), "}"),
            );
            self.push(class.span.end, class.span.end, statements);
        }
        Ok(())
    }

    fn member_metadata(
        &self,
        class: &ClassDeclaration,
        shell: &ClassMemberShell,
        kind: ElementKind,
    ) -> Result<Vec<String>, Diagnostic> {
        let metadata = |key: &str, value: String| {
            format!(
                "{}__bluetsMetadata(\"{key}\", {value})",
                super::relocations::unmapped()
            )
        };
        match kind {
            ElementKind::Field | ElementKind::AutoAccessor => {
                let field = shell.field.as_ref().expect("a field member");
                let annotation = field.declared_type().ok_or_else(|| {
                    unsupported(&field.span, "decorator metadata needs this field's type")
                })?;
                Ok(vec![metadata(
                    "design:type",
                    self.serialize(class, &annotation, &field.span)?,
                )])
            }
            ElementKind::Method => {
                let method = shell.method.as_ref().expect("a method member");
                let params = self.parameter_types(class, &method.parameters)?;
                let returned = match &method.return_type {
                    Some(value) => self.serialize(class, value, &method.span)?,
                    None => {
                        return Err(unsupported(
                            &method.span,
                            "decorator metadata needs this method's return type annotation",
                        ))
                    }
                };
                Ok(vec![
                    metadata("design:type", "Function".to_string()),
                    metadata("design:paramtypes", format!("[{}]", params.join(", "))),
                    metadata("design:returntype", returned),
                ])
            }
            ElementKind::Getter | ElementKind::Setter => {
                let this = shell.accessor.as_ref().expect("an accessor member");
                // TypeScript describes the pair: the type of the getter's result
                // (else the setter's parameter), and the setter's parameters.
                let find = |getter: bool| {
                    class
                        .members
                        .iter()
                        .filter_map(|member| member.accessor.as_ref())
                        .find(|accessor| {
                            accessor.name == this.name
                                && accessor.is_static == this.is_static
                                && accessor.getter == getter
                        })
                };
                let (getter, setter) = (find(true), find(false));
                let value = getter
                    .and_then(|accessor| accessor.return_type.clone())
                    .or_else(|| {
                        setter.and_then(|accessor| {
                            accessor
                                .parameters
                                .first()
                                .and_then(|p| p.annotation.clone())
                        })
                    });
                let Some(value) = value else {
                    return Err(unsupported(
                        &this.span,
                        "decorator metadata needs this accessor's type annotation",
                    ));
                };
                let params = match setter {
                    Some(accessor) => self.parameter_types(class, &accessor.parameters)?,
                    None => Vec::new(),
                };
                Ok(vec![
                    metadata("design:type", self.serialize(class, &value, &this.span)?),
                    metadata("design:paramtypes", format!("[{}]", params.join(", "))),
                ])
            }
        }
    }

    fn parameter_types(
        &self,
        class: &ClassDeclaration,
        parameters: &[Parameter],
    ) -> Result<Vec<String>, Diagnostic> {
        parameters
            .iter()
            .map(|parameter| match &parameter.annotation {
                Some(value) => self.serialize(class, value, &parameter.span),
                None => Ok("Object".to_string()),
            })
            .collect()
    }
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum ElementKind {
    Method,
    Getter,
    Setter,
    Field,
    AutoAccessor,
}
