// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

//! Declaration conflicts retain their lexical declaration kinds.

use super::*;

impl ModuleChecker<'_> {
    pub(super) fn rest_annotation_error(&mut self, parameter: &Parameter) {
        let mut annotation = parameter
            .annotation
            .clone()
            .expect("annotated rest parameter");
        let mut budget = TypeExpansionBudget::new(self.max_type_expansions);
        let mut visited = HashSet::new();
        while let Some(expanded) = instantiate_named(
            &annotation,
            &self.types,
            &mut visited,
            &mut budget,
            "rest diagnostic",
        ) {
            annotation = expanded;
        }
        let message = "the bounded rest-parameter rule requires an array annotation".to_string();
        if matches!(annotation, Type::Array(_) | Type::Tuple(_)) {
            self.blue_only_type_error(
                &parameter.span, message, DiagnosticCode::TypeMismatch,
                "BlueTSC's function rest subset requires a direct array annotation; TypeScript also accepts array aliases and tuples.",
            );
        } else {
            self.type_error(&parameter.span, message, DiagnosticCode::TypeMismatch);
        }
    }

    pub(super) fn duplicate_counterpart(&self, name: &str, diagnostic: Diagnostic) -> Diagnostic {
        let mut variable = false;
        let mut enumeration = false;
        let mut non_enum = false;
        let mut namespace = None;
        let mut merge_target = None;
        for declaration in &self.module.declarations {
            match declaration {
                Declaration::Variable(value) if value.name == name => {
                    variable = true;
                    non_enum = true;
                }
                Declaration::Enum(value) if value.name == name => enumeration = true,
                Declaration::Namespace(value) if value.name == name => {
                    namespace = Some(value.span.start)
                }
                Declaration::Class(value) if value.name == name => {
                    non_enum = true;
                    merge_target = Some(value.span.start);
                }
                Declaration::Function(value) if value.name == name => {
                    non_enum = true;
                    merge_target = Some(value.span.start);
                }
                _ => {}
            }
        }
        let code = if enumeration && non_enum {
            2567
        } else if variable {
            2451
        } else if namespace
            .zip(merge_target)
            .is_some_and(|(namespace, target)| namespace < target)
        {
            2434
        } else {
            2300
        };
        diagnostic.with_typescript(code, vec![name.into()])
    }

    pub(super) fn class_shape_counterpart(
        &self,
        class: &crate::parser::ClassDeclaration,
        diagnostic: Diagnostic,
    ) -> Diagnostic {
        for member in &class.members {
            if member.kind != crate::parser::ClassMemberKind::Opaque {
                continue;
            }
            let tokens = class
                .body
                .iter()
                .filter(|token| member.span.start <= token.start && token.end <= member.span.end)
                .collect::<Vec<_>>();
            if tokens.windows(2).any(|pair| {
                pair[0].is("static")
                    && matches!(pair[1].text.as_str(), "public" | "protected" | "private")
            }) {
                let modifier = tokens
                    .iter()
                    .find(|token| matches!(token.text.as_str(), "public" | "protected" | "private"))
                    .unwrap();
                return diagnostic
                    .with_typescript(1029, vec![modifier.text.clone(), "static".into()]);
            }
        }
        diagnostic
    }
}
