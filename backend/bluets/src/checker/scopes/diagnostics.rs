// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

//! Diagnostic codes derived from lexical meaning and binding identity.

use super::*;

impl ScopeModel<'_> {
    pub(super) fn diagnostic_counterpart(
        &self,
        reference: &Reference,
        diagnostic: Diagnostic,
    ) -> Diagnostic {
        let root = reference.name.split('.').next().unwrap_or(&reference.name);
        let binding = self
            .resolve(reference.scope, root, Meaning::Query)
            .and_then(|(_, binding)| binding);
        if reference.meaning == Meaning::Type {
            if let Some((namespace, member)) = reference.name.rsplit_once('.').filter(|_| {
                binding.is_some() || self.resolve(reference.scope, root, Meaning::Type).is_some()
            }) {
                let is_value = binding.is_some_and(|binding| {
                    let mut budget = TypeExpansionBudget::new(self.max_type_expansions);
                    matches!(
                        property_type(
                            &binding.declared_type,
                            member,
                            &self.type_definitions,
                            &mut HashSet::new(),
                            &mut budget,
                        ),
                        PropertyType::Found { .. }
                    )
                });
                return if is_value {
                    diagnostic.with_typescript(2749, vec![reference.name.clone()])
                } else {
                    diagnostic.with_typescript(2694, vec![namespace.into(), member.into()])
                };
            }
            if binding.is_some_and(|binding| {
                matches!(
                    binding.kind,
                    BindingKind::Namespace | BindingKind::NamespaceImport
                )
            }) {
                return diagnostic.with_typescript(2709, vec![root.into()]);
            }
            if binding.is_some_and(|binding| !binding.type_only) {
                return diagnostic.with_typescript(2749, vec![root.into()]);
            }
            return diagnostic;
        }
        if diagnostic.code == DiagnosticCode::UsedBeforeDeclaration {
            if binding.is_some() {
                if self
                    .tokens
                    .iter()
                    .position(|token| token.start == reference.span.start)
                    .and_then(|index| index.checked_sub(1))
                    .is_some_and(|index| self.tokens[index].is("of") || self.tokens[index].is("in"))
                {
                    return diagnostic.with_typescript(7022, vec![root.into()]);
                }
                if self
                    .parameters
                    .contains_key(&(reference.scope, root.into()))
                {
                    if let Some(((_, name), _)) =
                        self.parameters.iter().find(|((scope, _), span)| {
                            *scope == reference.scope
                                && span.start <= reference.span.start
                                && reference.span.end <= span.end
                        })
                    {
                        return diagnostic.with_typescript(2373, vec![name.clone(), root.into()]);
                    }
                }
            }
            return diagnostic;
        }
        if binding
            .is_some_and(|binding| binding.type_only && binding.kind == BindingKind::Namespace)
        {
            return diagnostic.with_typescript(2708, vec![root.into()]);
        }
        if binding.is_none() {
            if root == "document" {
                return diagnostic.with_typescript(2584, vec![root.into()]);
            }
            if self.resolve(reference.scope, root, Meaning::Type).is_some()
                || matches!(
                    root,
                    "any"
                        | "unknown"
                        | "never"
                        | "number"
                        | "string"
                        | "boolean"
                        | "symbol"
                        | "bigint"
                        | "object"
                )
            {
                return diagnostic.with_typescript(2693, vec![root.into()]);
            }
            if let Some((_, Some(receiver))) = self.resolve(reference.scope, "this", Meaning::Value)
            {
                let mut budget = TypeExpansionBudget::new(self.max_type_expansions);
                if matches!(
                    property_type(
                        &receiver.declared_type,
                        root,
                        &self.type_definitions,
                        &mut HashSet::new(),
                        &mut budget,
                    ),
                    PropertyType::Found { .. }
                ) {
                    return diagnostic.with_typescript(2663, vec![root.into()]);
                }
            }
            if let Some(index) = self
                .tokens
                .iter()
                .position(|token| token.start == reference.span.start)
            {
                if index > 0
                    && matches!(self.tokens[index - 1].text.as_str(), "{" | ",")
                    && self
                        .tokens
                        .get(index + 1)
                        .is_some_and(|token| token.is(",") || token.is("}"))
                {
                    return diagnostic.with_typescript(18004, vec![root.into()]);
                }
            }
            let mut scope = Some(reference.scope);
            let mut candidates = BTreeSet::new();
            while let Some(index) = scope {
                candidates.extend(
                    self.scopes[index]
                        .values
                        .keys()
                        .filter(|name| name.as_str() != root && name.eq_ignore_ascii_case(root))
                        .cloned(),
                );
                scope = self.scopes[index].parent;
            }
            if candidates.len() == 1 {
                return diagnostic
                    .with_typescript(2552, vec![root.into(), candidates.pop_first().unwrap()]);
            }
        }
        diagnostic
    }
}
