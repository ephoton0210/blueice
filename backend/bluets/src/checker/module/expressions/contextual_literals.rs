// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

//! Primitive index constraints retain fresh property literals, not const arrays.
use super::*;

impl ModuleChecker<'_> {
    pub(super) fn const_generic_argument(
        &self,
        tokens: &[Token],
        scope: &BTreeMap<String, Type>,
        parameter: &Parameter,
        signature: &FunctionSignature,
    ) -> Option<Type> {
        let Type::Named { name, arguments } = parameter.annotation.as_ref()? else {
            return None;
        };
        if !arguments.is_empty() {
            return None;
        }
        let binder = signature
            .type_parameters
            .iter()
            .find(|binder| binder.name == *name && binder.is_const)?;
        let tokens = strip_outer_parentheses(tokens);
        let first = tokens.first()?;
        if !(first.is("{")
            || first.is("[")
            || matches!(
                first.kind,
                TokenKind::String | TokenKind::Number | TokenKind::Template
            )
            || first.is("true")
            || first.is("false"))
        {
            return None;
        }
        let value = self.infer_const_assertion(tokens, scope);
        if !parameter.rest {
            if let Some(constraint) = &binder.constraint {
                let mut budget = TypeExpansionBudget::new(self.max_type_expansions);
                budget.checking = self.checking;
                if !is_assignable(
                    &value,
                    constraint,
                    &self.types,
                    &mut HashSet::new(),
                    &mut budget,
                ) {
                    return None;
                }
            }
        }
        Some(value)
    }

    pub(super) fn mapped_sequence_argument(
        &self,
        tokens: &[Token],
        scope: &BTreeMap<String, Type>,
        annotation: &Type,
        signature: &FunctionSignature,
    ) -> Option<Type> {
        let mut template = annotation.clone();
        let mut budget = TypeExpansionBudget::new(self.max_type_expansions);
        let mut visited = HashSet::new();
        while let Some(value) = instantiate_named(
            &template,
            &self.types,
            &mut visited,
            &mut budget,
            "mapped sequence context",
        ) {
            template = value;
            if matches!(template, Type::Mapped(_)) {
                break;
            }
        }
        let Type::Mapped(mapped) = template else {
            return None;
        };
        let Type::KeyOf(object) = mapped.parameter.constraint.as_ref()? else {
            return None;
        };
        let Type::Named { name, arguments } = object.as_ref() else {
            return None;
        };
        if !arguments.is_empty() || mapped.name_type.is_some() {
            return None;
        }
        let constraint = signature
            .type_parameters
            .iter()
            .find(|p| p.name == *name)?
            .constraint
            .as_ref()?;
        if !matches!(constraint, Type::Array(_) | Type::Readonly(_)) {
            return None;
        }
        let values = Self::literal_elements(tokens)?
            .iter()
            .map(|piece| {
                TupleTypeElement::required(self.array_element_type(
                    piece,
                    scope,
                    self.infer_expression(piece, scope),
                ))
            })
            .collect();
        Some(Type::Tuple(values))
    }

    pub(super) fn constrained_record_argument(
        &self,
        tokens: &[Token],
        scope: &BTreeMap<String, Type>,
        parameter: &Parameter,
        signature: &FunctionSignature,
    ) -> Option<Type> {
        if !tokens.first()?.is("{") {
            return None;
        }
        let Type::Named { name, arguments } = parameter.annotation.as_ref()? else {
            return None;
        };
        if !arguments.is_empty() {
            return None;
        }
        let constraint = signature
            .type_parameters
            .iter()
            .find(|p| p.name == *name)?
            .constraint
            .as_ref()?;
        let substitutions = signature
            .type_parameters
            .iter()
            .filter_map(|p| {
                p.constraint
                    .as_ref()
                    .map(|value| (p.name.clone(), value.clone()))
            })
            .collect();
        let mut budget = TypeExpansionBudget::new(self.max_type_expansions);
        let constraint = crate::checker::type_operators::expanded(
            &substitute_type(constraint, &substitutions),
            &self.types,
            &mut HashSet::new(),
            &mut budget,
        );
        let Type::IndexedRecord { indices, .. } = constraint else {
            return None;
        };
        fn primitive(value: &Type) -> bool {
            match value {
                Type::String | Type::Number | Type::Boolean | Type::BigInt | Type::Symbol => true,
                Type::Union(parts) => parts.iter().any(primitive),
                _ => false,
            }
        }
        if !indices.iter().any(|index| primitive(&index.value)) {
            return None;
        }
        let Type::Record(mut fields) = self.infer_expression(tokens, scope) else {
            return None;
        };
        for field in &mut fields {
            let piece: Vec<_> = tokens
                .iter()
                .filter(|token| field.span.start <= token.start && token.end <= field.span.end)
                .collect();
            if let [_, colon, literal] = piece.as_slice() {
                if colon.is(":")
                    && (matches!(literal.kind, TokenKind::String | TokenKind::Number)
                        || literal.is("true")
                        || literal.is("false"))
                {
                    field.value = Type::Literal(literal.text.clone());
                }
            }
        }
        Some(Type::Record(fields))
    }
}
