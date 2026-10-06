// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

//! Scope construction over the parser's structured declarations and bodies.

use super::*;
use crate::parser::{BindingPattern, ClassDeclaration, TypeParameter, VariableDeclaration};

impl ScopeModel<'_> {
    pub(super) fn declarations(&mut self, declarations: &[Declaration], scope: ScopeId) {
        let cyclic = cyclic_classes(declarations);
        for declaration in declarations {
            let name = match declaration {
                Declaration::Function(d) => Some(&d.name),
                Declaration::Class(d) => Some(&d.name),
                Declaration::Enum(d) => Some(&d.name),
                Declaration::Namespace(d) => Some(&d.name),
                _ => None,
            };
            if let Some(name) = name {
                if let Some(token) = self
                    .tokens_in(declaration.span())
                    .iter()
                    .find(|t| t.text == *name)
                {
                    self.declaration_names.insert(token.start);
                }
            }
            match declaration {
                Declaration::Variable(variable) => self.variable(variable, scope),
                Declaration::Function(function) => {
                    self.value(
                        scope,
                        &function.name,
                        0,
                        false,
                        false,
                        function_type(&function.parameters, &function.return_type),
                    );
                    self.binding_kind(scope, &function.name, BindingKind::Function);
                    self.function(function, scope);
                }
                Declaration::TypeAlias(alias) => {
                    self.type_declarations.insert(
                        (scope, alias.name.clone()),
                        (alias.span.clone(), alias.exported),
                    );
                    self.scopes[scope].types.insert(alias.name.clone());
                    let inner = self.generic_scope(scope, &alias.span, &alias.type_parameters);
                    self.type_scopes(&alias.value, &alias.span, inner);
                }
                Declaration::Interface(interface) => {
                    self.type_declarations.insert(
                        (scope, interface.name.clone()),
                        (interface.span.clone(), interface.exported),
                    );
                    self.scopes[scope].types.insert(interface.name.clone());
                    let inner =
                        self.generic_scope(scope, &interface.span, &interface.type_parameters);
                    for field in &interface.fields {
                        self.type_scopes(&field.value, &field.span, inner);
                    }
                }
                Declaration::Import(import) => {
                    for binding in &import.bindings {
                        if let Some(token) = self
                            .tokens_in(&import.span)
                            .iter()
                            .rev()
                            .find(|t| t.text == binding.local)
                        {
                            self.declaration_names.insert(token.start);
                        }
                        let source = self
                            .project
                            .resolutions
                            .get(&(self.module.id.clone(), import.specifier.clone()))
                            .and_then(|id| self.project.modules.get(id));
                        let pure_type = source.is_some_and(|source| {
                            exported_value(&source.declarations, &binding.imported) == Some(false)
                        });
                        if binding.type_only || import.type_only || pure_type {
                            self.value(scope, &binding.local, 0, false, true, Type::Unknown);
                        }
                        self.binding_kind(
                            scope,
                            &binding.local,
                            if binding.imported == "*" {
                                BindingKind::NamespaceImport
                            } else {
                                BindingKind::Import
                            },
                        );
                        // Qualified namespace members resolve from the bound
                        // type map; a namespace root is not itself a type.
                        if self.qualified_types.contains(&binding.local) {
                            self.scopes[scope].types.insert(binding.local.clone());
                        }
                    }
                }
                Declaration::Class(class) => {
                    self.scopes[scope].types.insert(class.name.clone());
                    let constructor = self.scopes[scope]
                        .values
                        .get(&class.name)
                        .map(|binding| binding.declared_type.clone())
                        .unwrap_or(Type::Unknown);
                    self.value(scope, &class.name, class.span.end, true, false, constructor);
                    self.binding_kind(scope, &class.name, BindingKind::Class);
                    self.class(class, scope, !cyclic.contains(class.name.as_str()));
                }
                Declaration::Enum(enumeration) => {
                    self.scopes[scope].types.insert(enumeration.name.clone());
                    self.value(
                        scope,
                        &enumeration.name,
                        0,
                        false,
                        false,
                        Type::Named {
                            name: format!("typeof {}", enumeration.name),
                            arguments: Vec::new(),
                        },
                    );
                    self.binding_kind(scope, &enumeration.name, BindingKind::Enum);
                    let inner =
                        self.child(Some(scope), enumeration.body_span.clone(), false, false);
                    for member in &enumeration.members {
                        self.value(inner, &member.name, 0, false, false, Type::Unknown);
                        if let Some(tokens) = &member.initializer {
                            self.expression(tokens, inner);
                        }
                    }
                }
                Declaration::Namespace(namespace) => {
                    let has_value = namespace_has_value(&namespace.body);
                    // A merged namespace retains the class/function/enum's
                    // value identity and initialization point. A namespace
                    // containing only types has no independent value or type
                    // meaning; its qualified members still resolve below.
                    if self.scopes[scope]
                        .values
                        .get(&namespace.name)
                        .is_none_or(|binding| has_value && binding.type_only)
                    {
                        self.value(
                            scope,
                            &namespace.name,
                            0,
                            false,
                            !has_value,
                            Type::Named {
                                name: format!("typeof {}", namespace.name),
                                arguments: Vec::new(),
                            },
                        );
                        self.binding_kind(scope, &namespace.name, BindingKind::Namespace);
                    }
                    let span = SourceSpan::new(
                        &self.module.id,
                        namespace.header_span.end,
                        namespace.closing_span.end,
                    );
                    if self.scopes[scope]
                        .values
                        .get(&namespace.name)
                        .is_some_and(|binding| binding.kind == BindingKind::Mutable)
                    {
                        self.binding_kind(scope, &namespace.name, BindingKind::Namespace);
                    }
                    let inner = self.child(Some(scope), span, true, false);
                    let outer = &self.scopes[scope].namespace_path;
                    self.scopes[inner].namespace_path = if outer.is_empty() {
                        namespace.name.clone()
                    } else {
                        format!("{outer}.{}", namespace.name)
                    };
                    self.scopes[inner].ambient_exports = namespace.exports_every_member();
                    let shared = self.scopes[scope].namespace_path.is_empty()
                        || namespace.exported
                        || namespace.implicit
                        || self.scopes[scope].ambient_exports;
                    let outer_identity = &self.scopes[scope].namespace_identity;
                    let identity = if shared {
                        if outer_identity.is_empty() {
                            namespace.name.clone()
                        } else {
                            format!("{outer_identity}.{}", namespace.name)
                        }
                    } else {
                        format!("{scope}::{}", namespace.name)
                    };
                    self.scopes[inner].namespace_identity = identity.clone();
                    if let Some(binding) = self.scopes[scope].values.get_mut(&namespace.name) {
                        binding.namespace = Some(identity.clone());
                    }
                    if let Some(members) = self.namespace_members.get(&identity) {
                        self.scopes[inner].values = members.values.clone();
                        self.scopes[inner].types = members.types.clone();
                    }
                    self.declarations(&namespace.body, inner);
                    let exported = self.namespace_members.entry(identity).or_default();
                    for item in &namespace.body {
                        let (name, public) = match item {
                            Declaration::Variable(item) => (&item.name, item.exported),
                            Declaration::Function(item) => (&item.name, item.exported),
                            Declaration::Class(item) => (&item.name, item.exported),
                            Declaration::Enum(item) => (&item.name, item.exported),
                            Declaration::Namespace(item) => (&item.name, item.exported),
                            Declaration::TypeAlias(item) => (&item.name, item.exported),
                            Declaration::Interface(item) => (&item.name, item.exported),
                            _ => continue,
                        };
                        if public || namespace.exports_every_member() {
                            if let Some(binding) = self.scopes[inner].values.get(name) {
                                exported.values.insert(name.clone(), binding.clone());
                            }
                            if self.scopes[inner].types.contains(name) {
                                exported.types.insert(name.clone());
                            }
                        }
                    }
                }
                Declaration::Raw(raw) => self.statements(&raw.tokens, scope),
                Declaration::DefaultExport(_)
                | Declaration::ValueExport(_)
                | Declaration::TypeExport(_) => {}
            }
        }
    }

    pub(super) fn generic_scope(
        &mut self,
        parent: ScopeId,
        span: &SourceSpan,
        parameters: &[TypeParameter],
    ) -> ScopeId {
        let scope = self.child(Some(parent), span.clone(), false, false);
        self.scopes[scope]
            .types
            .extend(parameters.iter().map(|parameter| parameter.name.clone()));
        scope
    }

    pub(super) fn variable(&mut self, variable: &VariableDeclaration, scope: ScopeId) {
        let tokens = self.tokens_in(&variable.span);
        if let Some(keyword) = tokens
            .iter()
            .find(|token| matches!(token.text.as_str(), "const" | "let" | "var"))
        {
            self.local_variables.insert(keyword.start, variable.clone());
        }
        self.statements(&tokens, scope);
    }

    pub(super) fn parameters(&mut self, parameters: &[Parameter], scope: ScopeId) {
        for parameter in parameters {
            if parameter.pattern.is_none() {
                self.parameters
                    .insert((scope, parameter.name.clone()), parameter.span.clone());
            }
            let value = parameter.annotation.clone().unwrap_or(Type::Unknown);
            if let Some(annotation) = &parameter.annotation {
                self.type_scopes(annotation, &parameter.span, scope);
            }
            match &parameter.pattern {
                Some(BindingPattern::Object(bindings)) => {
                    for binding in bindings {
                        self.value(
                            scope,
                            &binding.name,
                            binding.span.end,
                            false,
                            false,
                            Type::Unknown,
                        );
                        if let Some(default) = &binding.default {
                            self.expression(default, scope);
                        }
                    }
                }
                Some(BindingPattern::Array(bindings)) => {
                    for binding in bindings.iter().flatten() {
                        self.value(
                            scope,
                            &binding.name,
                            binding.span.end,
                            false,
                            false,
                            Type::Unknown,
                        );
                        if let Some(default) = &binding.default {
                            self.expression(default, scope);
                        }
                    }
                }
                None => self.value(
                    scope,
                    &parameter.name,
                    parameter.span.end,
                    false,
                    false,
                    value,
                ),
            }
            if let Some(default) = &parameter.default {
                self.expression(default, scope);
            }
        }
    }

    pub(super) fn function(&mut self, function: &FunctionDeclaration, parent: ScopeId) {
        let scope = self.child(Some(parent), function.span.clone(), true, true);
        self.scopes[scope]
            .types
            .extend(function.type_parameters.iter().map(|p| p.name.clone()));
        self.value(scope, "arguments", 0, false, false, Type::Unknown);
        self.parameters(&function.parameters, scope);
        if let Some(result) = &function.return_type {
            self.type_scopes(result, &function.span, scope);
        }
        self.function_body(&function.body, scope, &function.span);
    }

    pub(super) fn function_body(
        &mut self,
        items: &[FunctionBodyItem],
        scope: ScopeId,
        enclosing: &SourceSpan,
    ) {
        // A body scope is invisible to default parameter initializers.
        let start = items
            .first()
            .map_or(enclosing.end.saturating_sub(1), |item| {
                body_item_span(item).start
            });
        let body = self.child(
            Some(scope),
            SourceSpan::new(&self.module.id, start, enclosing.end),
            true,
            false,
        );
        self.body(items, body);
    }

    pub(super) fn body(&mut self, items: &[FunctionBodyItem], scope: ScopeId) {
        self.register_body(items);
        let (Some(first), Some(last)) = (items.first(), items.last()) else {
            return;
        };
        let span = SourceSpan::new(
            &self.module.id,
            body_item_span(first).start,
            body_item_span(last).end,
        );
        let tokens = self.tokens_in(&span);
        self.statements(&tokens, scope);
    }

    /// Opaque control-flow tokens can separate a declaration head from its
    /// structured variable item. Keep the parser's type/signature metadata,
    /// then scan the contiguous body for its lexical braces and loop heads.
    fn register_body(&mut self, items: &[FunctionBodyItem]) {
        for item in items {
            match item {
                FunctionBodyItem::Variable(variable) => {
                    self.local_variables
                        .insert(variable.span.start, variable.clone());
                }
                FunctionBodyItem::Function(function) => {
                    self.local_functions
                        .insert(function.span.start, (**function).clone());
                }
                FunctionBodyItem::If(statement) => {
                    self.register_body(&statement.consequent);
                    match &statement.alternate {
                        Some(FunctionElseBranch::Braced(items)) => self.register_body(items),
                        Some(FunctionElseBranch::ElseIf(statement)) => self.register_if(statement),
                        None => {}
                    }
                }
                FunctionBodyItem::While(statement) => self.register_body(&statement.body),
                FunctionBodyItem::Try(statement) => {
                    self.register_body(&statement.block);
                    if let Some(handler) = &statement.handler {
                        self.register_body(&handler.body);
                    }
                    if let Some(items) = &statement.finalizer {
                        self.register_body(items);
                    }
                }
                _ => {}
            }
        }
    }

    fn register_if(&mut self, statement: &FunctionIfStatement) {
        self.register_body(&statement.consequent);
        match &statement.alternate {
            Some(FunctionElseBranch::Braced(items)) => self.register_body(items),
            Some(FunctionElseBranch::ElseIf(statement)) => self.register_if(statement),
            None => {}
        }
    }

    pub(super) fn class(
        &mut self,
        class: &ClassDeclaration,
        parent: ScopeId,
        check_heritage: bool,
    ) {
        for decorator in &class.decorators {
            self.expression(&decorator.tokens, parent);
        }
        if let (true, Some(name), Some(span)) =
            (check_heritage, &class.extends_name, &class.extends_span)
        {
            self.references.push(Reference {
                scope: parent,
                name: name.clone(),
                meaning: Meaning::Value,
                span: span.clone(),
            });
        }
        let scope = self.child(Some(parent), class.body_span.clone(), false, false);
        self.private_class(class, scope);
        let constructor = self.scopes[parent]
            .values
            .get(&class.name)
            .map(|binding| binding.declared_type.clone())
            .unwrap_or(Type::Unknown);
        self.value(scope, &class.name, 0, true, false, constructor);
        self.binding_kind(scope, &class.name, BindingKind::Class);
        self.class_this(scope, &class.name, true);
        for member in &class.members {
            for decorator in &member.decorators {
                self.expression(&decorator.tokens, parent);
            }
            if let Some(field) = &member.field {
                if let Some(initializer) = &field.initializer {
                    let initializer_scope = if field.is_static {
                        scope
                    } else {
                        self.child(Some(scope), field.span.clone(), false, true)
                    };
                    if !field.is_static {
                        self.class_this(initializer_scope, &class.name, false);
                    }
                    self.expression(initializer, initializer_scope);
                }
            }
            if let Some(constructor) = &member.constructor {
                let inner = self.child(Some(scope), constructor.span.clone(), true, true);
                self.class_this(inner, &class.name, false);
                let mut fields: BTreeSet<String> = class
                    .members
                    .iter()
                    .filter_map(|member| member.field.as_ref())
                    .filter(|field| field.readonly && !field.is_static)
                    .map(|field| field.name.clone())
                    .collect();
                fields.extend(
                    constructor
                        .parameter_properties
                        .iter()
                        .filter(|property| property.readonly)
                        .map(|property| {
                            constructor.parameters[property.parameter_index]
                                .name
                                .clone()
                        }),
                );
                self.constructor_fields.insert(inner, fields);
                self.value(inner, "arguments", 0, false, false, Type::Unknown);
                self.parameters(&constructor.parameters, inner);
                if let Some(items) = &constructor.body {
                    self.function_body(items, inner, &constructor.span);
                }
            }
            if let Some(method) = &member.method {
                let inner = self.child(Some(scope), method.span.clone(), true, true);
                self.class_this(inner, &class.name, method.is_static);
                self.value(inner, "arguments", 0, false, false, Type::Unknown);
                self.parameters(&method.parameters, inner);
                if let Some(items) = &method.body {
                    self.function_body(items, inner, &method.span);
                }
            }
            if let Some(accessor) = &member.accessor {
                let inner = self.child(Some(scope), accessor.span.clone(), true, true);
                self.class_this(inner, &class.name, accessor.is_static);
                self.value(inner, "arguments", 0, false, false, Type::Unknown);
                self.parameters(&accessor.parameters, inner);
                self.function_body(&accessor.body, inner, &accessor.span);
            }
            if let Some(block) = &member.static_block {
                let inner = self.child(Some(scope), block.span.clone(), true, false);
                self.class_this(inner, &class.name, true);
                self.body(&block.body, inner);
            }
        }
    }

    pub(super) fn type_scopes(&mut self, value: &Type, span: &SourceSpan, scope: ScopeId) {
        match value {
            Type::Function { parameters, result } => {
                let start = parameters
                    .first()
                    .map_or(span.start, |parameter| parameter.span.start);
                let end = self
                    .module
                    .edits
                    .iter()
                    .filter(|edit| {
                        edit.replacement.is_empty() && edit.start <= start && start < edit.end
                    })
                    .map(|edit| edit.end)
                    .min()
                    .unwrap_or(span.end)
                    .min(span.end);
                let inner = self.child(
                    Some(scope),
                    SourceSpan::new(&self.module.id, start, end),
                    false,
                    false,
                );
                for parameter in parameters {
                    self.value(
                        inner,
                        &parameter.name,
                        0,
                        false,
                        false,
                        parameter.annotation.clone().unwrap_or(Type::Unknown),
                    );
                    if let Some(annotation) = &parameter.annotation {
                        self.type_scopes(annotation, &parameter.span, inner);
                    }
                }
                self.type_scopes(result, span, inner);
            }
            Type::Record(fields) => {
                for field in fields {
                    self.type_scopes(&field.value, &field.span, scope);
                }
            }
            Type::Array(element) => self.type_scopes(element, span, scope),
            Type::Tuple(elements) => {
                for element in elements {
                    self.type_scopes(element.annotation(), span, scope);
                }
            }
            Type::Union(options) | Type::Intersection(options) => {
                for option in options {
                    self.type_scopes(option, span, scope);
                }
            }
            Type::Named { arguments, .. } => {
                for argument in arguments {
                    self.type_scopes(argument, span, scope);
                }
            }
            _ => {}
        }
    }

    pub(super) fn tokens_in(&self, span: &SourceSpan) -> Vec<Token> {
        let start = self
            .tokens
            .partition_point(|token| token.start < span.start);
        let end = self.tokens.partition_point(|token| token.start < span.end);
        self.tokens[start..end].to_vec()
    }
}

