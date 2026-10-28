// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

//! Preserve lexical binding identities through runtime lowering and type elision.

use super::*;
use crate::parser::TextEdit;

mod jsx;

/// Source positions of value references resolved to a module binding. Local
/// parameters, blocks and class self names retain their lexical identities.
pub(crate) fn module_value_reference_positions(
    project: &Project,
    module: &Module,
    names: &BTreeSet<String>,
    budget: usize,
) -> BTreeSet<usize> {
    let values = names
        .iter()
        .map(|name| (name.clone(), Type::Never))
        .collect();
    let model = ScopeModel::new(
        project,
        module,
        &values,
        &BTreeMap::new(),
        None,
        None,
        budget,
    );
    model
        .references
        .iter()
        .filter(|reference| reference.meaning == Meaning::Value)
        .filter_map(|reference| {
            let root = reference.name.split('.').next()?;
            let (owner, binding) = model.resolve(reference.scope, root, Meaning::Value)?;
            (owner == 0 && binding.is_some() && names.contains(root))
                .then_some(reference.span.start)
        })
        .collect()
}

/// Erasure uses lexical binding identity, not same-spelled property/type tokens.
pub(crate) fn runtime_import_uses(
    project: &Project,
    module: &Module,
    budget: usize,
    jsx_mode: Option<crate::JsxMode>,
    jsx_factory: Option<&str>,
    jsx_fragment_factory: Option<&str>,
) -> BTreeSet<String> {
    let imported = module
        .declarations
        .iter()
        .filter_map(|declaration| {
            let Declaration::Import(import) = declaration else {
                return None;
            };
            Some(import.bindings.iter().map(|binding| binding.local.as_str()))
        })
        .flatten()
        .collect::<BTreeSet<_>>();
    if imported.is_empty() {
        return BTreeSet::new();
    }
    // Only binding identity is inspected here. An uninhabited marker seeds
    // external imports without granting a checker type or a runtime value.
    let imports = imported
        .iter()
        .map(|name| (name.to_string(), Type::Never))
        .collect();
    let model = ScopeModel::new(
        project,
        module,
        &imports,
        &BTreeMap::new(),
        None,
        None,
        budget,
    );
    let implicit = jsx::references(&model, jsx_mode, jsx_factory, jsx_fragment_factory);
    model
        .references
        .iter()
        .chain(&implicit)
        .filter(|reference| reference.meaning == Meaning::Value)
        .filter_map(|reference| {
            let root = reference.name.split('.').next()?;
            if !imported.contains(root) {
                return None;
            }
            let (owner, binding) = model.resolve(reference.scope, root, Meaning::Value)?;
            (owner == 0 && binding.is_some()).then(|| root.to_string())
        })
        .collect()
}

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
