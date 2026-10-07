// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

//! Lexical lookup, named types and generic declaration validation.

use super::*;
mod compatibility;
mod more_types;
mod operators;

impl ModuleChecker<'_> {
    pub(crate) fn check_names(&mut self) {
        let scopes = scopes::ScopeModel::new(
            self.project,
            self.module,
            &self.values,
            &self.types,
            self.ambient,
            self.target,
            self.max_type_expansions,
        );
        self.diagnostics.extend(scopes.diagnostics());
        if self.explicit_checking {
            self.diagnostics
                .extend(scopes.checking_diagnostics(self.checking));
        }
        for (name, value) in scopes.query_types() {
            self.types.insert(
                name,
                TypeDefinition {
                    kind: TypeDefinitionKind::Alias,
                    parameters: Vec::new(),
                    value,
                },
            );
        }
        self.scopes = Some(scopes);
        let flow = scopes::flow::FlowModel::build(
            self.scopes.as_ref().expect("lexical scopes are bound"),
            |tokens, values, expected| self.infer_in_context(tokens, values, expected),
        );
        self.diagnostics.extend(flow.diagnostics.iter().cloned());
        self.flow = Some(flow);
    }

    pub(crate) fn dedupe_name_diagnostics(&mut self) {
        let mut seen = BTreeSet::new();
        self.diagnostics.reverse();
        self.diagnostics.retain(|diagnostic| {
            !matches!(
                diagnostic.code,
                DiagnosticCode::UnknownName
                    | DiagnosticCode::UnknownType
                    | DiagnosticCode::UsedBeforeDeclaration
            ) || seen.insert((
                diagnostic.code.to_string(),
                diagnostic.span.module.clone(),
                diagnostic.span.start,
                diagnostic.span.end,
            ))
        });
        self.diagnostics.reverse();
    }

    /// Avoid cascaded assignability errors after lexical lookup has already
    /// diagnosed an unresolved annotation at its identifier token.
    pub(in crate::checker::module) fn type_is_bound(&self, value: &Type) -> bool {
        match value {
            Type::KeyOf(value) => self.type_is_bound(value),
            Type::IndexedAccess { object, index, .. } => {
                self.type_is_bound(object) && self.type_is_bound(index)
            }
            Type::Predicate(predicate) => predicate.return_position,
            Type::Named { name, arguments } => {
                (self.types.contains_key(name) || self.type_parameters.contains(name))
                    && arguments
                        .iter()
                        .all(|argument| self.type_is_bound(argument))
            }
            Type::Array(value) => self.type_is_bound(value),
            Type::Tuple(elements) => elements
                .iter()
                .all(|element| self.type_is_bound(element.annotation())),
            Type::Union(options) | Type::Intersection(options) => {
                options.iter().all(|option| self.type_is_bound(option))
            }
            Type::CallableRecord { fields, signatures } => {
                fields.iter().all(|field| self.type_is_bound(&field.value))
                    && signatures
                        .iter()
                        .all(|signature| self.type_is_bound(&signature.function_type()))
            }
            Type::Record(fields) => fields.iter().all(|field| self.type_is_bound(&field.value)),
            Type::Function { parameters, result } => {
                parameters.iter().all(|parameter| {
                    parameter
                        .annotation
                        .as_ref()
                        .is_none_or(|annotation| self.type_is_bound(annotation))
                }) && self.type_is_bound(result)
            }
            _ => true,
        }
    }

    pub(in crate::checker::module) fn check_type(&mut self, value: &Type, span: &SourceSpan) {
        self.check_additional_type(value, span);
        self.check_operator(value);
        if let Some(children) = value.operator_children() {
            let previous = self.type_parameters.clone();
            if let Type::Mapped(value) = value {
                self.type_parameters.insert(value.parameter.name.clone());
            }
            if let Type::Conditional(value) = value {
                self.type_parameters
                    .extend(value.extends.infer_parameters().into_iter().map(|p| p.name));
            }
            for child in children {
                self.check_type(child, span);
            }
            self.type_parameters = previous;
            return;
        }
        match value {
            Type::KeyOf(value) => self.check_type(value, span),
            Type::IndexedAccess { object, index, .. } => {
                self.check_type(object, span);
                self.check_type(index, span);
            }
            Type::Predicate(predicate) => self.check_predicate_position(predicate),
            Type::Named { name, arguments } => {
                if let Some(query) = name.strip_prefix("typeof ") {
                    if let Some((value_name, position)) = query
                        .rsplit_once('@')
                        .filter(|_| !self.types.contains_key(name))
                    {
                        if let Ok(position) = position.parse::<usize>() {
                            let value = self
                                .scopes
                                .as_ref()
                                .and_then(|scopes| scopes.query_type(value_name, position))
                                .or_else(|| self.values.get(value_name).cloned());
                            if let Some(value) = value {
                                self.types.insert(
                                    name.clone(),
                                    TypeDefinition {
                                        kind: TypeDefinitionKind::Alias,
                                        parameters: Vec::new(),
                                        value,
                                    },
                                );
                            }
                        }
                    }
                }
                if self.refuse_hidden_namespace_type(name, span) {
                    return;
                }
                if let Some(definition) = self.types.get(name).cloned() {
                    self.check_type_arguments(name, arguments, &definition, span);
                }
            }
            Type::Array(value) => self.check_type(value, span),
            Type::Tuple(values) => {
                for value in values {
                    self.check_type(&value.annotation, span);
                }
                if values
                    .iter()
                    .any(|element| element.rest && matches!(element.annotation, Type::Named { .. }))
                {
                    let specialized = substitute_type(
                        &Type::Tuple(values.clone()),
                        &self.allowed_tuple_spread_parameters,
                    );
                    let Type::Tuple(specialized) = specialized else {
                        unreachable!("tuple substitution retains a tuple")
                    };
                    let mut budget = TypeExpansionBudget::new(self.max_type_expansions);
                    if let Err(error) = expand_concrete_tuple_spreads(
                        &specialized,
                        &self.types,
                        &mut HashSet::new(),
                        &mut budget,
                    ) {
                        let (code, message) = match error {
                            TupleSpreadError::Exhausted => (
                                DiagnosticCode::ResourceLimit,
                                "tuple spread exceeds the generic-expansion limit",
                            ),
                            TupleSpreadError::Cyclic => (
                                DiagnosticCode::UnsupportedSyntax,
                                "cyclic tuple spread cannot be resolved",
                            ),
                            TupleSpreadError::Unresolved => (
                                DiagnosticCode::UnsupportedSyntax,
                                "tuple spread names an unresolved type",
                            ),
                            TupleSpreadError::Unsupported => (
                                DiagnosticCode::UnsupportedSyntax,
                                "tuple spread requires one concrete tuple or array type",
                            ),
                        };
                        self.type_error(span, message.to_string(), code);
                    }
                }
            }
            Type::Union(values) | Type::Intersection(values) => {
                for value in values {
                    self.check_type(value, span);
                }
            }
            Type::CallableRecord { fields, signatures } => {
                self.check_type(&Type::Record(fields.clone()), span);
                for signature in signatures {
                    self.check_type(&signature.function_type(), &signature.span);
                }
            }
            Type::Record(fields) => {
                for field in fields {
                    self.check_type(&field.value, &field.span);
                }
            }
            Type::GenericFunction {
                type_parameters,
                parameters,
                result,
                ..
            } => {
                let previous = self.type_parameters.clone();
                self.check_type_parameters(type_parameters);
                self.check_type(
                    &Type::Function {
                        parameters: parameters.clone(),
                        result: result.clone(),
                    },
                    span,
                );
                self.type_parameters = previous;
            }
            Type::Function { parameters, result } => {
                for parameter in parameters {
                    if let Some(annotation) = &parameter.annotation {
                        self.check_type(annotation, &parameter.span);
                    }
                }
                self.check_type(result, span);
                self.check_predicate_signature(result, parameters);
            }
            _ => {}
        }
    }

    pub(in crate::checker::module) fn check_type_parameters(
        &mut self,
        parameters: &[TypeParameter],
    ) {
        let mut saw_default = false;
        for parameter in parameters {
            if !self.type_parameters.insert(parameter.name.clone()) {
                self.type_error(
                    &parameter.span,
                    format!("duplicate type parameter `{}`", parameter.name),
                    DiagnosticCode::DuplicateDeclaration,
                );
            }
            if saw_default && parameter.default.is_none() {
                self.type_error(
                    &parameter.span,
                    format!(
                        "required type parameter `{}` cannot follow a defaulted type parameter",
                        parameter.name
                    ),
                    DiagnosticCode::TypeMismatch,
                );
            }
            if parameter.default.is_some() {
                saw_default = true;
            }
            if let Some(constraint) = &parameter.constraint {
                self.check_type(constraint, &parameter.span);
            }
            if let Some(default) = &parameter.default {
                self.check_type(default, &parameter.span);
                if let Some(constraint) = &parameter.constraint {
                    if !self.is_assignable_bounded(default, constraint, &parameter.span) {
                        self.type_error(
                            &parameter.span,
                            format!(
                                "default type `{}` does not satisfy constraint `{}` for `{}`",
                                type_label(default),
                                type_label(constraint),
                                parameter.name,
                            ),
                            DiagnosticCode::TypeMismatch,
                        );
                        self.point_last_type_default(&parameter.span);
                    }
                }
            }
        }
    }

    pub(in crate::checker::module) fn check_type_arguments(
        &mut self,
        name: &str,
        arguments: &[Type],
        definition: &TypeDefinition,
        span: &SourceSpan,
    ) {
        let required = definition
            .parameters
            .iter()
            .filter(|parameter| parameter.default.is_none())
            .count();
        if arguments.len() < required || arguments.len() > definition.parameters.len() {
            let total = definition.parameters.len();
            self.typescript_type_error(
                span,
                format!(
                    "type `{name}` requires {required} to {} type argument(s), got {}",
                    definition.parameters.len(),
                    arguments.len(),
                ),
                DiagnosticCode::TypeMismatch,
                if total == 0 {
                    2315
                } else if required == total {
                    2314
                } else {
                    2707
                },
                vec![name.into(), required.to_string(), total.to_string()],
            );
        }
        for argument in arguments {
            self.check_type(argument, span);
        }
        let Some(arguments) = complete_type_arguments(&definition.parameters, arguments) else {
            return;
        };
        let substitutions = definition
            .parameters
            .iter()
            .map(|parameter| parameter.name.clone())
            .zip(arguments)
            .collect::<BTreeMap<_, _>>();
        for (index, parameter) in definition.parameters.iter().enumerate() {
            let Some(constraint) = &parameter.constraint else {
                continue;
            };
            let actual = substitutions
                .get(&parameter.name)
                .expect("completed generic arguments contain every parameter");
            let expected = substitute_type(constraint, &substitutions);
            let actual_bound = match actual {
                Type::Named { name, arguments } if arguments.is_empty() => self
                    .allowed_tuple_spread_parameters
                    .get(name)
                    .cloned()
                    .unwrap_or_else(|| actual.clone()),
                _ => actual.clone(),
            };
            if !self.is_assignable_bounded(&actual_bound, &expected, span) {
                self.type_error(
                    span,
                    format!(
                        "type argument `{}` does not satisfy constraint `{}` for `{}`",
                        type_label(actual),
                        type_label(&expected),
                        parameter.name,
                    ),
                    DiagnosticCode::TypeMismatch,
                );
                self.point_last_type_argument(span, Some(index));
            }
        }
    }

    pub(in crate::checker::module) fn type_error(
        &mut self,
        span: &SourceSpan,
        message: String,
        code: DiagnosticCode,
    ) {
        if self.enforce_types {
            self.diagnostics
                .push(Diagnostic::error(code, span.clone(), message));
        }
    }

    pub(in crate::checker::module) fn typescript_type_error(
        &mut self,
        span: &SourceSpan,
        message: String,
        code: DiagnosticCode,
        typescript_code: u32,
        arguments: Vec<String>,
    ) {
        if self.enforce_types {
            self.diagnostics.push(
                Diagnostic::error(code, span.clone(), message)
                    .with_typescript(typescript_code, arguments),
            );
        }
    }

    pub(in crate::checker::module) fn blue_only_type_error(
        &mut self,
        span: &SourceSpan,
        message: String,
        code: DiagnosticCode,
        reason: &'static str,
    ) {
        if self.enforce_types {
            self.diagnostics
                .push(Diagnostic::error(code, span.clone(), message).blue_only(reason));
        }
    }
}
