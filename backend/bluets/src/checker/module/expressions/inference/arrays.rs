// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

//! Array literal result inference, including referenced spread elements.

use super::*;

impl<'a> ModuleChecker<'a> {
    /// The type of `tokens` where an expression of type `expected` is
    /// required. A tuple literal takes its element positions from a tuple
    /// context instead of widening to an array, so `[1, "a"]` can be a
    /// `[number, string]`; every other expression is inferred as usual.
    pub(in crate::checker::module) fn infer_in_context(
        &self,
        tokens: &[Token],
        scope: &BTreeMap<String, Type>,
        expected: &Type,
    ) -> Type {
        if let Some(literal) = self.enum_literal_for(tokens, expected) {
            return literal;
        }
        let mut contextual = expected.clone();
        let mut budget = TypeExpansionBudget::new(self.max_type_expansions);
        let mut visited = HashSet::new();
        while let Some(expanded) = instantiate_named(
            &contextual,
            &self.types,
            &mut visited,
            &mut budget,
            "contextual tuple",
        ) {
            contextual = expanded;
        }
        match &contextual {
            Type::Literal(expected) => {
                if let [literal] = strip_outer_parentheses(tokens) {
                    if literal.kind == TokenKind::String {
                        let expected_token = Token {
                            kind: TokenKind::String,
                            text: expected.clone(),
                            start: 0,
                            end: expected.len(),
                        };
                        if crate::syntax::string_contents(literal)
                            == crate::syntax::string_contents(&expected_token)
                        {
                            return Type::Literal(expected.clone());
                        }
                        return Type::Literal(literal.text.clone());
                    }
                    if literal.kind == TokenKind::Number
                        || literal.is("true")
                        || literal.is("false")
                    {
                        return Type::Literal(literal.text.clone());
                    }
                }
            }
            Type::Tuple(elements) => {
                if let Some(literal) = self.infer_contextual_tuple_literal(tokens, scope, elements)
                {
                    return literal;
                }
            }
            Type::Record(fields)
                if tokens.first().is_some_and(|token| token.is("{"))
                    && tokens.last().is_some_and(|token| token.is("}")) =>
            {
                return self.infer_record_in_context(tokens, scope, Some(fields));
            }
            Type::Array(item) => {
                if let Some(literal) = self.infer_contextual_array_literal(tokens, scope, item) {
                    return literal;
                }
            }
            Type::Union(options) => {
                // The first member of the union the literal fits, read against it.
                for option in options {
                    let candidate = self.infer_in_context(tokens, scope, option);
                    let mut budget = TypeExpansionBudget::new(self.max_type_expansions);
                    if is_assignable(
                        &candidate,
                        option,
                        &self.types,
                        &mut HashSet::new(),
                        &mut budget,
                    ) {
                        return candidate;
                    }
                }
            }
            _ => {}
        }
        self.infer_expression(tokens, scope)
    }

    /// The comma-separated pieces of a bracketed literal, or `None` when the
    /// tokens are not one.
    fn literal_elements(tokens: &[Token]) -> Option<Vec<&[Token]>> {
        if tokens.first().is_none_or(|token| !token.is("["))
            || tokens.last().is_none_or(|token| !token.is("]"))
        {
            return None;
        }
        let mut pieces: Vec<&[Token]> = Vec::new();
        let mut start = 1usize;
        let mut depth = 0usize;
        for index in 1..tokens.len() {
            match tokens[index].text.as_str() {
                "[" | "(" | "{" => depth += 1,
                "]" | ")" | "}" if depth > 0 => depth -= 1,
                "," if depth == 0 => {
                    pieces.push(&tokens[start..index]);
                    start = index + 1;
                }
                _ => {}
            }
        }
        if start + 1 < tokens.len() {
            pieces.push(&tokens[start..tokens.len() - 1]);
        }
        Some(pieces)
    }

    /// A bracketed literal against an array context: each element is read
    /// against the element type, so `[[1, "a"]]` is an array of tuples where the
    /// context says so. `None` for an empty literal, a spread, or anything that
    /// is not a plain literal.
    fn infer_contextual_array_literal(
        &self,
        tokens: &[Token],
        scope: &BTreeMap<String, Type>,
        item: &Type,
    ) -> Option<Type> {
        let pieces = Self::literal_elements(tokens)?;
        if pieces.is_empty()
            || pieces
                .iter()
                .any(|piece| piece.is_empty() || piece.first().is_some_and(|token| token.is("...")))
        {
            return None;
        }
        let mut types: Vec<Type> = Vec::new();
        for piece in pieces {
            let element = self.infer_in_context(piece, scope, item);
            if !types.contains(&element) {
                types.push(element);
            }
        }
        Some(Type::Array(Box::new(if types.len() == 1 {
            types.remove(0)
        } else {
            Type::Union(types)
        })))
    }

    /// A bracketed literal as a tuple: each element is inferred against the
    /// tuple element at the same position while positions are still known, and
    /// a spread of a tuple contributes its elements, optional flags included.
    /// `None` when the literal is not a plain tuple shape.
    fn infer_contextual_tuple_literal(
        &self,
        tokens: &[Token],
        scope: &BTreeMap<String, Type>,
        expected: &[TupleTypeElement],
    ) -> Option<Type> {
        if tokens.first().is_none_or(|token| !token.is("["))
            || tokens.last().is_none_or(|token| !token.is("]"))
        {
            return None;
        }
        let mut pieces: Vec<&[Token]> = Vec::new();
        let mut start = 1usize;
        let mut depth = 0usize;
        for index in 1..tokens.len() {
            match tokens[index].text.as_str() {
                "[" | "(" | "{" => depth += 1,
                "]" | ")" | "}" if depth > 0 => depth -= 1,
                "," if depth == 0 => {
                    pieces.push(&tokens[start..index]);
                    start = index + 1;
                }
                _ => {}
            }
        }
        if start + 1 < tokens.len() {
            pieces.push(&tokens[start..tokens.len() - 1]);
        }
        let mut values: Vec<TupleTypeElement> = Vec::new();
        // The expected element at the current position, until a spread makes
        // later positions depend on a length that is not fixed.
        let mut positions_known = true;
        for piece in pieces {
            if piece.is_empty() {
                return None;
            }
            if piece.first().is_some_and(|token| token.is("...")) {
                positions_known = false;
                let Type::Tuple(spread) = self.infer_expression(&piece[1..], scope) else {
                    return None;
                };
                if spread.iter().any(|element| element.rest) {
                    return None;
                }
                // A required element after an optional one would need a
                // length TypeScript does not model as a tuple.
                for element in spread {
                    if values.last().is_some_and(|last| last.optional) && !element.optional {
                        return None;
                    }
                    values.push(TupleTypeElement {
                        label: None,
                        ..element
                    });
                }
                continue;
            }
            if values.last().is_some_and(|last| last.optional) {
                return None;
            }
            let context = positions_known
                .then(|| expected.get(values.len()))
                .flatten()
                .map(|element| match (&element.annotation, element.rest) {
                    (Type::Array(item), true) => (**item).clone(),
                    (annotation, _) => annotation.clone(),
                });
            let element = match (&context, piece) {
                (Some(context), _) => self.infer_in_context(piece, scope, context),
                (None, [single]) => infer_simple(std::slice::from_ref(single), scope),
                (None, _) => self.infer_expression(piece, scope),
            };
            values.push(TupleTypeElement::required(element));
        }
        Some(Type::Tuple(values))
    }

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
