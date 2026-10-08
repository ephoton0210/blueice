// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

//! Structural assignment reasons keep the original public type names.
use super::*;
use crate::parser::Variance;

impl ModuleChecker<'_> {
    pub(super) fn compatibility_shape(&self, value: &Type) -> Type {
        let mut value = value.clone();
        let mut visited = HashSet::new();
        let mut budget = TypeExpansionBudget::new(self.max_type_expansions);
        while let Some(next) = instantiate_named(
            &value,
            &self.types,
            &mut visited,
            &mut budget,
            "compatibility reason",
        ) {
            value = next;
        }
        value
    }

    pub(super) fn compatibility_assignment_error(
        &mut self,
        span: &SourceSpan,
        message: &str,
        code: DiagnosticCode,
        actual: &Type,
        expected: &Type,
    ) -> bool {
        if let Type::Named { name, .. } = expected {
            if self.bound_parameters.contains_key(name) {
                self.typescript_type_error(
                    span,
                    message.into(),
                    code,
                    2322,
                    vec![
                        crate::diagnostic::type_text::render_in(actual, self.project),
                        name.clone(),
                    ],
                );
                self.explain_last_type_pair(actual, expected);
                return true;
            }
        }
        if self.weak_compatibility_error(span, message, code, actual, expected) {
            if let Ok(tokens) = crate::syntax::lex(&self.module.id, &self.module.source) {
                let tokens = tokens
                    .iter()
                    .filter(|token| span.start <= token.start && token.end <= span.end)
                    .filter(|token| !token.is("export") && !token.is("declare"))
                    .collect::<Vec<_>>();
                if tokens
                    .first()
                    .is_some_and(|token| token.is("const") || token.is("let") || token.is("var"))
                {
                    if let Some(name) = tokens.get(1) {
                        self.point_last_typescript(std::slice::from_ref(*name));
                    }
                }
            }
            return true;
        }
        let a = self.compatibility_shape(actual);
        let e = self.compatibility_shape(expected);
        if let (
            Type::IndexedRecord {
                object: ao,
                indices: ai,
            },
            Type::IndexedRecord {
                object: eo,
                indices: ei,
            },
        ) = (&a, &e)
        {
            if let (Type::Record(af), Type::Record(ef)) = (ao.as_ref(), eo.as_ref()) {
                if let Some(field) = ef
                    .iter()
                    .find(|field| !field.optional && !af.iter().any(|a| a.name == field.name))
                {
                    self.typescript_type_error(
                        span,
                        message.into(),
                        code,
                        2741,
                        vec![
                            field.name.clone(),
                            crate::diagnostic::type_text::render_in(actual, self.project),
                            crate::diagnostic::type_text::render_in(expected, self.project),
                        ],
                    );
                    return true;
                }
            }
            for target in ei {
                if let Some(source) = ai.iter().find(|source| source.key == target.key) {
                    let mut budget = TypeExpansionBudget::new(self.max_type_expansions);
                    budget.checking = self.checking;
                    if !is_assignable(
                        &source.value,
                        &target.value,
                        &self.types,
                        &mut HashSet::new(),
                        &mut budget,
                    ) {
                        self.typescript_type_error(
                            span,
                            message.into(),
                            code,
                            2322,
                            vec![
                                crate::diagnostic::type_text::render_in(actual, self.project),
                                crate::diagnostic::type_text::render_in(expected, self.project),
                            ],
                        );
                        let detail = format!(
                            "\n  '{}' index signatures are incompatible.{}",
                            crate::diagnostic::type_text::render(&target.key),
                            pair_detail(&source.value, &target.value, self.project)
                        );
                        if let Some(d) = self
                            .diagnostics
                            .last_mut()
                            .and_then(|d| d.typescript.as_mut())
                        {
                            d.message.push_str(&detail);
                        }
                        return true;
                    }
                }
            }
        }
        false
    }

    pub(in crate::checker::module) fn weak_compatibility_error(
        &mut self,
        span: &SourceSpan,
        message: &str,
        code: DiagnosticCode,
        actual: &Type,
        expected: &Type,
    ) -> bool {
        let a = self.compatibility_shape(actual);
        let e = self.compatibility_shape(expected);
        if let (Type::Record(a), Type::Record(e)) = (&a, &e) {
            if !a.is_empty()
                && !e.is_empty()
                && e.iter().all(|field| field.optional)
                && !a
                    .iter()
                    .any(|field| e.iter().any(|target| target.name == field.name))
            {
                self.typescript_type_error(
                    span,
                    message.into(),
                    code,
                    2559,
                    vec![
                        crate::diagnostic::type_text::render_in(actual, self.project),
                        crate::diagnostic::type_text::render_in(expected, self.project),
                    ],
                );
                return true;
            }
        }
        false
    }

    pub(super) fn compatibility_detail(&self, actual: &Type, expected: &Type) -> Option<String> {
        if let Type::Named { name, .. } = expected {
            if let Some(parameter) = self.bound_parameters.get(name) {
                let source = crate::diagnostic::type_text::render_in(actual, self.project);
                return Some(if let Some(constraint) = &parameter.constraint {
                    format!("\n  '{source}' is assignable to the constraint of type '{name}', but '{name}' could be instantiated with a different subtype of constraint '{}'.",crate::diagnostic::type_text::render_in(constraint,self.project))
                } else {
                    format!("\n  '{name}' could be instantiated with an arbitrary type which could be unrelated to '{source}'.")
                });
            }
        }
        if let (
            Type::Named {
                name: a,
                arguments: aa,
            },
            Type::Named {
                name: e,
                arguments: ea,
            },
        ) = (actual, expected)
        {
            if a == e {
                if let Some(definition) = self
                    .types
                    .get(a)
                    .filter(|d| d.parameters.iter().any(|p| p.variance.is_some()))
                {
                    for (parameter, (a, e)) in definition.parameters.iter().zip(aa.iter().zip(ea)) {
                        let mut budget = TypeExpansionBudget::new(self.max_type_expansions);
                        budget.checking = self.checking;
                        let forward =
                            is_assignable(a, e, &self.types, &mut HashSet::new(), &mut budget);
                        let backward =
                            is_assignable(e, a, &self.types, &mut HashSet::new(), &mut budget);
                        let reversed = match parameter.variance {
                            Some(Variance::In) => true,
                            Some(Variance::InOut) => forward && !backward,
                            _ => false,
                        };
                        if !forward || !backward {
                            return Some(if reversed {
                                root_pair_detail(e, a, self.project)
                            } else {
                                root_pair_detail(a, e, self.project)
                            });
                        }
                    }
                }
            }
        }
        let a = self.compatibility_shape(actual);
        let e = self.compatibility_shape(expected);
        if let (
            Type::CallableRecord {
                signatures: actual, ..
            },
            Type::CallableRecord {
                signatures: expected,
                ..
            },
        ) = (&a, &e)
        {
            for target in expected.iter().filter(|signature| signature.construct) {
                if actual
                    .iter()
                    .filter(|signature| signature.construct)
                    .all(|signature| signature.abstract_constructor)
                    && actual.iter().any(|signature| signature.construct)
                    && !target.abstract_constructor
                {
                    return Some("\n  Cannot assign an abstract constructor type to a non-abstract constructor type.".into());
                }
                if let Some(source) = actual.iter().find(|signature| {
                    signature.construct
                        && signature.constructor_visibility != target.constructor_visibility
                }) {
                    return Some(format!(
                        "\n  Cannot assign a '{}' constructor type to a '{}' constructor type.",
                        source.constructor_visibility.keyword(),
                        target.constructor_visibility.keyword()
                    ));
                }
            }
        }
        let (af, ac) = callable_parts(&a);
        let (ef, ec) = callable_parts(&e);
        if matches!(a, Type::CallableRecord { .. }) || matches!(e, Type::CallableRecord { .. }) {
            if let (Some(af), Some(ef)) = (af, ef) {
                let detail = crate::diagnostic::type_text::detail(
                    &Type::Record(af.to_vec()),
                    &Type::Record(ef.to_vec()),
                    self.project,
                );
                if !detail.is_empty() {
                    return Some(detail);
                }
            }
            for target in ec {
                let mut budget = TypeExpansionBudget::new(self.max_type_expansions);
                budget.checking = self.checking;
                if ac.iter().any(|source| {
                    source.0 == target.0
                        && is_assignable(
                            &source.1,
                            &target.1,
                            &self.types,
                            &mut HashSet::new(),
                            &mut budget,
                        )
                }) {
                    continue;
                }
                if let Some(source) = ac.iter().find(|source| source.0 == target.0) {
                    return Some(crate::diagnostic::type_text::detail(
                        &source.1,
                        &target.1,
                        self.project,
                    ));
                }
            }
        }
        None
    }
}

fn callable_parts(value: &Type) -> (Option<&[TypeField]>, Vec<(bool, Type)>) {
    match value {
        Type::CallableRecord { fields, signatures } => (
            Some(fields),
            signatures
                .iter()
                .map(|s| {
                    (
                        s.construct,
                        Type::Function {
                            parameters: s.parameters.clone(),
                            result: Box::new(s.result.clone()),
                        },
                    )
                })
                .collect(),
        ),
        Type::Function { .. } => (None, vec![(false, value.clone())]),
        _ => (None, Vec::new()),
    }
}
fn root_pair_detail(actual: &Type, expected: &Type, project: &crate::Project) -> String {
    format!(
        "\n  Type '{}' is not assignable to type '{}'.{}",
        crate::diagnostic::type_text::render_in(actual, project),
        crate::diagnostic::type_text::render_in(expected, project),
        crate::diagnostic::type_text::detail(actual, expected, project).replace("\n", "\n  ")
    )
}
fn pair_detail(actual: &Type, expected: &Type, project: &crate::Project) -> String {
    root_pair_detail(actual, expected, project).replace("\n", "\n  ")
}
