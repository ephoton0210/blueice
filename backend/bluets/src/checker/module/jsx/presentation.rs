// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

//! Presentation of an already-rejected JSX props assignment.
use super::*;
use crate::diagnostic::type_text;

impl ModuleChecker<'_> {
    fn jsx_present_message(&mut self, actual: String, expected: String, detail: String) {
        if !self.enforce_types {
            return;
        }
        let Some(diagnostic) = self.diagnostics.last_mut() else {
            return;
        };
        let span = diagnostic
            .typescript
            .as_ref()
            .map(|counterpart| counterpart.span.clone())
            .unwrap_or_else(|| diagnostic.span.clone());
        let mut counterpart = Diagnostic::error(diagnostic.code, span, &diagnostic.message)
            .with_typescript(2322, vec![actual, expected])
            .typescript
            .unwrap();
        counterpart.message.push_str(&detail);
        diagnostic.typescript = Some(counterpart);
    }
    pub(super) fn jsx_present_pair(&mut self, actual: &Type, expected: &Type) {
        self.jsx_present_message(
            type_text::argument_in(actual, expected, self.project),
            type_text::render_in(expected, self.project),
            type_text::detail(actual, expected, self.project),
        );
    }
    fn jsx_actual_props(
        &self,
        element: &JsxElement,
        scope: &BTreeMap<String, Type>,
        props: &Props,
    ) -> Type {
        let mut fields = Vec::new();
        let children = Self::semantic_children(element)
            .iter()
            .filter_map(|child| match child {
                JsxChild::Text { start, end }
                    if !jsx::is_blank_with_newline(&self.module.source[*start..*end]) =>
                {
                    Some(Type::String)
                }
                JsxChild::Expression { tokens, .. } if !tokens.is_empty() => {
                    Some(self.infer_expression(tokens, scope))
                }
                JsxChild::Element(_) => Some(self.jsx_element_type()),
                _ => None,
            })
            .collect::<Vec<_>>();
        if !children.is_empty() {
            let value = if children.len() == 1 {
                children[0].clone()
            } else {
                let mut values = children;
                values.dedup();
                Type::Array(Box::new(if values.len() == 1 {
                    values.remove(0)
                } else {
                    Type::Union(values)
                }))
            };
            fields.push(TypeField {
                name: self
                    .jsx_children_name()
                    .unwrap_or_else(|| "children".into()),
                readonly: false,
                optional: false,
                value,
                span: self.jsx_span(element.start, element.opening_end),
            });
        }
        for attribute in &element.attributes {
            match attribute {
                JsxAttribute::Named { name, value, .. } => {
                    let value = match value {
                        Some(JsxValue::String { .. }) => Type::String,
                        Some(JsxValue::Expression { tokens, .. }) => match props {
                            Props::Known { fields, .. } => fields
                                .iter()
                                .find(|field| field.name == name.text)
                                .map(|field| self.infer_in_context(tokens, scope, &field.value))
                                .unwrap_or_else(|| self.infer_expression(tokens, scope)),
                            _ => self.infer_expression(tokens, scope),
                        },
                        Some(JsxValue::Element(_)) => self.jsx_element_type(),
                        None => Type::Boolean,
                    };
                    fields.push(TypeField {
                        name: name.text.clone(),
                        readonly: false,
                        optional: false,
                        value,
                        span: self.jsx_span(name.start, name.end),
                    });
                }
                JsxAttribute::Spread { tokens, .. } => {
                    if let Some(spread) = self
                        .expanded_record_fields(self.infer_expression(tokens, scope))
                        .0
                    {
                        fields.extend(spread);
                    }
                }
            }
        }
        Type::Record(fields)
    }
    pub(super) fn jsx_present_props(
        &mut self,
        element: &JsxElement,
        props: &Props,
        scope: &BTreeMap<String, Type>,
        excess: Option<&str>,
        missing: Option<&str>,
    ) {
        let Props::Known {
            value, value_based, ..
        } = props
        else {
            return;
        };
        let actual =
            type_text::render_in(&self.jsx_actual_props(element, scope, props), self.project);
        let target = type_text::render_in(value, self.project);
        let expected = if *value_based {
            format!("IntrinsicAttributes & {target}")
        } else {
            target.clone()
        };
        let detail = if let Some(name) = excess {
            format!("\n  Property '{name}' does not exist on type '{expected}'.")
        } else if let Some(name) = missing {
            format!("\n  Property '{name}' is missing in type '{actual}' but required in type '{target}'.")
        } else {
            String::new()
        };
        self.jsx_present_message(actual, expected, detail);
    }
    pub(super) fn jsx_present_children(
        &mut self,
        element: &JsxElement,
        props: &Props,
        scope: &BTreeMap<String, Type>,
        actual: &Type,
        expected: &Type,
    ) {
        let Props::Known { value, .. } = props else {
            return;
        };
        let given = self.jsx_actual_props(element, scope, props);
        let given_text = type_text::render_in(&given, self.project);
        let target = type_text::render_in(value, self.project);
        let actual = match actual {
            Type::Array(value) if matches!(value.as_ref(),Type::Union(values) if values.iter().all(|value|value==&values[0])) => {
                Type::Array(Box::new(match value.as_ref() {
                    Type::Union(values) => values[0].clone(),
                    _ => unreachable!(),
                }))
            }
            _ => actual.clone(),
        };
        let pair=format!("\n  Type '{given_text}' is not assignable to type '{target}'.\n    Types of property '{}' are incompatible.\n      Type '{}' is not assignable to type '{}'.",self.jsx_children_name().unwrap_or_else(||"children".into()),type_text::render_in(&actual,self.project),type_text::render_in(expected,self.project));
        self.jsx_present_message(given_text, format!("IntrinsicAttributes & {target}"), pair);
    }
    pub(super) fn jsx_present_spread(&mut self, actual: &Type, props: &Props) {
        let Props::Known { value, fields, .. } = props else {
            return;
        };
        self.jsx_present_message(
            type_text::render_in(actual, self.project),
            type_text::render_in(value, self.project),
            type_text::detail(actual, &Type::Record(fields.clone()), self.project),
        );
    }
}
