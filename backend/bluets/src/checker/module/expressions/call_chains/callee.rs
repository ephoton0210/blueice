// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

//! Keep chain short-circuiting separate from an optional member's own type.
use super::*;

pub(super) struct Callee {
    pub value: Type,
    pub short_circuit: bool,
    pub receiver: Option<Type>,
}

impl ModuleChecker<'_> {
    pub(super) fn chain_callee(
        &self,
        tokens: &[Token],
        scope: &BTreeMap<String, Type>,
        depth: usize,
    ) -> Option<Callee> {
        if depth >= 128 {
            return None;
        }
        let inner = strip_outer_parentheses(tokens);
        if inner.len() != tokens.len() {
            let mut callee = self.chain_callee(inner, scope, depth + 1)?;
            if callee.short_circuit {
                callee.value = union([callee.value, Type::Undefined]);
            }
            callee.short_circuit = false;
            return Some(callee);
        }
        if tokens.last()?.is("!") {
            let mut callee = self.chain_callee(&tokens[..tokens.len() - 1], scope, depth + 1)?;
            callee.value = self.chain_non_nullable(&callee.value)?.0;
            return Some(callee);
        }
        if let Some(dot) = syntax::property(tokens) {
            let mut base = self.chain_callee(&tokens[..dot], scope, depth + 1)?;
            if tokens[dot].is("?.") {
                let (value, nullable) = self.chain_non_nullable(&base.value)?;
                base.value = value;
                base.short_circuit |= nullable;
            }
            let mut budget = TypeExpansionBudget::new(self.max_type_expansions);
            budget.checking = self.checking;
            let PropertyType::Found { value, .. } = property_type(
                &base.value,
                &tokens[dot + 1].text,
                &self.types,
                &mut HashSet::new(),
                &mut budget,
            ) else {
                return None;
            };
            return Some(Callee {
                value,
                short_circuit: base.short_circuit,
                receiver: Some(base.value),
            });
        }
        Some(Callee {
            value: self.infer_expression(tokens, scope),
            short_circuit: false,
            receiver: None,
        })
    }

    pub(super) fn chain_non_nullable(&self, value: &Type) -> Option<(Type, bool)> {
        let mut budget = TypeExpansionBudget::new(self.max_type_expansions);
        budget.checking = self.checking;
        let (parts, nullable) = optional_property::optional_receiver_parts(
            value,
            &self.types,
            &mut HashSet::new(),
            &mut budget,
        )
        .ok()?;
        Some((union(parts), nullable))
    }
}
