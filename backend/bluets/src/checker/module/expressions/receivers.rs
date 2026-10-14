// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

//! Explicit receiver annotations affect calls without consuming an argument.

use super::*;

impl ModuleChecker<'_> {
    pub(super) fn receiver_compatibility_detail(&self, actual: &Type, expected: &Type) -> String {
        if let ((Some(actual_fields), false), (Some(expected_fields), false)) = (
            self.expanded_record_fields(actual.clone()),
            self.expanded_record_fields(expected.clone()),
        ) {
            let missing = expected_fields
                .iter()
                .filter(|field| {
                    !field.optional && !actual_fields.iter().any(|actual| actual.name == field.name)
                })
                .map(|field| field.name.as_str())
                .collect::<Vec<_>>();
            let actual_text = crate::diagnostic::type_text::render_in(actual, self.project);
            let expected_text = crate::diagnostic::type_text::render_in(expected, self.project);
            if let [name] = missing.as_slice() {
                return format!("\n  Property '{name}' is missing in type '{actual_text}' but required in type '{expected_text}'.");
            }
            if !missing.is_empty() {
                return format!("\n  Type '{actual_text}' is missing the following properties from type '{expected_text}': {}", missing.join(", "));
            }
        }
        self.compatibility_detail(actual, expected)
            .unwrap_or_else(|| crate::diagnostic::type_text::detail(actual, expected, self.project))
    }

    pub(super) fn check_call_receiver(
        &mut self,
        parameters: &[Parameter],
        receiver: Option<&[Token]>,
        scope: &BTreeMap<String, Type>,
        span: &SourceSpan,
        location: &[Token],
    ) {
        let Some(expected) = parameters
            .iter()
            .find(|parameter| parameter.name == "this")
            .and_then(|parameter| parameter.annotation.as_ref())
        else {
            return;
        };
        let actual = receiver.map_or(Type::Void, |tokens| self.infer_expression(tokens, scope));
        if self.is_assignable_bounded(&actual, expected, span) {
            return;
        }
        self.typescript_type_error(
            span,
            "the call receiver is incompatible with its this parameter".to_string(),
            DiagnosticCode::TypeMismatch,
            2684,
            vec![
                crate::diagnostic::type_text::render_in(&actual, self.project),
                crate::diagnostic::type_text::render_in(expected, self.project),
            ],
        );
        let detail = self.receiver_compatibility_detail(&actual, expected);
        if let Some(counterpart) = self
            .diagnostics
            .last_mut()
            .and_then(|d| d.typescript.as_mut())
        {
            counterpart.message.push_str(&detail);
        }
        self.point_last_typescript(location);
    }
}
