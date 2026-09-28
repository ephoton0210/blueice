// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

//! Bounded class constructor and method checks before runtime admission.

use super::*;
use crate::parser::{ClassConstructor, ClassDeclaration, ClassMethod};

impl ModuleChecker<'_> {
    pub(in crate::checker::module) fn check_type_only_class_value_uses(
        &mut self,
        tokens: &[Token],
        scope: &BTreeMap<String, Type>,
        span: &SourceSpan,
    ) {
        let mut inspected = 0usize;
        for (index, token) in tokens.iter().enumerate() {
            if token.kind != TokenKind::Identifier
                || !self.type_only_classes.contains(&token.text)
                || scope.contains_key(&token.text)
            {
                continue;
            }
            let previous = index.checked_sub(1).and_then(|index| tokens.get(index));
            let next = tokens.get(index + 1);
            let runtime_use = tokens.len() == 1
                || previous.is_some_and(|previous| previous.is("new"))
                || next.is_some_and(|next| next.is("(") || next.is("."))
                || previous.is_some_and(|previous| {
                    matches!(
                        previous.text.as_str(),
                        "(" | "[" | "{" | "," | "=" | "return" | "throw"
                    )
                }) && !next.is_some_and(|next| next.is(">") || next.is(":"));
            if !runtime_use {
                continue;
            }
            inspected += 1;
            if inspected > self.max_type_expansions {
                self.type_error(
                    span,
                    format!(
                        "type-only class value scan exceeds the {} generic-expansion limit",
                        self.max_type_expansions
                    ),
                    DiagnosticCode::ResourceLimit,
                );
                return;
            }
            self.type_error(
                &SourceSpan::new(&span.module, token.start, token.end),
                format!("type-only class {} cannot be used as a value", token.text),
                DiagnosticCode::UnknownName,
            );
        }
    }

    pub(super) fn bind_imported_class(
        &mut self,
        local_name: &str,
        class: &ExportedClass,
        span: &SourceSpan,
    ) {
        if self.types.contains_key(local_name) || self.values.contains_key(local_name) {
            self.duplicate(local_name, span.clone());
            return;
        }
        self.types.insert(
            local_name.to_string(),
            TypeDefinition {
                kind: TypeDefinitionKind::Class,
                parameters: Vec::new(),
                value: specialize_imported_class_type(
                    &class.instance_type,
                    &class.source_name,
                    local_name,
                ),
            },
        );
        self.insert_value(
            local_name,
            specialize_imported_class_type(&class.constructor_type, &class.source_name, local_name),
            span.clone(),
            SymbolKind::Import,
            false,
        );
        let signatures = class
            .constructor_binding
            .signatures
            .iter()
            .map(|signature| {
                let mut signature = signature.clone();
                for parameter in &mut signature.parameters {
                    parameter.annotation = parameter.annotation.as_ref().map(|annotation| {
                        specialize_imported_class_type(annotation, &class.source_name, local_name)
                    });
                }
                signature.return_type = specialize_imported_class_type(
                    &signature.return_type,
                    &class.source_name,
                    local_name,
                );
                signature
            })
            .collect();
        self.class_constructors.insert(
            local_name.to_string(),
            ClassConstructorBinding {
                signatures,
                inherited: class.constructor_binding.inherited,
            },
        );
    }

    pub(in crate::checker::module) fn is_bound_class_constructor_value(
        &self,
        name: &str,
        scope: &BTreeMap<String, Type>,
    ) -> bool {
        self.class_constructors.contains_key(name)
            && scope
                .get(name)
                .is_some_and(|value| self.values.get(name) == Some(value))
    }

    pub(in crate::checker::module) fn is_bound_class_instance_type(&self, value: &Type) -> bool {
        matches!(value, Type::Named { name, .. } if self.types.get(name).is_some_and(|definition| definition.kind == TypeDefinitionKind::Class))
    }

    pub(super) fn bind_class(&mut self, class: &ClassDeclaration) {
        let existing_type = self
            .types
            .get(&class.name)
            .map(|definition| definition.kind);
        if self.values.contains_key(&class.name)
            || matches!(
                existing_type,
                Some(TypeDefinitionKind::Alias | TypeDefinitionKind::Class)
            )
        {
            self.duplicate(&class.name, class.name_span.clone());
            return;
        }

        // TypeScript permits an interface with the same name as a class.
        // Retain the prior interface surface until declaration merging is
        // checked later; class output is refused throughout this phase.
        if existing_type.is_none() {
            self.types.insert(
                class.name.clone(),
                TypeDefinition {
                    kind: TypeDefinitionKind::Class,
                    parameters: Vec::new(),
                    value: class_instance_type(class),
                },
            );
        }
        self.values
            .insert(class.name.clone(), class_constructor_side_type(class));
        self.class_constructors.insert(
            class.name.clone(),
            ClassConstructorBinding {
                signatures: class_constructor_signatures(class),
                inherited: class.extends_name.is_some(),
            },
        );
    }

    pub(super) fn validate_class_constructor_group(&mut self, class: &ClassDeclaration) {
        let constructors = class
            .members
            .iter()
            .enumerate()
            .filter_map(|(index, member)| member.constructor.as_ref().map(|value| (index, value)))
            .collect::<Vec<_>>();
        let implementations = constructors
            .iter()
            .filter(|(_, constructor)| constructor.body.is_some())
            .collect::<Vec<_>>();

        for (_, constructor) in &constructors {
            let mut scope = self.values.clone();
            for (index, parameter) in constructor.parameters.iter().enumerate() {
                if parameter.rest && index + 1 != constructor.parameters.len() {
                    self.type_error(
                        &parameter.span,
                        "a rest parameter must be last".to_string(),
                        DiagnosticCode::TypeMismatch,
                    );
                }
                if parameter.rest && parameter.optional {
                    self.type_error(
                        &parameter.span,
                        "a rest parameter cannot be optional or have a default initializer"
                            .to_string(),
                        DiagnosticCode::TypeMismatch,
                    );
                }
                if parameter.rest
                    && parameter
                        .annotation
                        .as_ref()
                        .is_some_and(|annotation| !matches!(annotation, Type::Array(_)))
                {
                    self.type_error(
                        &parameter.span,
                        "the bounded rest-parameter rule requires an array annotation".to_string(),
                        DiagnosticCode::TypeMismatch,
                    );
                }
                if let Some(annotation) = &parameter.annotation {
                    self.check_type(annotation, &parameter.span);
                }
                if let Some(default) = &parameter.default {
                    if constructor.body.is_none() {
                        self.type_error(
                            &parameter.span,
                            "a constructor overload signature cannot have a default initializer"
                                .to_string(),
                            DiagnosticCode::TypeMismatch,
                        );
                    } else {
                        self.check_direct_runtime_expression(default, &scope, &parameter.span);
                        let actual = self.infer_expression(default, &scope);
                        let expected = parameter.annotation.clone().unwrap_or(Type::Unknown);
                        if !self.is_assignable_bounded(&actual, &expected, &parameter.span) {
                            self.type_error(
                                &parameter.span,
                                format!(
                                    "default initializer has type `{}`, which is not assignable to parameter `{}` of type `{}`",
                                    type_label(&actual),
                                    parameter.name,
                                    type_label(&expected)
                                ),
                                DiagnosticCode::TypeMismatch,
                            );
                        }
                    }
                }
                scope.insert(
                    parameter.name.clone(),
                    parameter.annotation.clone().unwrap_or(Type::Unknown),
                );
            }
        }

        if implementations.len() > 1 {
            for (_, constructor) in implementations {
                self.type_error(
                    &constructor.span,
                    "multiple constructor implementations are not allowed".to_string(),
                    DiagnosticCode::DuplicateDeclaration,
                );
            }
            return;
        }

        let Some(&(implementation_index, implementation)) = implementations.first() else {
            if let Some((_, signature)) = constructors.last() {
                self.type_error(
                    &signature.span,
                    "constructor implementation is missing".to_string(),
                    DiagnosticCode::TypeMismatch,
                );
            }
            return;
        };

        for &(signature_index, signature) in &constructors {
            if signature.body.is_some() {
                continue;
            }
            if signature_index >= *implementation_index
                || class.members[signature_index + 1..*implementation_index]
                    .iter()
                    .any(|member| member.constructor.is_none())
            {
                self.type_error(
                    &signature.span,
                    "constructor overload requires an immediately following implementation"
                        .to_string(),
                    DiagnosticCode::TypeMismatch,
                );
                continue;
            }
            match class_constructor_overload_is_compatible(
                signature,
                implementation,
                &self.types,
                self.max_type_expansions,
            ) {
                Ok(true) => {}
                Ok(false) => self.type_error(
                    &signature.span,
                    "constructor overload is incompatible with its implementation".to_string(),
                    DiagnosticCode::TypeMismatch,
                ),
                Err(()) => self.type_error(
                    &signature.span,
                    format!(
                        "constructor overload compatibility exceeds the {} generic-expansion limit",
                        self.max_type_expansions
                    ),
                    DiagnosticCode::ResourceLimit,
                ),
            }
        }
    }

    pub(super) fn validate_class_method_groups(&mut self, class: &ClassDeclaration) {
        let mut implementations: BTreeMap<(bool, &str), usize> = BTreeMap::new();
        for group in &class.method_groups {
            if group.implementation_member_index.is_some() {
                *implementations
                    .entry((group.is_static, &group.name))
                    .or_default() += 1;
            }
        }

        for group in &class.method_groups {
            if implementations
                .get(&(group.is_static, group.name.as_str()))
                .copied()
                .unwrap_or(0)
                > 1
            {
                for index in group
                    .signature_member_indices
                    .iter()
                    .copied()
                    .chain(group.implementation_member_index)
                {
                    self.type_error(
                        &class.members[index].span,
                        format!("duplicate implementation of class method `{}`", group.name),
                        DiagnosticCode::DuplicateDeclaration,
                    );
                }
                continue;
            }

            let Some(implementation_index) = group.implementation_member_index else {
                if let Some(&last_signature) = group.signature_member_indices.last() {
                    self.type_error(
                        &class.members[last_signature].span,
                        format!(
                            "overload signature for class method `{}` requires an immediately following implementation",
                            group.name
                        ),
                        DiagnosticCode::TypeMismatch,
                    );
                }
                continue;
            };
            let implementation = class.members[implementation_index]
                .method
                .as_ref()
                .expect("method group implementation is parsed");
            for &signature_index in &group.signature_member_indices {
                let signature = class.members[signature_index]
                    .method
                    .as_ref()
                    .expect("method group signature is parsed");
                match class_method_overload_is_compatible(
                    signature,
                    implementation,
                    &self.types,
                    self.max_type_expansions,
                ) {
                    Ok(true) => {}
                    Ok(false) => self.type_error(
                        &signature.span,
                        format!(
                            "overload signature for class method `{}` is incompatible with its implementation",
                            group.name
                        ),
                        DiagnosticCode::TypeMismatch,
                    ),
                    Err(()) => self.type_error(
                        &signature.span,
                        format!(
                            "class method overload compatibility exceeds the {} generic-expansion limit",
                            self.max_type_expansions
                        ),
                        DiagnosticCode::ResourceLimit,
                    ),
                }
            }
        }
    }

    fn check_class_construction(
        &mut self,
        tokens: &[Token],
        scope: &BTreeMap<String, Type>,
        span: &SourceSpan,
    ) {
        let Some(call) = constructor_call_parts(tokens) else {
            return;
        };
        if !self.is_bound_class_constructor_value(&call.callee.text, scope) {
            return;
        }
        let Some(binding) = self.class_constructors.get(&call.callee.text) else {
            return;
        };
        let call_span = SourceSpan::new(
            span.module.clone(),
            tokens.first().expect("constructor call has new").start,
            tokens
                .last()
                .expect("constructor call has closing paren")
                .end,
        );
        if binding.inherited {
            // An omitted derived constructor inherits the parent signature.
            // Heritage resolution and super calls belong to J.3.1.3.4;
            // the class remains unconditionally refused until then.
            return;
        }
        let arguments = split_call_arguments(call.arguments)
            .expect("constructor call has a balanced argument list");
        let Ok(actuals) = self.expanded_call_argument_types(&arguments, scope) else {
            self.type_error(
                &call_span,
                "a class constructor spread must have a fixed-length tuple type".to_string(),
                DiagnosticCode::TypeMismatch,
            );
            return;
        };
        match self.select_function_signature(&binding.signatures, &actuals, None) {
            Ok(Some(_)) => {}
            Ok(None) => self.type_error(
                &call_span,
                format!(
                    "no constructor of class {} accepts the supplied argument types",
                    call.callee.text
                ),
                DiagnosticCode::TypeMismatch,
            ),
            Err(()) => self.type_error(
                &call_span,
                format!(
                    "class constructor selection exceeds the {} generic-expansion limit",
                    self.max_type_expansions
                ),
                DiagnosticCode::ResourceLimit,
            ),
        }
    }

    pub(in crate::checker::module) fn check_class_constructions_in_expression(
        &mut self,
        tokens: &[Token],
        scope: &BTreeMap<String, Type>,
        span: &SourceSpan,
    ) {
        if !tokens.iter().any(|token| token.is("new")) {
            return;
        }
        let mut openings = Vec::new();
        let mut closes = vec![None; tokens.len()];
        for (index, token) in tokens.iter().enumerate() {
            if token.is("(") {
                openings.push(index);
            } else if token.is(")") {
                if let Some(opening) = openings.pop() {
                    closes[opening] = Some(index);
                }
            }
        }
        let mut checked = 0usize;
        for start in 0..tokens.len().saturating_sub(2) {
            if !tokens[start].is("new")
                || tokens[start + 1].kind != TokenKind::Identifier
                || !tokens[start + 2].is("(")
                || !self.is_bound_class_constructor_value(&tokens[start + 1].text, scope)
            {
                continue;
            }
            let Some(end) = closes[start + 2] else {
                continue;
            };
            checked += 1;
            if checked > self.max_type_expansions {
                self.type_error(
                    span,
                    format!(
                        "class construction exceeds the {} generic-expansion limit",
                        self.max_type_expansions
                    ),
                    DiagnosticCode::ResourceLimit,
                );
                return;
            }
            self.check_class_construction(&tokens[start..=end], scope, span);
        }
    }

    pub(in crate::checker::module) fn check_bound_class_calls_in_expression(
        &mut self,
        tokens: &[Token],
        scope: &BTreeMap<String, Type>,
        span: &SourceSpan,
    ) {
        if !tokens.iter().any(|token| token.is("(")) {
            return;
        }
        let mut openings = Vec::new();
        let mut closes = vec![None; tokens.len()];
        for (index, token) in tokens.iter().enumerate() {
            if token.is("(") {
                openings.push(index);
            } else if token.is(")") {
                if let Some(opening) = openings.pop() {
                    closes[opening] = Some(index);
                }
            }
        }
        let mut inspected = 0usize;
        for start in 0..tokens.len().saturating_sub(1) {
            let callee = &tokens[start];
            if callee.kind != TokenKind::Identifier
                || !tokens[start + 1].is("(")
                || start > 0 && (tokens[start - 1].is("new") || tokens[start - 1].is("."))
                || !self.is_bound_class_constructor_value(&callee.text, scope)
            {
                continue;
            }
            let Some(end) = closes[start + 1] else {
                continue;
            };
            inspected += 1;
            if inspected > self.max_type_expansions {
                self.type_error(
                    span,
                    format!(
                        "class call scan exceeds the {} generic-expansion limit",
                        self.max_type_expansions
                    ),
                    DiagnosticCode::ResourceLimit,
                );
                return;
            }
            self.type_error(
                &SourceSpan::new(&span.module, callee.start, tokens[end].end),
                format!("class {} cannot be called without `new`", callee.text),
                DiagnosticCode::TypeMismatch,
            );
        }
    }
}

