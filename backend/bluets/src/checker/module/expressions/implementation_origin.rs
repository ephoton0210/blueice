// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

//! Hidden implementation signatures explain rejection without admitting calls.
use super::*;

impl ModuleChecker<'_> {
    pub(in crate::checker::module) fn present_hidden_implementation(
        &mut self,
        callee: &str,
        actuals: &[Type],
        construct: bool,
    ) {
        let has_function_overloads = self.module.declarations.iter().any(|declaration| {
            matches!(declaration, Declaration::Function(function) if function.name == callee && function.overload)
        });
        let mut implementation = None;
        for declaration in &self.module.declarations {
            match declaration {
                Declaration::Function(function)
                    if !construct
                        && has_function_overloads
                        && function.name == callee
                        && !function.overload
                        && !function.declared =>
                {
                    implementation = Some((
                        function.parameters.clone(),
                        function.span.clone(),
                        function.name.clone(),
                    ));
                }
                Declaration::Class(class)
                    if construct
                        && class.name == callee
                        && class.members.iter().any(|member| {
                            member
                                .constructor
                                .as_ref()
                                .is_some_and(|constructor| constructor.body.is_none())
                        }) =>
                {
                    if let Some(member) = class.members.iter().find(|member| {
                        member
                            .constructor
                            .as_ref()
                            .is_some_and(|constructor| constructor.body.is_some())
                    }) {
                        implementation = Some((
                            member.constructor.as_ref().unwrap().parameters.clone(),
                            member.span.clone(),
                            "constructor".into(),
                        ));
                    }
                }
                _ => {}
            }
        }
        let Some((parameters, span, name)) = implementation else {
            return;
        };
        let signature = FunctionSignature {
            parameters,
            type_parameters: Vec::new(),
            return_type: Type::Any,
        };
        if function_signature_matches(
            &signature,
            actuals,
            None,
            &self.types,
            self.max_type_expansions,
            self.checking,
        ) != Ok(true)
        {
            return;
        }
        let Ok(tokens) = crate::syntax::lex(
            &span.module,
            self.project.source(&span.module).unwrap_or(""),
        ) else {
            return;
        };
        let Some(token) = tokens
            .iter()
            .find(|token| span.start <= token.start && token.end <= span.end && token.is(&name))
        else {
            return;
        };
        if let Some(counterpart) = self
            .diagnostics
            .last_mut()
            .and_then(|diagnostic| diagnostic.typescript.as_mut())
            .filter(|counterpart| matches!(counterpart.code, 2345 | 2769))
        {
            if !counterpart
                .related_information
                .iter()
                .any(|related| related.code == 2793)
            {
                let hint =
                    Diagnostic::error(DiagnosticCode::TypeMismatch, token.span(&span.module), "")
                        .with_typescript(2793, Vec::new())
                        .typescript
                        .unwrap();
                counterpart
                    .related_information
                    .push(crate::TypeScriptRelatedInformation {
                        code: hint.code,
                        message: hint.message,
                        span: hint.span,
                        position: None,
                    });
            }
        }
    }
}
