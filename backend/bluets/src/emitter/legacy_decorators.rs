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
//! fallback, written here and versioned (`bluets-legacy-decorator-helper-v1`).
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

/// The version of the emitted legacy-decorator helper text.
pub const LEGACY_DECORATOR_HELPER_V1_VERSION: &str = "bluets-legacy-decorator-helper-v1";

const DECORATE_HELPER: &str = "var __bluetsDecorate = function (decorators, target, key, desc) { var argc = arguments.length, result = argc < 3 ? target : desc === null ? desc = Object.getOwnPropertyDescriptor(target, key) : desc, decorator; if (typeof Reflect === \"object\" && typeof Reflect.decorate === \"function\") result = Reflect.decorate(decorators, target, key, desc); else for (var i = decorators.length - 1; i >= 0; i--) if (decorator = decorators[i]) result = (argc < 3 ? decorator(result) : argc > 3 ? decorator(target, key, result) : decorator(target, key)) || result; return argc > 3 && result && Object.defineProperty(target, key, result), result; };";
const PARAM_HELPER: &str = "var __bluetsParam = function (index, decorator) { return function (target, key) { decorator(target, key, index); }; };";
const METADATA_HELPER: &str = "var __bluetsMetadata = function (key, value) { if (typeof Reflect === \"object\" && typeof Reflect.metadata === \"function\") return Reflect.metadata(key, value); };";

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
    options: &CompilerOptions,
    edits: &mut Vec<TextEdit>,
) -> Result<(), Diagnostic> {
    let mut first_start: Option<usize> = None;
    let mut need_param = false;
    let mut need_metadata = false;
    for (declaration, nested) in super::runtime_declarations(&module.declarations) {
        let Declaration::Class(class) = declaration else {
            continue;
        };
        if !needs_work(class) {
            continue;
        }
        if nested {
            return Err(unsupported(
                &class.name_span,
                "a decorated class inside a namespace is not lowered yet",
            ));
        }
        let mut lowerer = Lowerer {
            module,
            options,
            edits,
            need_param: false,
        };
        lowerer.class(class)?;
        need_param |= lowerer.need_param;
        need_metadata |= options.emit_decorator_metadata;
        first_start = Some(first_start.map_or(class.span.start, |at| at.min(class.span.start)));
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
                entries.push(format!(
                    "__bluetsParam({index}, {})",
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
                        Some(field) => {
                            if !shell.decorators.is_empty() {
                                return Err(unsupported(
                                    &field.span,
                                    "auto-accessors are not combined with experimentalDecorators",
                                ));
                            }
                            continue;
                        }
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
                statements.push_str(&format!(
                    " __bluetsDecorate([{}], {target}, {}, {descriptor});",
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
                "__bluetsMetadata(\"design:paramtypes\", [{}])",
                types.join(", ")
            ));
        }
        if class_decorated {
            statements.push_str(&format!(
                " {0} = __bluetsDecorate([{1}], {0});",
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
            let header = format!(
                "let {} = class {}",
                class.name,
                newlines(class.span.start, header_end)
            );
            self.push(class.span.start, header_end, header);
            let close = class.body_span.end - 1;
            let export_suffix = if class.exported {
                if self.options.module_kind == crate::compiler::ModuleKind::CommonJs {
                    format!(" exports.{0} = {0};", class.name)
                } else {
                    format!(" export {{ {} }};", class.name)
                }
            } else {
                String::new()
            };
            self.push(close, close + 1, "};".to_string());
            self.push(
                class.span.end,
                class.span.end,
                format!("{statements}{export_suffix}"),
            );
        } else if !statements.is_empty() {
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
        let metadata = |key: &str, value: String| format!("__bluetsMetadata(\"{key}\", {value})");
        match kind {
            ElementKind::Field => {
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

    /// TypeScript's `serializeTypeNode` for the types BlueTS resolves.
    fn serialize(
        &self,
        class: &ClassDeclaration,
        value: &Type,
        span: &SourceSpan,
    ) -> Result<String, Diagnostic> {
        self.serialize_at(class, value, span, 0)
    }

    fn serialize_at(
        &self,
        class: &ClassDeclaration,
        value: &Type,
        span: &SourceSpan,
        depth: usize,
    ) -> Result<String, Diagnostic> {
        if depth > 8 {
            return Ok("Object".to_string());
        }
        Ok(match value {
            Type::Predicate(predicate) => {
                self.serialize_at(class, &predicate.runtime_type(), span, depth + 1)?
            }
            Type::Number => "Number".to_string(),
            Type::String => "String".to_string(),
            Type::Boolean => "Boolean".to_string(),
            Type::Literal(text) => {
                if text.starts_with(['"', '\'', '`']) {
                    "String".to_string()
                } else if matches!(text.as_str(), "true" | "false") {
                    "Boolean".to_string()
                } else {
                    "Number".to_string()
                }
            }
            Type::Void | Type::Undefined | Type::Null | Type::Never => "void 0".to_string(),
            Type::Any | Type::Unknown | Type::Record(_) | Type::Intersection(_) => {
                "Object".to_string()
            }
            Type::Array(_) | Type::Tuple(_) => "Array".to_string(),
            Type::Function { .. } => "Function".to_string(),
            Type::Union(parts) => {
                // `null` and `undefined` do not count; one remaining kind decides.
                let mut kinds: Vec<String> = Vec::new();
                for part in parts {
                    if matches!(part, Type::Null | Type::Undefined) {
                        continue;
                    }
                    let serialized = self.serialize_at(class, part, span, depth + 1)?;
                    if serialized == "Object" {
                        return Ok("Object".to_string());
                    }
                    if !kinds.contains(&serialized) {
                        kinds.push(serialized);
                    }
                }
                match kinds.as_slice() {
                    [only] => only.clone(),
                    [] => "void 0".to_string(),
                    _ => "Object".to_string(),
                }
            }
            Type::Named { name, .. } => match name.as_str() {
                "object" => "Object".to_string(),
                "symbol" => "Symbol".to_string(),
                "bigint" => "BigInt".to_string(),
                "Array" | "ReadonlyArray" => "Array".to_string(),
                "Promise" => "Promise".to_string(),
                "Function" => "Function".to_string(),
                _ => self.serialize_named(class, name, span, depth)?,
            },
        })
    }

    fn serialize_named(
        &self,
        class: &ClassDeclaration,
        name: &str,
        span: &SourceSpan,
        depth: usize,
    ) -> Result<String, Diagnostic> {
        let _ = class;
        for declaration in &self.module.declarations {
            match declaration {
                Declaration::Class(other) if other.name == name => return Ok(name.to_string()),
                Declaration::Interface(other) if other.name == name => {
                    return Ok("Object".to_string())
                }
                Declaration::TypeAlias(alias) if alias.name == name => {
                    return self.serialize_at(class, &alias.value, span, depth + 1);
                }
                Declaration::Enum(item) if item.name == name => {
                    let numeric = item.members.iter().all(|member| {
                        member.initializer.as_ref().is_none_or(|tokens| {
                            tokens
                                .iter()
                                .all(|token| token.kind != crate::TokenKind::String)
                        })
                    });
                    return Ok(if numeric { "Number" } else { "String" }.to_string());
                }
                _ => {}
            }
        }
        Err(unsupported(
            span,
            format!("decorator metadata cannot serialize `{name}`: only types declared in this module are supported"),
        ))
    }
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum ElementKind {
    Method,
    Getter,
    Setter,
    Field,
}
