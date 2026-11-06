// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

//! Decorator overloads and generic constraints use ordinary bounded call types.
use super::*;
use crate::checker::decorator_types;
use crate::diagnostic::type_text;

impl ModuleChecker<'_> {
    pub(super) fn resolve_decorator_callee(
        &mut self,
        decorator: &Decorator,
        target: Target,
        class: &ClassDeclaration,
        member: Option<&crate::parser::ClassMemberShell>,
    ) -> Option<Callee> {
        let name = decorator.tokens.first()?;
        let signatures = if decorator.tokens.len() == 1 {
            self.functions.get(&name.text)?.clone()
        } else {
            let value = self.infer_expression(&decorator.tokens, &self.values);
            super::super::expressions::signatures::callable_signatures(&value, false)?
        };
        if signatures.is_empty() || target == Target::Parameter {
            return None;
        }
        let constructor = decorator_types::constructor_type(class);
        if let (Type::Named { name, .. }, Some(value)) =
            (&constructor, self.values.get(&class.name))
        {
            self.types.insert(
                name.clone(),
                TypeDefinition {
                    kind: TypeDefinitionKind::Alias,
                    parameters: Vec::new(),
                    value: value.clone(),
                },
            );
        }
        let actuals = if self.experimental_decorators {
            if member.is_none() {
                vec![constructor]
            } else {
                let member = member?;
                let is_static = member
                    .method
                    .as_ref()
                    .is_some_and(|method| method.is_static)
                    || member
                        .accessor
                        .as_ref()
                        .is_some_and(|accessor| accessor.is_static)
                    || member.field.as_ref().is_some_and(|field| field.is_static);
                let receiver = if is_static {
                    constructor
                } else {
                    class.this_type()
                };
                let key = Type::Literal(
                    serde_json::to_string(member.name.as_deref().unwrap_or("")).unwrap(),
                );
                let mut arguments = vec![receiver, key];
                if let Some(value) = decorator_types::replacement_type(class, Some(member), true) {
                    arguments.push(value);
                }
                arguments
            }
        } else {
            let value = if member.is_none() {
                constructor
            } else if target == Target::Field {
                Type::Undefined
            } else if target == Target::Accessor {
                let field = member?.field.as_ref()?;
                Type::Named {
                    name: "ClassAccessorDecoratorTarget".into(),
                    arguments: vec![
                        if field.is_static {
                            constructor
                        } else {
                            class.this_type()
                        },
                        field.declared_type().unwrap_or(Type::Any),
                    ],
                }
            } else {
                decorator_types::replacement_type(class, member, false)?
            };
            vec![value, decorator_types::context_type(class, member)?]
        };
        let mut instantiated = Vec::new();
        for signature in &signatures {
            let count = signature
                .parameters
                .iter()
                .filter(|parameter| parameter.name != "this")
                .count();
            let arguments = &actuals[..count.min(actuals.len())];
            let mut budget = TypeExpansionBudget::new(self.max_type_expansions);
            budget.checking = self.checking;
            let substitutions =
                infer_call_substitutions(signature, arguments, &self.types, &mut budget);
            if budget.exhausted {
                self.type_error(
                    &decorator.span,
                    "decorator generic inference exceeds its expansion limit".into(),
                    DiagnosticCode::ResourceLimit,
                );
                return Some(Callee::Unknown);
            }
            let bound = FunctionSignature {
                type_parameters: Vec::new(),
                parameters: signature
                    .parameters
                    .iter()
                    .map(|parameter| Parameter {
                        annotation: parameter
                            .annotation
                            .as_ref()
                            .map(|value| substitute_type(value, &substitutions)),
                        ..parameter.clone()
                    })
                    .collect(),
                return_type: substitute_type(&signature.return_type, &substitutions),
            };
            match function_signature_matches(
                signature,
                arguments,
                None,
                &self.types,
                self.max_type_expansions,
                self.checking,
            ) {
                Ok(true) => {
                    return Some(Callee::Function {
                        parameters: bound.parameters,
                        result: bound.return_type,
                        arguments_checked: true,
                    })
                }
                Err(()) => {
                    self.type_error(
                        &decorator.span,
                        "decorator signature comparison exceeds its expansion limit".into(),
                        DiagnosticCode::ResourceLimit,
                    );
                    return Some(Callee::Unknown);
                }
                Ok(false) => instantiated.push(bound),
            }
        }
        let label = match target {
            Target::Class => "class",
            Target::Field | Target::Accessor => "property",
            Target::Parameter => "parameter",
            _ => "method",
        };
        let mut message = format!(
            "Unable to resolve signature of {label} decorator when called as an expression."
        );
        let mut related = Vec::new();
        if signatures.len() > 1 {
            message.push_str("\n  No overload matches this call.");
            for line in self
                .overload_details_with(&instantiated, &actuals, |actual, expected| {
                    self.decorator_relation_detail(actual, expected).0
                })
                .lines()
            {
                if !line.is_empty() {
                    message.push_str(&format!("\n  {line}"));
                }
            }
        } else if let Some(signature) = instantiated.first() {
            for (actual, parameter) in actuals.iter().zip(&signature.parameters) {
                let Some(expected) = &parameter.annotation else {
                    continue;
                };
                let mut budget = TypeExpansionBudget::new(self.max_type_expansions);
                budget.checking = self.checking;
                if !is_assignable(
                    actual,
                    expected,
                    &self.types,
                    &mut HashSet::new(),
                    &mut budget,
                ) {
                    let (detail, information) = self.decorator_relation_detail(actual, expected);
                    related.extend(information);
                    message.push_str(&format!(
                        "\n  Argument of type '{}' is not assignable to parameter of type '{}'.{}",
                        type_text::argument_in(actual, expected, self.project),
                        type_text::render_in(expected, self.project),
                        detail
                            .lines()
                            .map(|line| if line.is_empty() {
                                String::new()
                            } else {
                                format!("\n  {line}")
                            })
                            .collect::<String>()
                    ));
                    break;
                }
            }
        }
        self.typescript_type_error(
            &SourceSpan::new(
                &decorator.span.module,
                name.start,
                decorator.tokens.last()?.end,
            ),
            "resolved decorator signature does not accept its arguments".into(),
            DiagnosticCode::TypeMismatch,
            match target {
                Target::Class => 1238,
                Target::Field | Target::Accessor => 1240,
                Target::Parameter => 1239,
                _ => 1241,
            },
            Vec::new(),
        );
        if let Some(counterpart) = self
            .diagnostics
            .last_mut()
            .and_then(|diagnostic| diagnostic.typescript.as_mut())
        {
            counterpart.message = message;
            counterpart.related_information = related;
        }
        let parameters = instantiated.first()?.parameters.clone();
        let mut results = instantiated
            .into_iter()
            .map(|signature| signature.return_type)
            .collect::<Vec<_>>();
        let result = if results.len() == 1 {
            results.remove(0)
        } else {
            Type::Intersection(results)
        };
        Some(Callee::Function {
            parameters,
            result,
            arguments_checked: true,
        })
    }
}