fn class_constructor_signatures(class: &ClassDeclaration) -> Vec<FunctionSignature> {
    let constructors = class
        .members
        .iter()
        .filter_map(|member| member.constructor.as_ref())
        .collect::<Vec<_>>();
    let overloads = constructors
        .iter()
        .copied()
        .filter(|constructor| constructor.body.is_none())
        .collect::<Vec<_>>();
    let selected = if overloads.is_empty() {
        constructors
            .into_iter()
            .filter(|constructor| constructor.body.is_some())
            .collect::<Vec<_>>()
    } else {
        overloads
    };
    if selected.is_empty() {
        return vec![FunctionSignature {
            parameters: Vec::new(),
            type_parameters: Vec::new(),
            return_type: Type::Named {
                name: class.name.clone(),
                arguments: Vec::new(),
            },
        }];
    }
    selected
        .into_iter()
        .map(|constructor| FunctionSignature {
            parameters: constructor.parameters.clone(),
            type_parameters: Vec::new(),
            return_type: Type::Named {
                name: class.name.clone(),
                arguments: Vec::new(),
            },
        })
        .collect()
}

pub(in crate::checker) fn class_export(class: &ClassDeclaration) -> ExportedClass {
    ExportedClass {
        source_name: class.name.clone(),
        instance_type: class_instance_type(class),
        constructor_type: class_constructor_side_type(class),
        constructor_binding: ClassConstructorBinding {
            signatures: class_constructor_signatures(class),
            inherited: class.extends_name.is_some(),
        },
        value_exported: class.exported,
    }
}

