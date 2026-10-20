// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

//! Missing unique-symbol properties do not inherit unrelated record values.
use super::*;

impl ModuleChecker<'_> {
    pub(in crate::checker::module) fn check_computed_symbol_reads(
        &mut self,
        tokens: &[Token],
        scope: &BTreeMap<String, Type>,
        span: &SourceSpan,
    ) {
        if !self.checking.no_implicit_any {
            return;
        }
        let execution = self
            .scopes
            .as_ref()
            .map(|scopes| scopes.flow_execution(span.start));
        for (index, piece) in tokens.windows(4).enumerate() {
            let [receiver, open, key, close] = piece else {
                unreachable!()
            };
            if receiver.kind != TokenKind::Identifier
                || !open.is("[")
                || !close.is("]")
                || key.kind != TokenKind::Identifier
                || index
                    .checked_sub(1)
                    .is_some_and(|previous| tokens[previous].is(".") || tokens[previous].is("?."))
                || self
                    .scopes
                    .as_ref()
                    .is_some_and(|scopes| Some(scopes.flow_execution(receiver.start)) != execution)
            {
                continue;
            }
            let key_type = self.infer_expression(std::slice::from_ref(key), scope);
            if !matches!(key_type, Type::UniqueSymbol(_)) {
                continue;
            }
            let owner = self.infer_expression(std::slice::from_ref(receiver), scope);
            let mut budget = TypeExpansionBudget::new(self.max_type_expansions);
            budget.checking = self.checking;
            let expanded = crate::checker::type_operators::expanded(
                &owner,
                &self.types,
                &mut HashSet::new(),
                &mut budget,
            );
            if !matches!(expanded, Type::Record(_) | Type::CallableRecord { .. }) {
                continue;
            }
            let name = crate::symbol_keys::name(&self.module.id, &key.text);
            if !matches!(
                property_type(
                    &expanded,
                    &name,
                    &self.types,
                    &mut HashSet::new(),
                    &mut budget
                ),
                PropertyType::Missing
            ) {
                continue;
            }
            let point = SourceSpan::new(&self.module.id, receiver.start, close.end);
            let label = crate::diagnostic::type_text::render_in(&owner, self.project);
            self.typescript_type_error(
                &point,
                "unique symbol is not a property of this receiver".into(),
                DiagnosticCode::TypeMismatch,
                7053,
                vec!["unique symbol".into(), label.clone()],
            );
            self.point_last_typescript(piece);
            let exported = self.module.declarations.iter().any(|declaration| matches!(declaration, Declaration::Variable(variable) if variable.name == key.text && variable.exported));
            let property = if exported {
                format!(
                    "[\"./{}\".{}]",
                    self.module.id.trim_end_matches(".ts"),
                    key.text
                )
            } else {
                format!("[{}]", key.text)
            };
            if let Some(counterpart) = self
                .diagnostics
                .last_mut()
                .and_then(|diagnostic| diagnostic.typescript.as_mut())
            {
                counterpart.message.push_str(&format!(
                    "\n  Property '{property}' does not exist on type '{label}'."
                ));
            }
            if let Some(diagnostic) = self.diagnostics.last_mut() {
                *diagnostic = diagnostic.clone().with_source_position(&self.module.source);
            }
        }
    }
}
