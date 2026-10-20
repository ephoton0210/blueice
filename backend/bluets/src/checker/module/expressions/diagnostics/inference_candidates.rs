// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

//! A rejected contravariant candidate retains its missing property's origin.
use super::*;

impl ModuleChecker<'_> {
    pub(super) fn inference_missing_parameter_property(
        &self,
        actual: &Type,
        expected: &Type,
    ) -> Option<crate::TypeScriptRelatedInformation> {
        let (
            Type::Function {
                parameters: actual, ..
            },
            Type::Function {
                parameters: expected,
                ..
            },
        ) = (
            self.compatibility_shape(actual),
            self.compatibility_shape(expected),
        )
        else {
            return None;
        };
        for (actual, expected) in actual.iter().zip(&expected) {
            let (Type::Record(target), Type::Record(source)) = (
                self.compatibility_shape(actual.annotation.as_ref()?),
                self.compatibility_shape(expected.annotation.as_ref()?),
            ) else {
                continue;
            };
            if let Some(field) = target
                .iter()
                .find(|f| !f.optional && !source.iter().any(|s| s.name == f.name))
            {
                let source = self.project.source(&field.span.module)?;
                let tokens = crate::syntax::lex(&field.span.module, source).ok()?;
                let token = tokens.iter().find(|t| {
                    field.span.start <= t.start && t.end <= field.span.end && t.is(&field.name)
                })?;
                return Some(crate::TypeScriptRelatedInformation {
                    code: 2728,
                    message: format!("'{}' is declared here.", field.name),
                    span: token.span(&field.span.module),
                    position: None,
                });
            }
        }
        None
    }
}
