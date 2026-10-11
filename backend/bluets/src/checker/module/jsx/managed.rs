// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

//! The JSX namespace supplies tag eligibility and component-managed attributes.
use super::*;
use crate::diagnostic::type_text;

impl ModuleChecker<'_> {
    pub(super) fn jsx_validate_element_type(
        &mut self,
        name: &JsxName,
        component: &Type,
        span: &SourceSpan,
    ) {
        if !self.enforce_types {
            return;
        }
        let Some(key) = self.jsx_key("ElementType") else {
            return;
        };
        let target = Type::Named {
            name: key,
            arguments: Vec::new(),
        };
        if self.is_assignable_bounded(component, &target, span) {
            return;
        }
        self.typescript_type_error(
            &self.jsx_span(name.start, name.end),
            format!(
                "JSX tag `{}` is not assignable to the declared ElementType",
                name.text
            ),
            DiagnosticCode::TypeMismatch,
            2786,
            vec![name.text.clone()],
        );
        let actual = type_text::render_in(component, self.project);
        let Some(expanded) = self.jsx_expand_type(&target, span) else {
            return;
        };
        let function = match &expanded {
            Type::Union(parts) => parts
                .iter()
                .find(|p| matches!(p, Type::Function { .. } | Type::GenericFunction { .. })),
            Type::Function { .. } | Type::GenericFunction { .. } => Some(&expanded),
            _ => None,
        };
        let detail = match (component, function) {
            (
                Type::Function {
                    result: actual_result,
                    ..
                },
                Some(
                    expected @ Type::Function {
                        result: expected_result,
                        ..
                    },
                ),
            ) => {
                format!("\n    Type '{actual}' is not assignable to type '{}'.\n      Type '{}' is not assignable to type '{}'.", type_text::render_in(expected, self.project), type_text::render_in(actual_result,self.project),type_text::render_in(expected_result,self.project))
            }
            _ => String::new(),
        };
        if let Some(counterpart) = self
            .diagnostics
            .last_mut()
            .and_then(|d| d.typescript.as_mut())
        {
            counterpart.message.push_str(&format!(
                "\n  Its type '{actual}' is not a valid JSX element type.{detail}"
            ));
        }
    }

    fn jsx_expand_type(&mut self, value: &Type, span: &SourceSpan) -> Option<Type> {
        let mut value = value.clone();
        let mut visited = HashSet::new();
        let mut budget = TypeExpansionBudget::new(self.max_type_expansions);
        while let Some(next) = instantiate_named(
            &value,
            &self.types,
            &mut visited,
            &mut budget,
            "JSX namespace member",
        ) {
            if next == value {
                break;
            }
            value = next;
        }
        if budget.exhausted {
            self.type_error(
                span,
                "JSX namespace type expansion exceeds the generic-expansion limit".into(),
                DiagnosticCode::ResourceLimit,
            );
            None
        } else {
            Some(value)
        }
    }

    pub(super) fn jsx_managed_props(
        &mut self,
        component: &Type,
        props: Props,
        instance: Option<&Type>,
        span: &SourceSpan,
    ) -> Props {
        let Props::Known { value, .. } = props else {
            return props;
        };
        let mut value = if let Some(name) = self.jsx_key("LibraryManagedAttributes") {
            let managed = Type::Named {
                name: name.clone(),
                arguments: vec![component.clone(), value],
            };
            // A direct record alias keeps its declared name; a conditional selects
            // the concrete branch before attribute expansion and presentation.
            if self
                .types
                .get(&name)
                .is_some_and(|definition| matches!(definition.value, Type::Conditional(_)))
            {
                let Some(value) = self.jsx_expand_type(&managed, span) else {
                    return Props::Open;
                };
                value
            } else {
                managed
            }
        } else {
            value
        };
        if let (Some(instance), Some(name)) = (instance, self.jsx_key("IntrinsicClassAttributes")) {
            value = Type::Intersection(vec![
                Type::Named {
                    name,
                    arguments: vec![instance.clone()],
                },
                value,
            ]);
        }
        let mut budget = TypeExpansionBudget::new(self.max_type_expansions);
        let value = self.jsx_reduced_props_type(&value, &mut budget);
        if budget.exhausted {
            self.type_error(
                span,
                "JSX managed prop arguments exceed the generic-expansion limit".into(),
                DiagnosticCode::ResourceLimit,
            );
            return Props::Open;
        }
        let mut props = self.props_of(&value, true);
        if let Props::Known { fields, .. } = &mut props {
            if let Some(intrinsic) = self.jsx_interface_fields("IntrinsicAttributes") {
                for field in intrinsic {
                    if !fields.iter().any(|existing| existing.name == field.name) {
                        fields.push(field)
                    }
                }
            }
        } else if !matches!(value, Type::Any | Type::Unknown) {
            self.type_error(
                span,
                "JSX managed attributes cannot be expanded within the type budget".into(),
                DiagnosticCode::ResourceLimit,
            );
        }
        props
    }

    fn jsx_reduced_props_type(&self, value: &Type, budget: &mut TypeExpansionBudget) -> Type {
        if !budget.consume() {
            return value.clone();
        }
        match value {
            Type::Named { name, arguments } => Type::Named {
                name: name.clone(),
                arguments: arguments
                    .iter()
                    .map(|argument| {
                        let reduced = crate::checker::type_operators::diagnostic_type(
                            argument,
                            &self.types,
                            budget,
                        );
                        self.jsx_reduced_props_type(&reduced, budget)
                    })
                    .collect(),
            },
            Type::Intersection(parts) => Type::Intersection(
                parts
                    .iter()
                    .map(|part| self.jsx_reduced_props_type(part, budget))
                    .collect(),
            ),
            _ => value.clone(),
        }
    }
}
