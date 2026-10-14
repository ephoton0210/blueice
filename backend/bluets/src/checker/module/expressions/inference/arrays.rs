// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

//! Array literal result inference, including referenced spread elements.

use super::*;

impl<'a> ModuleChecker<'a> {
    pub(super) fn infer_array(&self, tokens: &[Token], scope: &BTreeMap<String, Type>) -> Type {
        let mut values = Vec::new();
        let mut start = 1usize;
        let mut depth = 0usize;
        for index in 1..tokens.len() {
            match tokens[index].text.as_str() {
                "[" | "(" | "{" => depth += 1,
                "]" | ")" | "}" if depth > 0 => depth -= 1,
                "," if depth == 0 => {
                    if start < index {
                        values.push(self.infer_array_element(&tokens[start..index], scope));
                    }
                    start = index + 1;
                }
                _ => {}
            }
        }
        if start + 1 < tokens.len() {
            values.push(self.infer_array_element(&tokens[start..tokens.len() - 1], scope));
        }
        let Some(first) = values.first().cloned() else {
            return Type::Array(Box::new(Type::Unknown));
        };
        if values.iter().all(|value| value == &first) {
            Type::Array(Box::new(first))
        } else {
            Type::Array(Box::new(Type::Union(values)))
        }
    }

    fn infer_array_element(&self, tokens: &[Token], scope: &BTreeMap<String, Type>) -> Type {
        if tokens.first().is_some_and(|token| token.is("...")) {
            let spread = self.infer_expression(&tokens[1..], scope);
            let mut budget = TypeExpansionBudget::new(self.max_type_expansions);
            return indexed_value_type(
                &spread,
                None,
                &self.types,
                &mut HashSet::new(),
                &mut budget,
            );
        }
        self.infer_expression(tokens, scope)
    }
}
