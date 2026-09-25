// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

//! Bounded expression-result inference for BlueTS runtime forms.

use super::*;

impl<'a> ModuleChecker<'a> {
    pub(in crate::checker::module) fn infer_expression(
        &self,
        tokens: &[Token],
        scope: &BTreeMap<String, Type>,
    ) -> Type {
        if tokens
            .iter()
            .filter(|token| token.is("[") || token.is("{"))
            .count()
            > MAX_LITERAL_INFERENCE_CONTAINERS
        {
            return Type::Unknown;
        }
        if tokens
            .iter()
            .filter(|token| INFERRED_LOGICAL_ASSIGNMENT_OPERATORS.contains(&token.text.as_str()))
            .count()
            > MAX_LOGICAL_ASSIGNMENT_INFERENCE_OPERATORS
        {
            return Type::Unknown;
        }
        // Each chained member call recursively infers its receiver. Keep
        // that recursion under the compiler's existing expansion envelope.
        if tokens.iter().filter(|token| token.is(".")).count() > self.max_type_expansions {
            return Type::Unknown;
        }
        let tokens = strip_outer_parentheses(tokens);
        if let Some((_, _, result)) = top_level_binary_parts(tokens, &[","], |start| {
            self.module.generic_call_type_arguments.contains_key(&start)
        }) {
            // A sequence evaluates every operand but yields its final value.
            return self.infer_expression(result, scope);
        }
        if let Some((_, _, assigned)) = top_level_binary_parts(tokens, &["="], |_| false) {
            // Simple assignment yields the value written to its target.
            return self.infer_expression(assigned, scope);
        }
        if let Some((target, operator, assigned)) =
            top_level_binary_parts(tokens, &["&&=", "||=", "??="], |_| false)
        {
            let before = self.infer_expression(target, scope);
            let after = self.infer_expression(assigned, scope);
            return if operator.is("??=") {
                infer_nullish_coalescing_expression(before, after)
            } else {
                merge_conditional_branch_types(before, after)
            };
        }
        if tokens.len() > 1 && tokens.last().is_some_and(|token| token.is("!")) {
            let inferred = self.infer_expression(&tokens[..tokens.len() - 1], scope);
            return match inferred {
                Type::Union(options) => {
                    let mut retained = options
                        .into_iter()
                        .filter(|option| !matches!(option, Type::Null | Type::Undefined))
                        .collect::<Vec<_>>();
                    match retained.len() {
                        0 => Type::Never,
                        1 => retained.pop().expect("one retained non-null type"),
                        _ => Type::Union(retained),
                    }
                }
                Type::Null | Type::Undefined => Type::Never,
                value => value,
            };
        }
        if tokens
            .first()
            .is_some_and(|token| token.is("++") || token.is("--"))
            || tokens
                .last()
                .is_some_and(|token| token.is("++") || token.is("--"))
        {
            return Type::Number;
        }
        if let Some(call) = member_call_parts(tokens) {
            let base = self.infer_expression(call.receiver, scope);
            let mut budget = TypeExpansionBudget::new(self.max_type_expansions);
            if let PropertyType::Found {
                value: Type::Function { result, .. },
                ..
            } = property_type(
                &base,
                &call.member.text,
                &self.types,
                &mut HashSet::new(),
                &mut budget,
            ) {
                return *result;
            }
            return Type::Unknown;
        }
        if let Some(call) =
            direct_call_parts(tokens).filter(|call| split_call_arguments(call.arguments).is_some())
        {
            if let Some(signatures) = self.functions.get(&call.callee.text) {
                let explicit = call.generic.then(|| {
                    self.module
                        .generic_call_type_arguments
                        .get(&call.callee.start)
                        .expect("parsed generic call has recorded type arguments")
                        .as_slice()
                });
                return self.infer_function_call(signatures, call.arguments, scope, explicit);
            }
            return scope
                .get(&call.callee.text)
                .cloned()
                .unwrap_or(Type::Unknown);
        }
        if let Some((_, consequent, alternate)) = conditional_expression_parts(tokens) {
            return merge_conditional_branch_types(
                self.infer_expression(consequent, scope),
                self.infer_expression(alternate, scope),
            );
        }
        if let Some((left, _, right)) = top_level_binary_parts(tokens, &["??"], |start| {
            self.module.generic_call_type_arguments.contains_key(&start)
        }) {
            return infer_nullish_coalescing_expression(
                self.infer_expression(left, scope),
                self.infer_expression(right, scope),
            );
        }
        if let Some((left, _, right)) = top_level_binary_parts(tokens, &["||"], |start| {
            self.module.generic_call_type_arguments.contains_key(&start)
        }) {
            return infer_boolean_logical_expression(
                self.infer_expression(left, scope),
                self.infer_expression(right, scope),
            );
        }
        if let Some((left, _, right)) = top_level_binary_parts(tokens, &["&&"], |start| {
            self.module.generic_call_type_arguments.contains_key(&start)
        }) {
            return infer_boolean_logical_expression(
                self.infer_expression(left, scope),
                self.infer_expression(right, scope),
            );
        }
        if let Some(value) = erased_assertion_operand(tokens) {
            // Both forms disappear from emitted JavaScript. Keep the known
            // runtime receiver type, including readonly host qualifiers,
            // rather than accidentally inferring its first identifier.
            return self.infer_expression(value, scope);
        }
        for operators in [&["|"][..], &["^"][..], &["&"][..]] {
            if let Some((left, _, right)) = top_level_binary_parts(tokens, operators, |start| {
                self.module.generic_call_type_arguments.contains_key(&start)
            }) {
                return infer_numeric_binary_expression(
                    self.infer_expression(left, scope),
                    self.infer_expression(right, scope),
                );
            }
        }
        if top_level_binary_parts(
            tokens,
            &[
                "===",
                "!==",
                "==",
                "!=",
                "<",
                ">",
                "<=",
                ">=",
                "in",
                "instanceof",
            ],
            |start| self.module.generic_call_type_arguments.contains_key(&start),
        )
        .is_some()
        {
            return Type::Boolean;
        }
        if let Some((left, _, right)) = top_level_shift_parts(tokens, |start| {
            self.module.generic_call_type_arguments.contains_key(&start)
        }) {
            return infer_numeric_binary_expression(
                self.infer_expression(left, scope),
                self.infer_expression(right, scope),
            );
        }
        if let Some((left, operator, right)) =
            top_level_binary_parts(tokens, &["+", "-"], |start| {
                self.module.generic_call_type_arguments.contains_key(&start)
            })
        {
            return infer_additive_expression(
                operator,
                self.infer_expression(left, scope),
                self.infer_expression(right, scope),
            );
        }
        if let Some((left, _, right)) = top_level_binary_parts(tokens, &["*", "/", "%"], |start| {
            self.module.generic_call_type_arguments.contains_key(&start)
        }) {
            return infer_numeric_binary_expression(
                self.infer_expression(left, scope),
                self.infer_expression(right, scope),
            );
        }
        if let Some((left, _, right)) = top_level_binary_parts(tokens, &["**"], |start| {
            self.module.generic_call_type_arguments.contains_key(&start)
        }) {
            return infer_numeric_binary_expression(
                self.infer_expression(left, scope),
                self.infer_expression(right, scope),
            );
        }
        let Some(first) = tokens.first() else {
            return Type::Undefined;
        };
        if first.is("typeof") {
            return Type::String;
        }
        if first.is("void") {
            return Type::Undefined;
        }
        if matches!(first.text.as_str(), "!") {
            return Type::Boolean;
        }
        if matches!(first.text.as_str(), "+" | "-" | "~") {
            return Type::Number;
        }
        if first.kind == TokenKind::String || first.kind == TokenKind::Template {
            return Type::String;
        }
        if first.kind == TokenKind::Number {
            return Type::Number;
        }
        if let Some((receiver, property)) = member_access_target(tokens) {
            if tokens.iter().filter(|token| token.is("[")).count() > self.max_type_expansions {
                return Type::Unknown;
            }
            let owner = self.infer_expression(receiver, scope);
            let index = tokens
                .last()
                .is_some_and(|token| token.is("]"))
                .then(|| canonical_index_key(&tokens[receiver.len() + 1..tokens.len() - 1]))
                .flatten();
            if property.is_none() || index.is_some() {
                // A computed receiver may still have a known element type.
                // Retaining it lets a later `.readonlyField` write reach the
                // same property policy as a direct receiver. A literal index
                // also selects the exact member of a heterogeneous tuple.
                let mut budget = TypeExpansionBudget::new(self.max_type_expansions);
                let indexed = indexed_value_type(
                    &owner,
                    index,
                    &self.types,
                    &mut HashSet::new(),
                    &mut budget,
                );
                if property.is_none() || indexed != Type::Unknown {
                    return indexed;
                }
            }
            let Some(property) = property else {
                return Type::Unknown;
            };
            let mut budget = TypeExpansionBudget::new(self.max_type_expansions);
            return match property_type(
                &owner,
                property,
                &self.types,
                &mut HashSet::new(),
                &mut budget,
            ) {
                PropertyType::Found { value, .. } => value,
                PropertyType::Missing | PropertyType::Indeterminate | PropertyType::Exhausted => {
                    Type::Unknown
                }
            };
        }
        match first.text.as_str() {
            "true" | "false" => Type::Boolean,
            "null" => Type::Null,
            "undefined" => Type::Undefined,
            "[" => infer_array(tokens, &|value| self.infer_expression(value, scope)),
            "{" => infer_record(tokens, scope, &|value| self.infer_expression(value, scope)),
            _ if first.kind == TokenKind::Identifier => {
                if tokens.len() == 1 {
                    if let Some(signature) =
                        self.functions.get(&first.text).and_then(|set| set.first())
                    {
                        if signature.type_parameters.is_empty() {
                            return Type::Function {
                                parameters: signature.parameters.clone(),
                                result: Box::new(signature.return_type.clone()),
                            };
                        }
                    }
                }
                scope.get(&first.text).cloned().unwrap_or(Type::Unknown)
            }
            _ => Type::Unknown,
        }
    }
}