pub(in crate::checker) fn class_instance_type(class: &ClassDeclaration) -> Type {
    Type::Record(class_method_fields(class, false))
}

fn class_method_fields(class: &ClassDeclaration, is_static: bool) -> Vec<TypeField> {
    let mut fields = Vec::new();
    for group in &class.method_groups {
        if group.is_static != is_static {
            continue;
        }
        let members = group.signature_member_indices.iter().copied().chain(
            group
                .signature_member_indices
                .is_empty()
                .then_some(group.implementation_member_index)
                .flatten(),
        );
        for index in members {
            let method = class.members[index]
                .method
                .as_ref()
                .expect("method group member is parsed");
            fields.push(TypeField {
                name: method.name.clone(),
                readonly: false,
                optional: false,
                value: Type::Function {
                    parameters: method.parameters.clone(),
                    result: Box::new(method.return_type.clone().unwrap_or(Type::Unknown)),
                },
                span: method.span.clone(),
            });
        }
    }
    fields
}

fn class_constructor_side_type(class: &ClassDeclaration) -> Type {
    let mut fields = vec![TypeField {
        name: "prototype".to_string(),
        readonly: true,
        optional: false,
        value: Type::Named {
            name: class.name.clone(),
            arguments: Vec::new(),
        },
        span: class.name_span.clone(),
    }];
    fields.extend(class_method_fields(class, true));
    Type::Record(fields)
}

