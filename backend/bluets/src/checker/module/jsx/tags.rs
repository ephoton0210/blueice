// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

//! JSX value tags reuse bounded call inference and ordinary type substitution.
use super::*;

impl ModuleChecker<'_> {
    pub(super) fn jsx_value_props(
        &mut self,
        name: &JsxName,
        element: &JsxElement,
        span: &SourceSpan,
        scope: &BTreeMap<String, Type>,
    ) -> Props {
        if let Some(signature) = self
            .functions
            .get(&name.text)
            .and_then(|s| s.first())
            .cloned()
        {
            return self.jsx_bound_function_props(name, &signature, element, span, scope);
        }
        if let Some(binding) = self.class_constructors.get(&name.text).cloned() {
            let Some(signature) = binding.signatures.first() else {
                return Props::Open;
            };
            let Some(substitutions) = self.jsx_tag_substitutions(signature, element, span, scope)
            else {
                return Props::Open;
            };
            let instance = substitute_type(&signature.return_type, &substitutions);
            let component = self
                .values
                .get(&name.text)
                .cloned()
                .unwrap_or(Type::Unknown);
            self.jsx_validate_element_type(name, &component, span);
            let props = self.class_props(&name.text, &instance, span);
            return self.jsx_managed_props(&component, props, Some(&instance), span);
        }
        let bound = scope
            .get(&name.text)
            .or_else(|| self.values.get(&name.text))
            .cloned();
        match bound {
            Some(Type::Function { parameters, result }) => {
                let signature = FunctionSignature {
                    parameters,
                    return_type: *result,
                    type_parameters: Vec::new(),
                };
                self.jsx_bound_function_props(name, &signature, element, span, scope)
            }
            Some(Type::GenericFunction {
                parameters,
                result,
                type_parameters,
                ..
            }) => {
                let signature = FunctionSignature {
                    parameters,
                    return_type: *result,
                    type_parameters,
                };
                self.jsx_bound_function_props(name, &signature, element, span, scope)
            }
            Some(_) => Props::Open,
            None if self.name_is_in_scope(name.text.split('.').next().unwrap_or(&name.text)) => {
                Props::Open
            }
            None => {
                self.type_error(
                    span,
                    format!("cannot find the JSX tag name `{}`", name.text),
                    DiagnosticCode::UnknownName,
                );
                Props::Open
            }
        }
    }

    fn jsx_bound_function_props(
        &mut self,
        name: &JsxName,
        signature: &FunctionSignature,
        element: &JsxElement,
        span: &SourceSpan,
        scope: &BTreeMap<String, Type>,
    ) -> Props {
        let Some(substitutions) = self.jsx_tag_substitutions(signature, element, span, scope)
        else {
            return Props::Open;
        };
        let parameters = signature
            .parameters
            .iter()
            .map(|p| Parameter {
                annotation: p
                    .annotation
                    .as_ref()
                    .map(|t| substitute_type(t, &substitutions)),
                ..p.clone()
            })
            .collect::<Vec<_>>();
        let result = substitute_type(&signature.return_type, &substitutions);
        let callable = Type::Function {
            parameters: parameters.clone(),
            result: Box::new(result.clone()),
        };
        self.jsx_validate_element_type(name, &callable, span);
        let component = if self.namespaces.contains_key(&name.text) {
            let prefix = format!("{}.", name.text);
            let fields = self
                .values
                .iter()
                .filter_map(|(key, value)| {
                    let member = key.strip_prefix(&prefix)?;
                    (!member.contains('.')).then(|| TypeField {
                        accessor_write_type: None,
                        method: false,
                        name: member.to_string(),
                        readonly: false,
                        optional: false,
                        value: value.clone(),
                        span: self.jsx_span(name.start, name.end),
                    })
                })
                .collect();
            Type::Intersection(vec![callable, Type::Record(fields)])
        } else {
            callable
        };
        let props = self.component_props(&parameters, &result, &[], span);
        self.jsx_managed_props(&component, props, None, span)
    }

    fn jsx_tag_substitutions(
        &mut self,
        signature: &FunctionSignature,
        element: &JsxElement,
        span: &SourceSpan,
        scope: &BTreeMap<String, Type>,
    ) -> Option<BTreeMap<String, Type>> {
        if let Some(range) = &element.type_arguments {
            let (arguments, references) =
                match crate::parser::parse_jsx_type_arguments(self.module, range.clone(), 128) {
                    Ok(arguments) => arguments,
                    Err(diagnostics) => {
                        self.diagnostics.extend(diagnostics);
                        return None;
                    }
                };
            let mut missing = false;
            for reference in references {
                let bound = if reference.value_query {
                    self.scopes
                        .as_ref()
                        .and_then(|scopes| scopes.query_type(&reference.name, reference.span.start))
                        .is_some()
                        || self.values.contains_key(&reference.name)
                } else {
                    self.types.contains_key(&reference.name)
                        || self.type_parameters.contains(&reference.name)
                };
                if !bound {
                    self.typescript_type_error(
                        &reference.span,
                        format!("cannot find type argument name `{}`", reference.name),
                        DiagnosticCode::UnknownType,
                        2304,
                        vec![reference.name],
                    );
                    missing = true;
                }
            }
            if missing {
                return None;
            }
            let before = self.diagnostics.len();
            let substitutions = self.check_explicit_function_type_arguments(
                signature,
                &arguments,
                span,
                element.start,
            );
            for diagnostic in &mut self.diagnostics[before..] {
                if let Some(counterpart) = &mut diagnostic.typescript {
                    if matches!(counterpart.code, 2344 | 2558) {
                        counterpart.span = SourceSpan::new(&self.module.id, range.start, range.end);
                    }
                }
            }
            return substitutions;
        }
        let actual = self.jsx_actual_props(element, scope, &Props::Open);
        let mut budget = TypeExpansionBudget::new(self.max_type_expansions);
        budget.checking = self.checking;
        let substitutions =
            infer_call_substitutions(signature, &[actual], &self.types, &mut budget);
        if budget.exhausted {
            self.type_error(
                span,
                "JSX component inference exceeds the generic-expansion limit".into(),
                DiagnosticCode::ResourceLimit,
            );
            return None;
        }
        Some(substitutions)
    }
}
