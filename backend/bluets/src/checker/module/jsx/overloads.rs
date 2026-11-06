// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

//! JSX overloads check each candidate's managed props before choosing a target.
use super::*;
use crate::diagnostic::type_text;

impl ModuleChecker<'_> {
    pub(super) fn jsx_overloaded_props(
        &mut self,
        name: &JsxName,
        signatures: &[FunctionSignature],
        element: &JsxElement,
        span: &SourceSpan,
        scope: &BTreeMap<String, Type>,
        construct: bool,
    ) -> Props {
        let mut failures = Vec::new();
        for signature in signatures {
            let before = self.diagnostics.len();
            let props = if construct {
                self.jsx_bound_class_props(name, signature, element, span, scope)
            } else {
                self.jsx_bound_function_props(name, signature, element, span, scope)
            };
            let props = self.jsx_narrow_props(props, element, scope);
            if self.diagnostics.len() == before
                && self.jsx_candidate_fits(&props, element, scope, span)
            {
                return props;
            }
            if self.diagnostics[before..]
                .iter()
                .any(|diagnostic| diagnostic.code == DiagnosticCode::ResourceLimit)
            {
                return Props::Open;
            }
            self.check_jsx_attributes(element, &props, scope);
            self.check_jsx_children(element, &props, scope);
            let diagnostics = self.diagnostics.split_off(before);
            failures.push((signature, diagnostics));
        }
        let mut message = String::from("No overload matches this call.");
        let mut related = Vec::new();
        let mut location = self.jsx_span(name.start, name.end);
        for (index, (signature, diagnostics)) in failures.iter().enumerate() {
            let Some(diagnostic) = diagnostics.first() else {
                continue;
            };
            let Some(counterpart) = &diagnostic.typescript else {
                self.diagnostics.extend(diagnostics.iter().cloned());
                return Props::Open;
            };
            if construct && index == 0 {
                if let Some(JsxAttribute::Named { name, .. }) = element.attributes.iter().find(|attribute| {
                    matches!(attribute, JsxAttribute::Named { start, end, .. } if *start <= diagnostic.span.start && diagnostic.span.end <= *end)
                }) {
                    location = self.jsx_span(name.start, name.end);
                }
            }
            let parameters =
                type_text::parameter_list_in(&signature.parameters, Some(self.project));
            let result = type_text::render_in(&signature.return_type, self.project);
            let result = result.strip_prefix("JSX.").unwrap_or(&result);
            message.push_str(&format!(
                "\n  Overload {} of {}, '({parameters}): {result}', gave the following error.",
                index + 1,
                signatures.len()
            ));
            for line in counterpart.message.lines() {
                message.push_str(&format!("\n    {line}"));
            }
            related.extend(counterpart.related_information.iter().cloned());
        }
        self.typescript_type_error(
            &location,
            "no JSX overload accepts these props".into(),
            DiagnosticCode::TypeMismatch,
            2769,
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
        Props::Open
    }

    fn jsx_candidate_fits(
        &mut self,
        props: &Props,
        element: &JsxElement,
        scope: &BTreeMap<String, Type>,
        span: &SourceSpan,
    ) -> bool {
        let Props::Known { fields, .. } = props else {
            return true;
        };
        let Type::Record(actual) = self.jsx_actual_props(element, scope, props) else {
            return false;
        };
        for attribute in &element.attributes {
            if let JsxAttribute::Named { name, .. } = attribute {
                if !name.text.contains('-') && !fields.iter().any(|field| field.name == name.text) {
                    return false;
                }
            }
        }
        for expected in fields {
            match actual.iter().find(|field| field.name == expected.name) {
                Some(actual)
                    if !self.is_assignable_bounded(&actual.value, &expected.value, span) =>
                {
                    return false
                }
                None if !expected.optional && !matches!(expected.value, Type::Undefined) => {
                    return false
                }
                _ => {}
            }
        }
        true
    }
}