pub(super) fn function_type(parameters: &[Parameter], result: &Option<Type>) -> Type {
    Type::Function {
        parameters: parameters.to_vec(),
        result: Box::new(result.clone().unwrap_or(Type::Unknown)),
    }
}

fn body_item_span(item: &FunctionBodyItem) -> &SourceSpan {
    match item {
        FunctionBodyItem::Variable(item) => &item.span,
        FunctionBodyItem::Function(item) => &item.span,
        FunctionBodyItem::If(item) => &item.span,
        FunctionBodyItem::While(item) => &item.span,
        FunctionBodyItem::Try(item) => &item.span,
        FunctionBodyItem::Expression { span, .. }
        | FunctionBodyItem::Return { span, .. }
        | FunctionBodyItem::Throw { span, .. }
        | FunctionBodyItem::Opaque(span) => span,
    }
}

/// Declaration merging can give an interface and a value the same exported
/// name. Resolve the value meaning from the sealed source graph, without
/// assuming every ordinary import is a runtime binding.
fn exported_value(declarations: &[Declaration], name: &str) -> Option<bool> {
    let mut result = None;
    for declaration in declarations {
        let (local, exported, has_value) = match declaration {
            Declaration::Variable(item) => (&item.name, item.exported, true),
            Declaration::Function(item) => (&item.name, item.exported, true),
            Declaration::Class(item) => (&item.name, item.exported, true),
            Declaration::Enum(item) => (&item.name, item.exported, true),
            Declaration::Namespace(item) => {
                (&item.name, item.exported, namespace_has_value(&item.body))
            }
            Declaration::Interface(item) => (&item.name, item.exported, false),
            Declaration::TypeAlias(item) => (&item.name, item.exported, false),
            _ => continue,
        };
        if exported && local == name {
            result = Some(result.unwrap_or(false) || has_value);
        }
    }
    result
}