fn class_constructor_overload_is_compatible(
    signature: &ClassConstructor,
    implementation: &ClassConstructor,
    aliases: &BTreeMap<String, TypeDefinition>,
    max_type_expansions: usize,
) -> Result<bool, ()> {
    let required_signature = signature
        .parameters
        .iter()
        .filter(|parameter| !parameter.optional)
        .count();
    let required_implementation = implementation
        .parameters
        .iter()
        .filter(|parameter| !parameter.optional)
        .count();
    if required_signature < required_implementation
        || signature.parameters.len() > implementation.parameters.len()
    {
        return Ok(false);
    }

    let mut budget = TypeExpansionBudget::new(max_type_expansions);
    let substitutions = BTreeMap::new();
    for (signature_parameter, implementation_parameter) in
        signature.parameters.iter().zip(&implementation.parameters)
    {
        let actual = parameter_expected_type(signature_parameter, &substitutions);
        let expected = parameter_expected_type(implementation_parameter, &substitutions);
        if !is_assignable(
            &actual,
            &expected,
            aliases,
            &mut HashSet::new(),
            &mut budget,
        ) {
            return if budget.exhausted { Err(()) } else { Ok(false) };
        }
        if budget.exhausted {
            return Err(());
        }
    }
    Ok(true)
}

