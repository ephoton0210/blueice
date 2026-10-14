// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

//! Declared variance is validated using fresh ordered parameter witnesses.
use super::*;
use crate::parser::Variance;

impl ModuleChecker<'_> {
    pub(in crate::checker::module) fn check_variance(
        &mut self,
        name: &str,
        parameters: &[TypeParameter],
        value: &Type,
        alias: bool,
    ) {
        for parameter in parameters {
            if parameter.is_const {
                let span = SourceSpan::new(
                    &parameter.span.module,
                    parameter.span.start,
                    parameter.span.start + "const".len(),
                );
                self.typescript_type_error(
                    &span,
                    "const type parameters require a function, method or class".into(),
                    DiagnosticCode::TypeMismatch,
                    1277,
                    vec!["const".into()],
                );
            }
            let Some(variance) = parameter.variance else {
                continue;
            };
            if alias
                && !matches!(
                    value,
                    Type::Record(_)
                        | Type::CallableRecord { .. }
                        | Type::IndexedRecord { .. }
                        | Type::Function { .. }
                        | Type::GenericFunction { .. }
                        | Type::Mapped(_)
                )
            {
                self.typescript_type_error(
                    &parameter.span,
                    "variance annotation needs an object, function, constructor or mapped alias"
                        .into(),
                    DiagnosticCode::TypeMismatch,
                    2637,
                    Vec::new(),
                );
                continue;
            }
            if variance == Variance::InOut {
                continue;
            }
            let sub = format!("sub-{}", parameter.name);
            let sup = format!("super-{}", parameter.name);
            let named = |name: &str| Type::Named {
                name: name.into(),
                arguments: Vec::new(),
            };
            let mut types = self.types.clone();
            types.insert(
                sup.clone(),
                TypeDefinition {
                    kind: TypeDefinitionKind::Parameter,
                    parameters: Vec::new(),
                    value: Type::StrictUnknown,
                },
            );
            types.insert(
                sub.clone(),
                TypeDefinition {
                    kind: TypeDefinitionKind::Parameter,
                    parameters: Vec::new(),
                    value: named(&sup),
                },
            );
            let arguments = |which: &str| {
                parameters
                    .iter()
                    .map(|p| {
                        if p.name == parameter.name {
                            named(which)
                        } else {
                            Type::Any
                        }
                    })
                    .collect::<Vec<_>>()
            };
            let actual_args = arguments(if variance == Variance::Out {
                &sub
            } else {
                &sup
            });
            let expected_args = arguments(if variance == Variance::Out {
                &sup
            } else {
                &sub
            });
            let substitute = |args: &[Type]| {
                substitute_type(
                    value,
                    &parameters
                        .iter()
                        .map(|p| p.name.clone())
                        .zip(args.iter().cloned())
                        .collect(),
                )
            };
            let actual = substitute(&actual_args);
            let expected = substitute(&expected_args);
            let mut budget = TypeExpansionBudget::new(self.max_type_expansions);
            budget.checking = self.checking;
            if !is_assignable(&actual, &expected, &types, &mut HashSet::new(), &mut budget)
                && !budget.exhausted
            {
                let actual_name = Type::Named {
                    name: name.into(),
                    arguments: actual_args,
                };
                let expected_name = Type::Named {
                    name: name.into(),
                    arguments: expected_args,
                };
                self.typescript_type_error(
                    &parameter.span,
                    "type body contradicts its declared variance".into(),
                    DiagnosticCode::TypeMismatch,
                    2636,
                    vec![
                        crate::diagnostic::type_text::render_in(&actual_name, self.project),
                        crate::diagnostic::type_text::render_in(&expected_name, self.project),
                    ],
                );
                let detail = variance_method_detail(&actual, &expected, self.project)
                    .unwrap_or_else(|| {
                        crate::diagnostic::type_text::detail(&actual, &expected, self.project)
                    });
                if let Some(counterpart) = self
                    .diagnostics
                    .last_mut()
                    .and_then(|d| d.typescript.as_mut())
                {
                    counterpart.message.push_str(&detail);
                }
            }
        }
    }
}

fn variance_method_detail(
    actual: &Type,
    expected: &Type,
    project: &crate::Project,
) -> Option<String> {
    let (Type::Record(actual), Type::Record(expected)) = (actual, expected) else {
        return None;
    };
    for target in expected.iter() {
        let source = actual.iter().find(|field| field.name == target.name)?;
        let (
            Type::Function {
                parameters: a,
                result: ar,
            },
            Type::Function {
                parameters: e,
                result: er,
            },
        ) = (&source.value, &target.value)
        else {
            continue;
        };
        if ar != er
            && a.len() == e.len()
            && a.iter()
                .zip(e)
                .all(|(a, e)| a.annotation == e.annotation && a.optional == e.optional)
        {
            return Some(format!("\n  The types returned by '{}()' are incompatible between these types.\n    Type '{}' is not assignable to type '{}'.",target.name,crate::diagnostic::type_text::render_in(ar,project),crate::diagnostic::type_text::render_in(er,project)));
        }
    }
    None
}
