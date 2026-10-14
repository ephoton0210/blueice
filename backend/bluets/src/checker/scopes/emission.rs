// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

//! Preserve the checker's lexical binding identities when ES5 hoists variables.

use super::*;
use crate::parser::TextEdit;

#[derive(Default)]
pub(crate) struct LexicalEmission {
    pub(crate) edits: Vec<TextEdit>,
    pub(crate) captured_loop_bindings: BTreeSet<String>,
}

pub(crate) fn lexical_emission(project: &Project, module: &Module) -> LexicalEmission {
    let model = ScopeModel::new(
        project,
        module,
        &BTreeMap::new(),
        &BTreeMap::new(),
        None,
        None,
        128,
    );
    model.lexical_emission()
}

impl ScopeModel<'_> {
    fn lexical_emission(&self) -> LexicalEmission {
        let mut output = LexicalEmission::default();
        let mut names = BTreeMap::new();
        let mut sequence = 0;
        let mut declarations = self.declaration_names.clone();
        for (scope, name) in self.catch_bindings.keys() {
            let span = &self.scopes[*scope].span;
            let start = self
                .tokens
                .partition_point(|token| token.start < span.start);
            if self
                .tokens
                .get(start)
                .is_some_and(|token| token.is("catch"))
                && self
                    .tokens
                    .get(start + 1)
                    .is_some_and(|token| token.is("("))
                && self
                    .tokens
                    .get(start + 2)
                    .is_some_and(|token| token.text == *name)
            {
                declarations.insert(self.tokens[start + 2].start);
            }
        }
        for offset in &declarations {
            let index = self.tokens.partition_point(|token| token.start < *offset);
            let Some(token) = self.tokens.get(index) else {
                continue;
            };
            let scope = self.scope_at(token.start);
            let Some((owner, Some(binding))) = self.resolve(scope, &token.text, Meaning::Value)
            else {
                continue;
            };
            if self.scopes[owner].var_boundary
                || !matches!(binding.kind, BindingKind::Mutable | BindingKind::Const)
                || self.parameters.contains_key(&(owner, token.text.clone()))
            {
                continue;
            }
            let name = names
                .entry((owner, token.text.clone()))
                .or_insert_with(|| loop {
                    let name = format!("__blueice_target_binding_{sequence}");
                    sequence += 1;
                    if !self.module.source.contains(&name) {
                        break name;
                    }
                });
            output.edits.push(TextEdit {
                start: token.start,
                end: token.end,
                replacement: name.clone(),
            });
        }
        for reference in &self.references {
            if reference.meaning != Meaning::Value {
                continue;
            }
            let Some((owner, _)) = self.resolve(reference.scope, &reference.name, Meaning::Value)
            else {
                continue;
            };
            let Some(name) = names.get(&(owner, reference.name.clone())) else {
                continue;
            };
            output.edits.push(TextEdit {
                start: reference.span.start,
                end: reference.span.end,
                replacement: name.clone(),
            });
            if self.scopes[owner].execution != self.scopes[reference.scope].execution {
                let span = &self.scopes[owner].span;
                let index = self
                    .tokens
                    .partition_point(|token| token.start < span.start);
                if self.tokens.get(index).is_some_and(|token| token.is("for")) {
                    output.captured_loop_bindings.insert(name.clone());
                }
            }
        }
        output
    }
}