fn class_method_overload_is_compatible(
    signature: &ClassMethod,
    implementation: &ClassMethod,
    aliases: &BTreeMap<String, TypeDefinition>,
    max_type_expansions: usize,
) -> Result<bool, ()> {
    let required_signature = signature
        .parameters
        .iter()
        .filter(|parameter| !parameter.optional)
        .count();
    let required_implementation = implementation
        .parameters
        .iter()
        .filter(|parameter| !parameter.optional)
        .count();
    if required_signature < required_implementation
        || signature.parameters.len() > implementation.parameters.len()
    {
        return Ok(false);
    }

    let mut budget = TypeExpansionBudget::new(max_type_expansions);
    let substitutions = BTreeMap::new();
    for (signature_parameter, implementation_parameter) in
        signature.parameters.iter().zip(&implementation.parameters)
    {
        let actual = parameter_expected_type(signature_parameter, &substitutions);
        let expected = parameter_expected_type(implementation_parameter, &substitutions);
        if !is_assignable(
            &actual,
            &expected,
            aliases,
            &mut HashSet::new(),
            &mut budget,
        ) {
            return if budget.exhausted { Err(()) } else { Ok(false) };
        }
        if budget.exhausted {
            return Err(());
        }
    }

    let actual = signature.return_type.as_ref().unwrap_or(&Type::Unknown);
    let expected = implementation
        .return_type
        .as_ref()
        .unwrap_or(&Type::Unknown);
    let compatible = is_assignable(actual, expected, aliases, &mut HashSet::new(), &mut budget);
    if budget.exhausted {
        Err(())
    } else {
        Ok(compatible)
    }
}
