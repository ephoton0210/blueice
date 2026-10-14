// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

//! Replacement and context relations use the ordinary bounded type checker.
use super::*;
use crate::checker::decorator_types;
use crate::parser::ClassMemberShell;

impl ModuleChecker<'_> {
    pub(super) fn check_decorator_replacement(
        &mut self,
        decorator: &Decorator,
        target: Target,
        class: &ClassDeclaration,
        member: Option<&ClassMemberShell>,
        result: &Type,
    ) {
        if matches!(target, Target::Parameter) {
            return;
        }
        let Some(expected) =
            decorator_types::replacement_type(class, member, self.experimental_decorators)
        else {
            return;
        };
        let parts = match result {
            Type::Union(parts) => parts.as_slice(),
            other => std::slice::from_ref(other),
        };
        for part in parts {
            if matches!(
                part,
                Type::Any
                    | Type::Unknown
                    | Type::Void
                    | Type::Undefined
                    | Type::Number
                    | Type::String
                    | Type::Boolean
                    | Type::Literal(_)
                    | Type::Null
                    | Type::Array(_)
                    | Type::Tuple(_)
            ) {
                continue;
            }
            let actual = self.decorator_relation_type(part);
            if let Type::Named { name, .. } = &actual {
                if !self.types.contains_key(name) {
                    continue;
                }
            }
            let checked_expected = if member.is_none() {
                self.values
                    .get(&class.name)
                    .cloned()
                    .unwrap_or(expected.clone())
            } else {
                expected.clone()
            };
            if !self.decorator_types_assignable(&actual, &checked_expected, &decorator.span) {
                self.typescript_type_error(
                    &decorator.span,
                    format!(
                        "decorator replacement type `{}` is incompatible with the decorated {}",
                        type_label(part),
                        target.label()
                    ),
                    DiagnosticCode::TypeMismatch,
                    1270,
                    vec![
                        crate::diagnostic::type_text::render_in(result, self.project),
                        format!(
                            "void | {}",
                            crate::diagnostic::type_text::render_in(&expected, self.project)
                        ),
                    ],
                );
                break;
            }
        }
    }

    pub(super) fn check_decorator_context(
        &mut self,
        decorator: &Decorator,
        target: Target,
        class: &ClassDeclaration,
        member: Option<&ClassMemberShell>,
        parameters: &[Parameter],
    ) {
        let Some(expected) = parameters.get(1).and_then(|p| p.annotation.as_ref()) else {
            return;
        };
        let Some(actual) = decorator_types::context_type(class, member) else {
            return;
        };
        if let Some(value) = self.values.get(&class.name).cloned() {
            self.types
                .entry(format!("typeof {}", class.name))
                .or_insert(TypeDefinition {
                    kind: TypeDefinitionKind::Alias,
                    parameters: Vec::new(),
                    value,
                });
        }
        if !self.decorator_types_assignable(&actual, expected, &decorator.span) {
            self.typescript_type_error(
                &decorator.span,
                format!(
                    "decorator context is incompatible with the decorated {}",
                    target.label()
                ),
                DiagnosticCode::TypeMismatch,
                match target {
                    Target::Class => 1238,
                    Target::Field | Target::Accessor => 1240,
                    _ => 1241,
                },
                vec![
                    crate::diagnostic::type_text::render_in(&actual, self.project),
                    crate::diagnostic::type_text::render_in(expected, self.project),
                ],
            );
        }
    }

    fn decorator_relation_type(&self, value: &Type) -> Type {
        if let Type::Named { name, .. } = value {
            if let Some(query) = name.strip_prefix("typeof ") {
                let name = query.rsplit_once('@').map_or(query, |(name, _)| name);
                if let Some(value) = self.values.get(name) {
                    return value.clone();
                }
            }
        }
        value.clone()
    }

    fn decorator_types_assignable(
        &mut self,
        actual: &Type,
        expected: &Type,
        span: &SourceSpan,
    ) -> bool {
        let mut budget = TypeExpansionBudget::new(self.max_type_expansions);
        budget.checking = self.checking;
        let compatible = is_assignable(
            actual,
            expected,
            &self.types,
            &mut HashSet::new(),
            &mut budget,
        );
        if budget.exhausted {
            self.type_error(
                span,
                "decorator type relation exceeds the type expansion budget".into(),
                DiagnosticCode::ResourceLimit,
            );
            return true;
        }
        compatible
    }
}
