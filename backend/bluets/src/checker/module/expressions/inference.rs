// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

//! Bounded expression-result inference for BlueTS runtime forms.

use super::*;

mod arrays;
mod records;

impl<'a> ModuleChecker<'a> {
    /// The type of a top-level chain of `+`/`-` or of `*`/`/`/`%`, folded from the
    /// left, or `None` when `tokens` is not such a chain.
    #[inline(never)]
    fn infer_operator_chain(
        &self,
        tokens: &[Token],
        scope: &BTreeMap<String, Type>,
    ) -> Option<Type> {
        let generic_call =
            |start: usize| self.module.generic_call_type_arguments.contains_key(&start);
        for operators in [&["+", "-"][..], &["*", "/", "%"][..]] {
            let Some((mut rest, operator, last)) =
                top_level_binary_parts(tokens, operators, generic_call)
            else {
                continue;
            };
            let mut chain = vec![(operator, last)];
            while let Some((left, operator, operand)) =
                top_level_binary_parts(rest, operators, generic_call)
            {
                chain.push((operator, operand));
                rest = left;
            }
            let mut result = self.infer_expression(rest, scope);
            for (operator, operand) in chain.into_iter().rev() {
                let operand = self.infer_expression(operand, scope);
                result = if operators.contains(&"+") {
                    infer_additive_expression(operator, result, operand)
                } else {
                    infer_numeric_binary_expression(result, operand)
                };
            }
            return Some(result);
        }
        None
    }

    /// `yield x` is what the caller passes to `next`; `yield* g` is `g`'s result.
    /// Kept out of `infer_expression` so that function's stack frame, which every
    /// level of a long expression pays for, stays small.
    #[inline(never)]
    fn infer_yield(&self, tokens: &[Token], scope: &BTreeMap<String, Type>) -> Option<Type> {
        if !tokens.first().is_some_and(|token| token.is("yield")) {
            return None;
        }
        let context = self.generator_context.as_ref()?;
        if tokens.get(1).is_some_and(|token| token.is("*")) {
            return Some(match self.infer_expression(&tokens[2..], scope) {
                Type::Named { name, arguments } if name == "Generator" => {
                    arguments.get(1).cloned().unwrap_or(Type::Any)
                }
                _ => Type::Any,
            });
        }
        Some(context.next_type.clone())
    }

