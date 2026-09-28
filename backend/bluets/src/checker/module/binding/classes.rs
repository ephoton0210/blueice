// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

//! Bounded class-method checks before class runtime admission.

use super::*;
use crate::parser::{ClassDeclaration, ClassMethod};

impl ModuleChecker<'_> {
    pub(in crate::checker::module) fn is_local_class_constructor_value(
        &self,
        name: &str,
        scope: &BTreeMap<String, Type>,
    ) -> bool {
        self.class_constructors.contains_key(name)
            && scope
                .get(name)
                .is_some_and(|value| self.values.get(name) == Some(value))
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

    pub(super) fn validate_class_method_groups(&mut self, class: &ClassDeclaration) {
        let mut implementations: BTreeMap<&str, usize> = BTreeMap::new();
        for group in &class.method_groups {
            if group.implementation_member_index.is_some() {
                *implementations.entry(&group.name).or_default() += 1;
            }
        }

        for group in &class.method_groups {
            if implementations
                .get(group.name.as_str())
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
        if !self.is_local_class_constructor_value(&call.callee.text, scope) {
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
                || !self.is_local_class_constructor_value(&tokens[start + 1].text, scope)
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

    pub(in crate::checker::module) fn check_local_class_calls_in_expression(
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
                || !self.is_local_class_constructor_value(&callee.text, scope)
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

fn class_instance_type(class: &ClassDeclaration) -> Type {
    let mut fields = Vec::new();
    for group in &class.method_groups {
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
    Type::Record(fields)
}

fn class_constructor_side_type(class: &ClassDeclaration) -> Type {
    Type::Record(vec![TypeField {
        name: "prototype".to_string(),
        readonly: true,
        optional: false,
        value: Type::Named {
            name: class.name.clone(),
            arguments: Vec::new(),
        },
        span: class.name_span.clone(),
    }])
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
