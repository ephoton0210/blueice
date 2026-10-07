// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

//! Bounded syntax indexes shared by related diagnostic causes.
use super::*;
use crate::{ClassDeclaration, Parameter};

pub(super) struct Field {
    pub(super) name: String,
    pub(super) owner: String,
    pub(super) value: Type,
    pub(super) name_span: SourceSpan,
    pub(super) span: SourceSpan,
    pub(super) class_member: bool,
    pub(super) container: Option<Type>,
}
impl Field {
    pub(super) fn related_span(&self) -> SourceSpan {
        if self.class_member || !matches!(self.value, Type::Function { .. }) {
            self.name_span.clone()
        } else {
            self.span.clone()
        }
    }
}
pub(super) struct Index<'a> {
    pub(super) project: &'a Project,
    tokens: BTreeMap<String, Vec<Token>>,
    classes: Vec<&'a ClassDeclaration>,
    functions: Vec<&'a crate::FunctionDeclaration>,
    fields: Vec<Field>,
}
impl<'a> Index<'a> {
    pub(super) fn new(project: &'a Project) -> Self {
        let tokens = project
            .modules
            .iter()
            .filter_map(|(id, module)| {
                crate::syntax::lex(id, &module.source)
                    .ok()
                    .map(|tokens| (id.clone(), tokens))
            })
            .collect();
        let mut index = Self {
            project,
            tokens,
            classes: Vec::new(),
            functions: Vec::new(),
            fields: Vec::new(),
        };
        for module in project.modules.values() {
            index.declarations(&module.declarations);
        }
        index
    }
    fn declarations(&mut self, declarations: &'a [Declaration]) {
        for declaration in declarations {
            match declaration {
                Declaration::Namespace(ns) => self.declarations(&ns.body),
                Declaration::Class(class) => {
                    self.classes.push(class);
                    for member in &class.members {
                        let Some(name) = &member.name else { continue };
                        let span = member
                            .field
                            .as_ref()
                            .map(|field| field.name_span.clone())
                            .or_else(|| {
                                member
                                    .accessor
                                    .as_ref()
                                    .map(|field| field.name_span.clone())
                            })
                            .unwrap_or_else(|| self.name_in(name, &member.span));
                        let value = member
                            .field
                            .as_ref()
                            .and_then(|field| field.declared_type())
                            .or_else(|| {
                                member.method.as_ref().map(|method| Type::Function {
                                    parameters: method.parameters.clone(),
                                    result: Box::new(
                                        method.return_type.clone().unwrap_or(Type::Any),
                                    ),
                                })
                            })
                            .unwrap_or(Type::Any);
                        self.fields.push(Field {
                            name: name.clone(),
                            owner: class.name.clone(),
                            value,
                            name_span: span,
                            span: member.span.clone(),
                            class_member: true,
                            container: None,
                        });
                    }
                }
                Declaration::Function(function) => {
                    self.functions.push(function);
                    for parameter in &function.type_parameters {
                        if let Some(constraint) = &parameter.constraint {
                            self.type_fields("", constraint);
                        }
                    }
                    for parameter in &function.parameters {
                        if let Some(ty) = &parameter.annotation {
                            self.type_fields("", ty)
                        };
                    }
                    if let Some(ty) = &function.return_type {
                        self.type_fields("", ty);
                    }
                    for local in &function.locals {
                        if let Some(ty) = &local.annotation {
                            self.type_fields("", ty);
                        }
                    }
                }
                Declaration::Interface(interface) => {
                    self.type_fields(&interface.name, &interface.body_type())
                }
                Declaration::TypeAlias(alias) => self.type_fields(&alias.name, &alias.value),
                Declaration::Variable(variable) => {
                    if let Some(ty) = &variable.annotation {
                        self.type_fields("", ty)
                    }
                }
                _ => {}
            }
        }
    }
    fn type_fields(&mut self, owner: &str, ty: &Type) {
        match ty {
            Type::Record(fields) | Type::CallableRecord { fields, .. } => {
                for field in fields {
                    let span = self.name_in(&field.name, &field.span);
                    self.fields.push(Field {
                        name: field.name.clone(),
                        owner: owner.into(),
                        value: field.value.clone(),
                        name_span: span,
                        span: field.span.clone(),
                        class_member: false,
                        container: Some(ty.clone()),
                    });
                    self.type_fields("", &field.value);
                }
            }
            Type::Union(values) | Type::Intersection(values) => {
                for value in values {
                    self.type_fields(owner, value)
                }
            }
            _ => {}
        }
    }
    fn name_in(&self, name: &str, span: &SourceSpan) -> SourceSpan {
        self.tokens
            .get(&span.module)
            .into_iter()
            .flatten()
            .find(|token| token.start >= span.start && token.end <= span.end && token.is(name))
            .map(|token| token.span(&span.module))
            .unwrap_or_else(|| span.clone())
    }
    pub(super) fn field(&self, name: &str, owner: &str, module: &str) -> Option<&Field> {
        self.fields
            .iter()
            .filter(|field| field.name == name)
            .min_by_key(|field| {
                (
                    usize::from(!owner.is_empty() && owner != field.owner),
                    usize::from(field.span.module != module),
                    field.span.start,
                )
            })
    }
    pub(super) fn expected_owner(&self, field: &Field, span: &SourceSpan) -> String {
        if field.owner.is_empty() {
            return field
                .container
                .as_ref()
                .map(|ty| super::super::type_text::render_in(ty, self.project))
                .unwrap_or_default();
        }
        if let Some(module) = self.project.modules.get(&span.module) {
            for declaration in &module.declarations {
                let Declaration::Variable(variable) = declaration else {
                    continue;
                };
                if !(variable.span.start <= span.start && span.end <= variable.span.end) {
                    continue;
                }
                let Some(Type::Named { name, arguments }) = &variable.annotation else {
                    continue;
                };
                if name != &field.owner {
                    continue;
                }
                let defaults = module.declarations.iter().find_map(|declaration| {
                    let Declaration::Interface(interface) = declaration else {
                        return None;
                    };
                    (interface.name == *name)
                        .then(|| {
                            interface
                                .type_parameters
                                .iter()
                                .map(|p| p.default.clone())
                                .collect::<Option<Vec<_>>>()
                        })
                        .flatten()
                });
                let arguments = if arguments.is_empty() {
                    defaults.unwrap_or_default()
                } else {
                    arguments.clone()
                };
                if !arguments.is_empty() {
                    return super::super::type_text::render_in(
                        &Type::Named {
                            name: name.clone(),
                            arguments,
                        },
                        self.project,
                    );
                }
            }
        }
        let source = self.project.source(&span.module).unwrap_or("");
        let mut owner = field.owner.clone();
        if source.contains('<') && source.contains(&format!("{}<", field.owner)) {
            if let Some(start) = source.rfind(&format!("{}<", field.owner)) {
                if let Some(end) = source[start..].find('>') {
                    owner = source[start..start + end + 1].into();
                }
            }
        }
        if span.module.ends_with(".tsx") && !field.owner.is_empty() {
            format!("IntrinsicAttributes & {owner}")
        } else {
            owner
        }
    }
    pub(super) fn jsx_owner(&self, span: &SourceSpan, children: bool) -> Option<String> {
        super::jsx::owner(self.project, span, children, &self.classes, &self.functions)
    }
    pub(super) fn named(
        &self,
        name: &str,
        span: &SourceSpan,
        kind: &str,
        earlier: bool,
    ) -> Option<SourceSpan> {
        let mut found = Vec::new();
        for (module, tokens) in &self.tokens {
            for (i, token) in tokens
                .iter()
                .enumerate()
                .filter(|(_, token)| token.is(name))
            {
                let previous = i
                    .checked_sub(1)
                    .and_then(|i| tokens.get(i))
                    .map(|t| t.text.as_str())
                    .unwrap_or("");
                let next = tokens.get(i + 1).map(|t| t.text.as_str()).unwrap_or("");
                let in_import = tokens[..i]
                    .iter()
                    .rposition(|t| t.is("import"))
                    .is_some_and(|begin| !tokens[begin..i].iter().any(|t| t.is(";")));
                let eligible = match kind {
                    "class" => previous == "class",
                    "import" => in_import,
                    "property" => matches!(next, ":" | "(" | "?" | "!"),
                    _ => matches!(previous, "const" | "let" | "var" | "class" | "function"),
                };
                if eligible && (module != &span.module || token.start != span.start) {
                    found.push((
                        usize::from(module != &span.module),
                        usize::from(if earlier {
                            token.start >= span.start
                        } else {
                            token.start < span.start
                        }),
                        token.start.abs_diff(span.start),
                        token.span(module),
                    ));
                }
            }
        }
        found.sort_by_key(|item| (item.0, item.1, item.2));
        found.into_iter().next().map(|item| item.3)
    }
    pub(super) fn implementation(
        &self,
        span: &SourceSpan,
    ) -> Option<(&'a [Parameter], SourceSpan)> {
        if let Some(overload) = self.functions.iter().find(|function| {
            function.overload
                && function.span.module == span.module
                && function.span.start <= span.start
                && function.span.end >= span.end
        }) {
            if let Some(implementation) = self.functions.iter().find(|function| {
                function.name == overload.name
                    && function.span.module == span.module
                    && !function.overload
                    && !function.declared
            }) {
                return Some((
                    &implementation.parameters,
                    self.name_in(&implementation.name, &implementation.span),
                ));
            }
        }
        let class = self.classes.iter().find(|class| {
            class.span.module == span.module
                && class.span.start <= span.start
                && class.span.end >= span.end
        })?;
        let name = class
            .members
            .iter()
            .find(|member| member.span.start <= span.start && member.span.end >= span.end)?
            .name
            .as_deref()?;
        let member = class.members.iter().find(|member| {
            member.name.as_deref() == Some(name)
                && (member
                    .constructor
                    .as_ref()
                    .is_some_and(|ctor| ctor.body.is_some())
                    || member
                        .method
                        .as_ref()
                        .is_some_and(|method| method.body.is_some()))
        })?;
        let parameters = member
            .constructor
            .as_ref()
            .map(|ctor| ctor.parameters.as_slice())
            .or_else(|| {
                member
                    .method
                    .as_ref()
                    .map(|method| method.parameters.as_slice())
            })?;
        Some((parameters, self.name_in(name, &member.span)))
    }
    fn call_name(&self, span: &SourceSpan) -> Option<String> {
        let tokens = self.tokens.get(&span.module)?;
        let at = tokens.partition_point(|token| token.start < span.start);
        if tokens
            .get(at + 1)
            .is_some_and(|token| token.is("(") || token.is("<"))
        {
            return tokens.get(at).map(|token| token.text.clone());
        }
        let end = tokens.partition_point(|token| token.start < span.end);
        let inside = tokens
            .get(at..end)
            .and_then(|selected| selected.iter().position(|token| token.is("(")))
            .map(|index| at + index);
        let open = inside.or_else(|| {
            tokens[..=at.min(tokens.len() - 1)]
                .iter()
                .rposition(|token| token.is("("))
        })?;
        let name = tokens.get(open.checked_sub(1)?)?.text.clone();
        Some(name)
    }
    pub(super) fn call_implementation(
        &self,
        span: &SourceSpan,
    ) -> Option<(&'a [Parameter], SourceSpan)> {
        let name = self.call_name(span)?;
        for class in &self.classes {
            let constructor = name == class.name
                || name == "super"
                || self.classes.iter().any(|child| {
                    child.name == name && child.extends_name.as_deref() == Some(&class.name)
                });
            for member in &class.members {
                if constructor
                    && member
                        .constructor
                        .as_ref()
                        .is_some_and(|ctor| ctor.body.is_some())
                {
                    return Some((
                        &member.constructor.as_ref()?.parameters,
                        self.name_in("constructor", &member.span),
                    ));
                }
                if member.name.as_deref() == Some(&name)
                    && member
                        .method
                        .as_ref()
                        .is_some_and(|method| method.body.is_some())
                {
                    return Some((
                        &member.method.as_ref()?.parameters,
                        self.name_in(&name, &member.span),
                    ));
                }
            }
        }
        self.functions
            .iter()
            .find(|function| function.name == name && !function.overload)
            .map(|function| {
                (
                    function.parameters.as_slice(),
                    self.name_in(&name, &function.span),
                )
            })
    }
    pub(super) fn missing_parameter(&self, diagnostic: &Diagnostic) -> Option<&'a Parameter> {
        let counterpart = diagnostic.typescript.as_ref()?;
        let parameters = if counterpart.code == 2554 {
            let name = self.call_name(&counterpart.span)?;
            self.functions
                .iter()
                .find(|function| {
                    function.name == name
                        && function.overload
                        && function.span.module == counterpart.span.module
                })
                .map(|function| function.parameters.as_slice())
                .or_else(|| {
                    self.call_implementation(&counterpart.span)
                        .map(|(parameters, _)| parameters)
                })?
        } else {
            let source = self.project.source(&counterpart.span.module)?;
            let selected = source
                .get(counterpart.span.start..counterpart.span.end)?
                .trim_start_matches('@');
            self.functions
                .iter()
                .find(|function| selected.starts_with(&function.name))?
                .parameters
                .as_slice()
        };
        let actual = if counterpart.code == 2554 {
            counterpart.arguments.get(1)?.parse::<usize>().ok()?
        } else {
            diagnostic
                .message
                .split_whitespace()
                .find_map(|word| word.parse::<usize>().ok())
                .unwrap_or(2)
        };
        parameters
            .get(actual)
            .filter(|parameter| !parameter.optional && parameter.default.is_none())
    }
}