    pub(in crate::checker::module) fn infer_expression(
        &self,
        tokens: &[Token],
        scope: &BTreeMap<String, Type>,
    ) -> Type {
        if let [token] = tokens {
            if token.kind == TokenKind::Identifier || token.is("this") {
                if let Some(value) = self
                    .flow
                    .as_ref()
                    .and_then(|flow| flow.query(self.scopes.as_ref()?, token))
                {
                    return value;
                }
            }
        }
        if tokens
            .iter()
            .filter(|token| token.is("[") || token.is("{"))
            .count()
            > MAX_LITERAL_INFERENCE_CONTAINERS
        {
            return Type::Unknown;
        }
        // `<element .. />` is one operand: its token and the embedded expressions
        // the lexer follows it with as arguments.
        if tokens
            .first()
            .is_some_and(|token| token.kind == TokenKind::JsxElement)
        {
            return self.jsx_element_type();
        }
        if let Some(function) = self.nested_function_type(strip_outer_parentheses(tokens), scope) {
            return function;
        }
        if let Some(result) = self.infer_yield(tokens, scope) {
            return result;
        }
        // `await` of a `Promise<T>` is the `T`; of anything else, the operand.
        if tokens.len() > 1 && tokens[0].is("await") {
            let operand = self.infer_expression(&tokens[1..], scope);
            return super::super::binding::promise_value_type(&operand).unwrap_or(operand);
        }
        if tokens
            .iter()
            .filter(|token| INFERRED_LOGICAL_ASSIGNMENT_OPERATORS.contains(&token.text.as_str()))
            .count()
            > MAX_LOGICAL_ASSIGNMENT_INFERENCE_OPERATORS
        {
            return Type::Unknown;
        }
        if tokens
            .iter()
            .filter(|token| INFERRED_LOGICAL_EXPRESSION_OPERATORS.contains(&token.text.as_str()))
            .count()
            > MAX_LOGICAL_EXPRESSION_INFERENCE_OPERATORS
        {
            return Type::Unknown;
        }
        // Each chained member call recursively infers its receiver. Keep
        // that recursion under the compiler's existing expansion envelope.
        if tokens.iter().filter(|token| token.is(".")).count() > self.max_type_expansions {
            return Type::Unknown;
        }
        let tokens = strip_outer_parentheses(tokens);
        if let Some(value) = self
            .flow
            .as_ref()
            .and_then(|flow| flow.property(self.scopes.as_ref()?, tokens))
        {
            return value;
        }
        if let Some(call) = constructor_call_parts(tokens) {
            if let Some(signatures) = self.function_value_signatures(&call.callee.text, scope, true)
            {
                let explicit = call.generic.then(|| {
                    self.module.generic_call_type_arguments[&call.callee.start].as_slice()
                });
                return self.infer_function_call(&signatures, call.arguments, scope, explicit);
            }
            if call
                .receiver
                .is_none_or(|receiver| scope.get(&receiver.text) == self.values.get(&receiver.text))
                && self.is_bound_class_constructor_value(&call.callee.text, scope)
            {
                let Some(binding) = self.class_constructors.get(&call.callee.text) else {
                    return Type::Unknown;
                };
                let signatures = &binding.signatures;
                let explicit = call
                    .generic
                    .then(|| {
                        self.module
                            .generic_call_type_arguments
                            .get(&call.callee.start)
                            .map(Vec::as_slice)
                    })
                    .flatten();
                return self.infer_function_call(signatures, call.arguments, scope, explicit);
            }
        }
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
        if let Some(call) = member_call_parts(tokens, |start| {
            self.module.generic_call_type_arguments.contains_key(&start)
        }) {
            if let Some(signatures) =
                self.module_member_signatures(call.receiver, &call.member.text, scope)
            {
                let explicit = call.generic.then(|| {
                    self.module.generic_call_type_arguments[&call.member.start].as_slice()
                });
                return self.infer_function_call(&signatures, call.arguments, scope, explicit);
            }
            if let Some(value) =
                self.function_method_result(call.receiver, &call.member.text, call.arguments, scope)
            {
                return value;
            }
            let base = self.infer_expression(call.receiver, scope);
            let mut budget = TypeExpansionBudget::new(self.max_type_expansions);
            let found = property_type(
                &base,
                &call.member.text,
                &self.types,
                &mut HashSet::new(),
                &mut budget,
            );
            if let PropertyType::Found { value, .. } = found {
                match value {
                    Type::Function { result, .. } => return *result,
                    Type::Intersection(overloads) => {
                        if let Some(signatures) = method_overload_signatures(&overloads) {
                            return self.infer_function_call(
                                &signatures,
                                call.arguments,
                                scope,
                                None,
                            );
                        }
                    }
                    _ => {}
                }
            }
            return Type::Unknown;
        }
        if let Some(call) =
            direct_call_parts(tokens).filter(|call| split_call_arguments(call.arguments).is_some())
        {
            if let Some(signatures) =
                self.inferred_call_signatures(&call.callee.text, call.callee.start, scope)
            {
                let explicit = call.generic.then(|| {
                    self.module.generic_call_type_arguments[&call.callee.start].as_slice()
                });
                return self.infer_function_call(&signatures, call.arguments, scope, explicit);
            }
            if let Some(signatures) =
                self.function_value_signatures(&call.callee.text, scope, false)
            {
                let explicit = call.generic.then(|| {
                    self.module.generic_call_type_arguments[&call.callee.start].as_slice()
                });
                return self.infer_function_call(&signatures, call.arguments, scope, explicit);
            }
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
            // A call through a value that is not a known function type has no
            // known result.
            return Type::Unknown;
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
            return infer_logical_expression(
                self.infer_expression(left, scope),
                self.infer_expression(right, scope),
            );
        }
        if let Some((left, _, right)) = top_level_binary_parts(tokens, &["&&"], |start| {
            self.module.generic_call_type_arguments.contains_key(&start)
        }) {
            return infer_logical_expression(
                self.infer_expression(left, scope),
                self.infer_expression(right, scope),
            );
        }
        if let Some(value) = erased_assertion_operand(tokens) {
            if let Some(annotation) = tokens
                .get(value.len())
                .and_then(|token| self.module.type_assertions.get(&token.start))
            {
                let target = match annotation {
                    Type::Named { name, .. } => self
                        .types
                        .get(name)
                        .map(|definition| &definition.value)
                        .unwrap_or(annotation),
                    value => value,
                };
                if target.operator_children().is_some()
                    || matches!(target, Type::KeyOf(_) | Type::IndexedAccess { .. })
                {
                    return annotation.clone();
                }
            }
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
        // A long chain `a + b + c + ..` is folded left to right here rather than
        // by recursing on its left side, which would be as deep as the chain is
        // long (and, at every level, in this function's large stack frame).
        if let Some(result) = self.infer_operator_chain(tokens, scope) {
            return result;
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
        // A literal only stands for the whole expression when nothing follows
        // it: `"x".length` is a member read, not a string.
        if (first.kind == TokenKind::String || first.kind == TokenKind::Template)
            && tokens.len() == 1
        {
            return Type::String;
        }
        if first.kind == TokenKind::Number && tokens.len() == 1 {
            return Type::Number;
        }
        if let Some(value) = self.infer_optional_chain(tokens, scope) {
            return value;
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
            if property.is_none() && tokens.last().is_some_and(|token| token.is("]")) {
                let inside = &tokens[receiver.len() + 1..tokens.len() - 1];
                if let Some(result) = self.enum_index_type(&owner, inside, scope) {
                    return result;
                }
            }
            if property.is_none() || index.is_some() {
                // A computed receiver may still have a known element type.
                // Retaining it lets a later `.readonlyField` write reach the
                // same property policy as a direct receiver. A literal index
                // also selects the exact member of a heterogeneous tuple.
                let mut budget = TypeExpansionBudget::new(self.max_type_expansions);
                budget.checking = self.checking;
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
        if (first.is("this") || first.is("super")) && tokens.len() == 1 {
            return scope.get(&first.text).cloned().unwrap_or(Type::Unknown);
        }
        match first.text.as_str() {
            "true" | "false" => Type::Boolean,
            "null" => Type::Null,
            "undefined" => Type::Undefined,
            "[" => self.infer_array(tokens, scope),
            "{" => self.infer_record(tokens, scope),
            _ if first.kind == TokenKind::Identifier => {
                // A value in scope (a parameter, a local, a catch binding)
                // shadows a module function of the same name.
                if tokens.len() == 1 {
                    if let Some(value @ Type::Intersection(parts)) = scope.get(&first.text) {
                        if parts.iter().any(|part| matches!(part, Type::Record(_))) {
                            return value.clone();
                        }
                    }
                    if let Some(signature) =
                        self.functions.get(&first.text).and_then(|set| set.first())
                    {
                        return super::super::binding::declared_function_type(
                            &signature.parameters,
                            signature.return_type.clone(),
                            &signature.type_parameters,
                        );
                    }
                }
                scope.get(&first.text).cloned().unwrap_or(Type::Unknown)
            }
            _ => Type::Unknown,
        }
    }
}
