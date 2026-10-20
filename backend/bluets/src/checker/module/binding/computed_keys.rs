// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

//! Bind computed-key identities without creating source-visible declarations.
use super::*;

impl ModuleChecker<'_> {
    pub(in crate::checker::module) fn bind_computed_key_values(&mut self) {
        for (name, value) in &self.values {
            if matches!(value, Type::UniqueSymbol(_) | Type::Symbol) {
                self.types.insert(
                    crate::symbol_keys::name(&self.module.id, name),
                    TypeDefinition {
                        kind: TypeDefinitionKind::Alias,
                        parameters: Vec::new(),
                        value: value.clone(),
                    },
                );
            }
        }
    }

    pub(in crate::checker::module) fn check_computed_type_key(&mut self, field: &TypeField) {
        let Some(entity) = crate::symbol_keys::entity(&field.name) else {
            return;
        };
        let Some(value) = self.values.get(entity) else {
            return;
        };
        if matches!(
            value,
            Type::Symbol
                | Type::UniqueSymbol(_)
                | Type::String
                | Type::Number
                | Type::Literal(_)
                | Type::Any
        ) {
            return;
        }
        let tokens = crate::syntax::lex(&self.module.id, &self.module.source).unwrap_or_default();
        let Some(open) = tokens
            .iter()
            .position(|token| token.start >= field.span.start && token.is("["))
        else {
            return;
        };
        let Some(close) = crate::checker::scopes::targets::close(&tokens, open) else {
            return;
        };
        let point = SourceSpan::new(&self.module.id, tokens[open].start, tokens[close].end);
        self.typescript_type_error(
            &point,
            "invalid computed key domain".into(),
            DiagnosticCode::TypeMismatch,
            2464,
            Vec::new(),
        );
    }
}
