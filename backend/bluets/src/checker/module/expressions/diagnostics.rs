// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

//! Diagnostic causes determined from the checked call signatures.

use super::*;
mod callable_types;
mod compatibility;
mod constructors;
mod indexed;
mod inference_candidates;
mod key_sets;
mod more_types;
mod operators;
mod records;
mod string_mapping;
mod structural;

fn diagnostic_expected(actual: &Type, expected: &Type) -> Type {
    if !matches!(actual, Type::Null | Type::Undefined) {
        if let Type::Union(values) = expected {
            let values = values
                .iter()
                .filter(|value| !matches!(value, Type::Null | Type::Undefined))
                .cloned()
                .collect::<Vec<_>>();
            let value = crate::checker::type_operators::union(values);
            if !matches!(value, Type::Union(_) | Type::Never) {
                return value;
            }
        }
    }
    expected.clone()
}

impl ModuleChecker<'_> {
    pub(in crate::checker::module) fn related_contextual_property(
        &mut self,
        field: &TypeField,
        parent: &Type,
    ) {
        if !self.project.modules.contains_key(&field.span.module) {
            return;
        }
        let class = matches!(parent, Type::Named {name, ..} if self.types.get(name).is_some_and(|definition| definition.kind == TypeDefinitionKind::Class));
        let origin = if class {
            fn modifiers_start(declarations: &[Declaration], field: &TypeField) -> Option<usize> {
                for declaration in declarations {
                    match declaration {
                        Declaration::Class(class) => {
                            for member in &class.members {
                                if let Some(constructor) = &member.constructor {
                                    for property in &constructor.parameter_properties {
                                        let parameter =
                                            &constructor.parameters[property.parameter_index];
                                        if parameter.name == field.name
                                            && parameter.span == field.span
                                        {
                                            return Some(property.modifiers_start);
                                        }
                                    }
                                }
                            }
                        }
                        Declaration::Namespace(namespace) => {
                            if let Some(start) = modifiers_start(&namespace.body, field) {
                                return Some(start);
                            }
                        }
                        Declaration::Ambient(ambient) => {
                            if let Some(start) = modifiers_start(&ambient.body, field) {
                                return Some(start);
                            }
                        }
                        _ => {}
                    }
                }
                None
            }
            let source = &self.project.modules[&field.span.module];
            let start = modifiers_start(&source.declarations, field).unwrap_or(field.span.start);
            SourceSpan::new(&field.span.module, start, field.span.end)
        } else {
            // JSDoc fields retain a complete tag span; their property name is
            // inside a comment rather than a source token.
            self.project
                .source(&field.span.module)
                .and_then(|source| crate::syntax::lex(&field.span.module, source).ok())
                .and_then(|tokens| {
                    tokens.into_iter().find(|token| {
                        token.start >= field.span.start
                            && token.end <= field.span.end
                            && token.is(&field.name)
                    })
                })
                .map(|token| token.span(&field.span.module))
                .unwrap_or_else(|| field.span.clone())
        };
        let related = crate::TypeScriptRelatedInformation {
            code: 6500,
            message: format!(
                "The expected type comes from property '{}' which is declared here on type '{}'",
                field.name,
                crate::diagnostic::type_text::render_in(parent, self.project)
            ),
            span: origin,
            position: None,
        };
        if let Some(counterpart) = self
            .diagnostics
            .last_mut()
            .and_then(|diagnostic| diagnostic.typescript.as_mut())
        {
            counterpart.related_information.push(related);
        }
    }

    pub(in crate::checker::module) fn call_argument_error(
        &mut self,
        span: &SourceSpan,
        message: String,
        actual: &Type,
        expected: &Type,
        argument: &[Token],
        overloaded: bool,
    ) {
        if let Some((source, target)) = self.key_diagnostic_types(actual, expected) {
            if source != *actual || target != *expected {
                self.call_argument_error(span, message, &source, &target, argument, overloaded);
                return;
            }
        }
        let mut budget = TypeExpansionBudget::new(self.max_type_expansions);
        budget.checking = self.checking;
        // Inferred scalar barriers reduce, while object barriers retain their
        // public name in related-property diagnostics. Lookup expands the
        // underlying checking shape separately.
        let checked =
            crate::checker::inference::diagnostic_type(expected, &self.types, &mut budget);
        if checked != *expected && !budget.exhausted {
            self.call_argument_error(span, message, actual, &checked, argument, overloaded);
            return;
        }
        // A closed conditional alias computes its diagnostic shape. Other
        // operators retain their native source spelling (such as keyof Events).
        let resolved = if matches!(expected, Type::Mapped(_))
            || matches!(expected, Type::Named {name, ..}
            if self.types.get(name).is_some_and(|definition| matches!(definition.value, Type::Conditional(_))))
        {
            crate::checker::inference::result_type(
                expected,
                &BTreeMap::new(),
                &self.types,
                &mut budget,
            )
        } else {
            expected.clone()
        };
        let resolved = crate::checker::tuple_compositions::display_type(&resolved, self.checking);
        if resolved != *expected && !budget.exhausted {
            self.call_argument_error(span, message, actual, &resolved, argument, overloaded);
            return;
        }
        if overloaded {
            self.typescript_type_error(
                span,
                message,
                DiagnosticCode::TypeMismatch,
                2769,
                Vec::new(),
            );
            self.point_last_typescript(argument);
            return;
        }
        if self.weak_compatibility_error(
            span,
            &message,
            DiagnosticCode::TypeMismatch,
            actual,
            expected,
        ) {
            self.point_last_typescript(argument);
            return;
        }
        if self.check_array_element_assignment(argument, expected, actual, &self.values.clone()) {
            return;
        }
        if argument.first().is_some_and(|token| token.is("{")) {
            if let (Some(first), Some(last)) = (argument.first(), argument.last()) {
                let point = SourceSpan::new(&self.module.id, first.start, last.end);
                if self.nested_record_assignment_error(
                    &point,
                    &message,
                    DiagnosticCode::TypeMismatch,
                    actual,
                    expected,
                ) {
                    return;
                }
            }
            let (Some(actual_fields), false) = self.expanded_record_fields(actual.clone()) else {
                self.typescript_type_error(
                    span,
                    message,
                    DiagnosticCode::TypeMismatch,
                    2345,
                    vec![
                        crate::diagnostic::type_text::argument(actual, expected),
                        crate::diagnostic::type_text::render_in(expected, self.project),
                    ],
                );
                self.point_last_typescript(argument);
                return;
            };
            if let (Some(expected_fields), false) = self.expanded_record_fields(expected.clone()) {
                for field in actual_fields {
                    let Some(expected_field) = expected_fields
                        .iter()
                        .find(|expected| expected.name == field.name)
                    else {
                        continue;
                    };
                    let mut budget = TypeExpansionBudget::new(self.max_type_expansions);
                    budget.checking = self.checking;
                    if !is_assignable(
                        &field.value,
                        &expected_field.value,
                        &self.types,
                        &mut HashSet::new(),
                        &mut budget,
                    ) && !budget.exhausted
                    {
                        self.typescript_type_error(
                            span,
                            message,
                            DiagnosticCode::TypeMismatch,
                            2322,
                            vec![
                                crate::diagnostic::type_text::render_in(&field.value, self.project),
                                crate::diagnostic::type_text::render_in(
                                    &diagnostic_expected(&field.value, &expected_field.value),
                                    self.project,
                                ),
                            ],
                        );
                        if let Some(token) = argument.iter().find(|token| token.is(&field.name)) {
                            self.point_last_typescript(std::slice::from_ref(token));
                        }
                        self.related_contextual_property(expected_field, expected);
                        return;
                    }
                }
            }
        }
        let displayed_expected = diagnostic_expected(actual, expected);
        self.typescript_type_error(
            span,
            message,
            DiagnosticCode::TypeMismatch,
            2345,
            vec![
                crate::diagnostic::type_text::argument(actual, &displayed_expected),
                crate::diagnostic::type_text::render_in(&displayed_expected, self.project),
            ],
        );
        self.point_last_typescript(argument);
        if argument.first().is_some_and(|token| token.is("function")) {
            self.point_last_typescript(&argument[..1]);
        }
        self.explain_last_type_pair(actual, &displayed_expected);
        if let Type::Function { result, .. } = actual {
            if let Some(body) = argument
                .windows(2)
                .find(|pair| pair[0].is("=>") && pair[1].is("{"))
            {
                if let Some(end) = argument.last().map(|token| token.end) {
                    if let Some(newline) = self
                        .module
                        .source
                        .get(body[1].start..end)
                        .and_then(|text| text.find(['\n', '\r', '\u{2028}', '\u{2029}']))
                    {
                        if let Some(counterpart) = self
                            .diagnostics
                            .last_mut()
                            .and_then(|diagnostic| diagnostic.typescript.as_mut())
                        {
                            counterpart.span.end = body[1].start + newline;
                        }
                    }
                }
            }
            let mut budget = TypeExpansionBudget::new(self.max_type_expansions);
            budget.checking = self.checking;
            if is_assignable(
                result,
                expected,
                &self.types,
                &mut HashSet::new(),
                &mut budget,
            ) && !budget.exhausted
            {
                if let Some(counterpart) = self
                    .diagnostics
                    .last_mut()
                    .and_then(|diagnostic| diagnostic.typescript.as_mut())
                {
                    let hint = Diagnostic::error(
                        DiagnosticCode::TypeMismatch,
                        counterpart.span.clone(),
                        "",
                    )
                    .with_typescript(6212, Vec::new())
                    .typescript
                    .unwrap();
                    counterpart
                        .related_information
                        .push(crate::TypeScriptRelatedInformation {
                            code: hint.code,
                            message: hint.message,
                            span: hint.span,
                            position: None,
                        });
                }
            }
        }
    }

    pub(in crate::checker::module) fn point_last_typescript(&mut self, tokens: &[Token]) {
        if !self.enforce_types {
            return;
        }
        if let (Some(first), Some(last), Some(diagnostic)) =
            (tokens.first(), tokens.last(), self.diagnostics.last_mut())
        {
            if let Some(counterpart) = &mut diagnostic.typescript {
                counterpart.span = SourceSpan::new(&self.module.id, first.start, last.end);
            }
        }
    }

    pub(in crate::checker::module) fn assignment_error(
        &mut self,
        span: &SourceSpan,
        message: String,
        bts_code: DiagnosticCode,
        actual: &Type,
        expected: &Type,
    ) {
        if let Some(normalized) =
            crate::checker::type_relations::normalize_more_type(actual, &self.types)
        {
            self.assignment_error(span, message, bts_code, &normalized, expected);
            return;
        }
        if matches!(actual, Type::Named {name, ..} if name.starts_with("typeof ")) {
            let namespace_query = matches!(actual, Type::Named {name, ..} if name.strip_prefix("typeof ").and_then(|name| name.rsplit_once('@').map(|(name, _)| name).or(Some(name))).is_some_and(|name| self.type_only_namespaces.contains(name)));
            let mut source = actual.clone();
            let mut budget = TypeExpansionBudget::new(self.max_type_expansions);
            let mut seen = HashSet::new();
            while matches!(&source, Type::Named {name, ..} if name.starts_with("typeof ")) {
                let Some(next) = instantiate_named(
                    &source,
                    &self.types,
                    &mut seen,
                    &mut budget,
                    "query diagnostic",
                ) else {
                    break;
                };
                source = next;
            }
            if source != *actual
                && !budget.exhausted
                && (namespace_query
                    || !matches!(source, Type::Function { .. } | Type::GenericFunction { .. }))
            {
                self.assignment_error(span, message, bts_code, &source, expected);
                return;
            }
        }
        if self.constructor_assignment_error(span, &message, bts_code, actual, expected) {
            return;
        }
        if self.compatibility_assignment_error(span, &message, bts_code, actual, expected) {
            return;
        }
        if self.additional_assignment_error(span, &message, bts_code, actual, expected) {
            return;
        }
        if self.operator_assignment_error(span, &message, bts_code, actual, expected) {
            return;
        }
        let (actual_fields, actual_exhausted) = self.expanded_record_fields(actual.clone());
        let literal_initializer =
            self.module
                .source
                .get(span.start..span.end)
                .is_some_and(|source| {
                    source
                        .split_once('=')
                        .is_some_and(|(_, value)| value.trim_start().starts_with('{'))
                });
        let (expected_fields, expected_exhausted) = self.expanded_record_fields(expected.clone());
        if literal_initializer
            && self.nested_record_assignment_error(span, &message, bts_code, actual, expected)
        {
            return;
        }
        if !actual_exhausted && !expected_exhausted {
            if let (Some(actual_fields), Some(expected_fields)) = (actual_fields, expected_fields) {
                let missing = expected_fields
                    .iter()
                    .filter(|field| {
                        !field.optional
                            && !actual_fields.iter().any(|actual| actual.name == field.name)
                    })
                    .collect::<Vec<_>>();
                if let [field] = missing.as_slice() {
                    let name = field.name.rsplit(' ').next().unwrap_or(&field.name);
                    // A public/private mismatch or two private declarations
                    // is an incompatible identity, rather than an absent field.
                    let restricted = field.name.contains(' ') && !name.starts_with('#');
                    let conflicting = actual_fields
                        .iter()
                        .any(|actual| actual.name.rsplit(' ').next() == Some(name));
                    if !restricted && !conflicting {
                        self.typescript_type_error(
                            span,
                            message,
                            bts_code,
                            2741,
                            vec![
                                name.into(),
                                crate::diagnostic::type_text::render_in(actual, self.project),
                                crate::diagnostic::type_text::render_in(expected, self.project),
                            ],
                        );
                        return;
                    }
                }
                if self.checking.exact_optional_property_types
                    && expected_fields.iter().any(|field| {
                        field.optional
                            && actual_fields.iter().any(|actual| {
                                actual.name == field.name
                                    && actual.value == Type::Undefined
                                    && field.value != Type::Undefined
                            })
                    })
                {
                    self.typescript_type_error(
                        span,
                        message,
                        bts_code,
                        2375,
                        vec![
                            crate::diagnostic::type_text::render_in(actual, self.project),
                            crate::diagnostic::type_text::render_in(expected, self.project),
                        ],
                    );
                    self.explain_last_type_pair(
                        &Type::Record(actual_fields.clone()),
                        &Type::Record(expected_fields.clone()),
                    );
                    return;
                }
                for field in &actual_fields {
                    if !literal_initializer {
                        break;
                    }
                    let Some(expected_field) = expected_fields
                        .iter()
                        .find(|expected| expected.name == field.name)
                    else {
                        continue;
                    };
                    let mut budget = TypeExpansionBudget::new(self.max_type_expansions);
                    budget.checking = self.checking;
                    if !is_assignable(
                        &field.value,
                        &expected_field.value,
                        &self.types,
                        &mut HashSet::new(),
                        &mut budget,
                    ) && !budget.exhausted
                    {
                        if self
                            .module
                            .nested_functions
                            .get(&field.span.start)
                            .is_some_and(|function| {
                                function.kind == crate::parser::NestedFunctionKind::Getter
                            })
                        {
                            if let Ok(tokens) =
                                crate::syntax::lex(&self.module.id, &self.module.source)
                            {
                                if let Some(pair) = tokens.windows(2).find(|pair| {
                                    pair[0].start == field.span.start && pair[0].is("get")
                                }) {
                                    let mut field = field.clone();
                                    field.span = pair[1].span(&self.module.id);
                                    let mut origin = expected_field.clone();
                                    if let Some(source) = self.project.source(&origin.span.module) {
                                        if let Ok(tokens) =
                                            crate::syntax::lex(&origin.span.module, source)
                                        {
                                            if let Some(token) = tokens.iter().find(|token| {
                                                token.start >= origin.span.start
                                                    && token.end <= origin.span.end
                                                    && token.is(&origin.name)
                                            }) {
                                                origin.span = token.span(&origin.span.module);
                                            }
                                        }
                                    }
                                    self.asserted_property_error(&field, &origin, expected);
                                    return;
                                }
                            }
                        }
                        self.typescript_type_error(
                            span,
                            message,
                            bts_code,
                            2322,
                            vec![
                                crate::diagnostic::type_text::render_in(&field.value, self.project),
                                crate::diagnostic::type_text::render_in(
                                    &expected_field.value,
                                    self.project,
                                ),
                            ],
                        );
                        if let Ok(tokens) = crate::syntax::lex(&self.module.id, &self.module.source)
                        {
                            let selected = tokens
                                .iter()
                                .filter(|token| token.start >= span.start && token.end <= span.end)
                                .collect::<Vec<_>>();
                            let initializer = selected
                                .iter()
                                .position(|token| token.is("="))
                                .and_then(|index| selected.get(index + 1))
                                .filter(|token| token.is("{"));
                            if let Some(pair) = tokens
                                .windows(2)
                                .filter(|pair| {
                                    initializer.is_some_and(|start| pair[0].start > start.start)
                                        && pair[0].end <= span.end
                                })
                                .rfind(|pair| pair[0].is(&field.name) && pair[1].is(":"))
                            {
                                self.point_last_typescript(&pair[..1]);
                            }
                        }
                        return;
                    }
                }
            }
        }
        let mut budget = TypeExpansionBudget::new(self.max_type_expansions);
        budget.checking = self.checking;
        let display = super::super::super::type_operators::diagnostic_type(
            expected,
            &self.types,
            &mut budget,
        );
        self.typescript_type_error(
            span,
            message,
            bts_code,
            2322,
            vec![
                crate::diagnostic::type_text::assignment_in(actual, &display, self.project),
                crate::diagnostic::type_text::render_in(&display, self.project),
            ],
        );
        self.explain_last_type_pair(actual, &display);
    }

    pub(in crate::checker::module) fn explain_last_type_pair(
        &mut self,
        actual: &Type,
        expected: &Type,
    ) {
        if self.enforce_types {
            let origin = self
                .parameter_constraint_origin(actual, expected)
                .or_else(|| self.callable_parameter_origin(actual, expected));
            let structural = self.nested_structural_reason(actual, expected);
            let missing = structural
                .as_ref()
                .and_then(|reason| reason.related.clone())
                .or_else(|| self.inference_missing_parameter_property(actual, expected));
            let detail = structural
                .map(|reason| reason.message)
                .or_else(|| self.compatibility_detail(actual, expected))
                .unwrap_or_else(|| {
                    let original =
                        crate::diagnostic::type_text::detail(actual, expected, self.project);
                    if !original.is_empty() {
                        original
                    } else {
                        crate::diagnostic::type_text::detail(
                            &self.compatibility_shape(actual),
                            &self.compatibility_shape(expected),
                            self.project,
                        )
                    }
                });
            if let Some(counterpart) = self
                .diagnostics
                .last_mut()
                .and_then(|diagnostic| diagnostic.typescript.as_mut())
            {
                if matches!(counterpart.code, 2322 | 2345 | 2375 | 2719) {
                    counterpart.message.push_str(&detail);
                    if let Some(related) = missing {
                        counterpart.related_information.push(related);
                    }
                    if let Some((name, span)) = origin {
                        let related = crate::TypeScriptRelatedInformation {
                            code: 2208,
                            message: format!(
                                "This type parameter might need an `extends {name}` constraint."
                            ),
                            span,
                            position: None,
                        };
                        counterpart.related_information.push(related);
                    }
                }
            }
            if detail.is_empty() {
                self.explain_missing_constraint_signature(actual, expected);
            }
        }
    }

    pub(in crate::checker::module) fn rejected_call_error(
        &mut self,
        span: &SourceSpan,
        message: String,
        signatures: &[FunctionSignature],
        actuals: &[Type],
        typescript_overloads: bool,
    ) {
        let mut detail = String::new();
        let matching_arity = signatures
            .iter()
            .filter(|signature| function_signature_accepts_argument_count(signature, actuals.len()))
            .collect::<Vec<_>>();
        let (code, arguments) = if matching_arity.is_empty() && !signatures.is_empty() {
            let minimum = signatures
                .iter()
                .map(function_signature_required_arguments)
                .min()
                .unwrap();
            let maximum = signatures
                .iter()
                .map(|signature| signature.parameters.len())
                .max()
                .unwrap();
            let expected = if minimum == maximum {
                minimum.to_string()
            } else {
                format!("{minimum}-{maximum}")
            };
            (2554, vec![expected, actuals.len().to_string()])
        } else if matching_arity.len() > 1 || typescript_overloads && matching_arity.len() != 1 {
            detail = self.overload_details(signatures, actuals);
            (2769, Vec::new())
        } else if let Some(signature) = matching_arity.first() {
            let mut inference_budget = TypeExpansionBudget::new(self.max_type_expansions);
            inference_budget.checking = self.checking;
            let substitutions =
                infer_call_substitutions(signature, actuals, &self.types, &mut inference_budget);
            let mismatch = actuals.iter().enumerate().find_map(|(index, actual)| {
                let parameter = function_parameter_for_argument(signature, index)?;
                let expected = call_parameter_expected_type(parameter, &substitutions);
                let displayed_expected = if parameter.default.is_some() {
                    match &expected {
                        Type::Union(values) => {
                            let mut values = values
                                .iter()
                                .filter(|value| **value != Type::Undefined)
                                .cloned()
                                .collect::<Vec<_>>();
                            if values.len() == 1 {
                                values.pop().unwrap()
                            } else {
                                Type::Union(values)
                            }
                        }
                        _ => expected.clone(),
                    }
                } else {
                    expected.clone()
                };
                let displayed_expected = if !matches!(actual, Type::Null | Type::Undefined) {
                    if let Type::Union(values) = displayed_expected {
                        crate::checker::type_operators::union(
                            values
                                .into_iter()
                                .filter(|value| !matches!(value, Type::Null | Type::Undefined)),
                        )
                    } else {
                        displayed_expected
                    }
                } else {
                    displayed_expected
                };
                let mut budget = TypeExpansionBudget::new(self.max_type_expansions);
                budget.checking = self.checking;
                (!is_assignable(
                    actual,
                    &expected,
                    &self.types,
                    &mut HashSet::new(),
                    &mut budget,
                ))
                .then(|| {
                    detail = crate::diagnostic::type_text::detail(
                        actual,
                        &displayed_expected,
                        self.project,
                    );
                    vec![
                        crate::diagnostic::type_text::argument(actual, &displayed_expected),
                        crate::diagnostic::type_text::render_in(&displayed_expected, self.project),
                    ]
                })
            });
            (2345, mismatch.unwrap_or_default())
        } else {
            (2345, Vec::new())
        };
        self.typescript_type_error(span, message, DiagnosticCode::TypeMismatch, code, arguments);
        if self.enforce_types {
            if let Some(counterpart) = self
                .diagnostics
                .last_mut()
                .and_then(|diagnostic| diagnostic.typescript.as_mut())
            {
                counterpart.message.push_str(&detail);
            }
        }
    }
}