fn namespace_has_value(body: &[Declaration]) -> bool {
    body.iter().any(|item| match item {
        Declaration::Variable(_)
        | Declaration::Function(_)
        | Declaration::Class(_)
        | Declaration::Enum(_)
        | Declaration::Raw(_) => true,
        Declaration::Namespace(item) => namespace_has_value(&item.body),
        _ => false,
    })
}

/// Cyclic bases already get the checker's cycle diagnostic. Textual-order
/// errors for their heritage would be cascades, rather than independent uses.
fn cyclic_classes(declarations: &[Declaration]) -> BTreeSet<&str> {
    let graph: BTreeMap<_, _> = declarations
        .iter()
        .filter_map(|declaration| {
            if let Declaration::Class(class) = declaration {
                Some((class.name.as_str(), class.extends_name.as_deref()))
            } else {
                None
            }
        })
        .collect();
    let mut done = BTreeSet::new();
    let mut cyclic = BTreeSet::new();
    for origin in graph.keys() {
        let mut current = *origin;
        let mut path = Vec::new();
        let mut positions = BTreeMap::new();
        while !done.contains(current) {
            if let Some(position) = positions.get(current) {
                cyclic.extend(path[*position..].iter().copied());
                break;
            }
            positions.insert(current, path.len());
            path.push(current);
            let Some(Some(base)) = graph.get(current) else {
                break;
            };
            current = base;
        }
        done.extend(path);
    }
    cyclic
}
