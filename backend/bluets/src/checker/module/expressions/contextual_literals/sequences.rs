// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

//! Retain source positions before homomorphic sequence candidates are widened.
use super::*;

impl ModuleChecker<'_> {
    pub(in crate::checker::module::expressions) fn mapped_sequence_argument(
        &self,
        tokens: &[Token],
        scope: &BTreeMap<String, Type>,
        annotation: &Type,
        signature: &FunctionSignature,
        budget: &mut TypeExpansionBudget,
    ) -> Option<Type> {
        let result = self.mapped_sequence_context(tokens, scope, annotation, signature, budget);
        if budget.exhausted {
            if let (Some(first), Some(last)) = (tokens.first(), tokens.last()) {
                self.generic_inference_failure.set(Some(SourceSpan::new(
                    &self.module.id,
                    first.start,
                    last.end,
                )));
            }
            return None;
        }
        result
    }

    fn mapped_sequence_context(
        &self,
        tokens: &[Token],
        scope: &BTreeMap<String, Type>,
        annotation: &Type,
        signature: &FunctionSignature,
        budget: &mut TypeExpansionBudget,
    ) -> Option<Type> {
        let pieces = Self::literal_elements(tokens)?;
        let mut template = annotation.clone();
        let mut visited = HashSet::new();
        while let Some(value) = instantiate_named(
            &template,
            &self.types,
            &mut visited,
            budget,
            "mapped sequence context",
        ) {
            template = value;
            if matches!(template, Type::Mapped(_)) {
                break;
            }
        }
        let Type::Mapped(mapped) = &template else {
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
        let parameter = signature
            .type_parameters
            .iter()
            .find(|parameter| parameter.name == *name)?;
        if let Some(constraint) = &parameter.constraint {
            let constraint = crate::checker::type_operators::expanded(
                constraint,
                &self.types,
                &mut HashSet::new(),
                budget,
            );
            let sequence = |value: &Type| matches!(value, Type::Array(_) | Type::Tuple(_));
            if !sequence(&constraint)
                && !matches!(&constraint, Type::Readonly(value) if sequence(value))
                && !matches!(constraint, Type::Any | Type::Unknown | Type::StrictUnknown)
            {
                return None;
            }
        }
        let mut actual = self.mapped_literal_shape(&pieces, scope, None, budget)?;
        // The first pass supplies nested tuple/callback structure even when a
        // wrapped slot cannot yet contribute evidence. Only the real argument
        // shape is returned; provisional unknowns never become call candidates.
        for _ in 0..2 {
            let inferred = crate::checker::inference::reverse_mapped_object(
                mapped,
                &actual,
                &self.types,
                budget,
            );
            if budget.exhausted {
                return None;
            }
            let object = inferred.unwrap_or_else(|| sequence_placeholder(&actual));
            let context = crate::checker::inference::result_type(
                &template,
                &BTreeMap::from([(name.clone(), object)]),
                &self.types,
                budget,
            );
            actual = self.mapped_literal_shape(&pieces, scope, Some(&context), budget)?;
        }
        Some(actual)
    }

    fn mapped_literal_shape(
        &self,
        pieces: &[&[Token]],
        scope: &BTreeMap<String, Type>,
        context: Option<&Type>,
        budget: &mut TypeExpansionBudget,
    ) -> Option<Type> {
        let context = match context {
            Some(Type::Readonly(value)) => Some(value.as_ref()),
            value => value,
        };
        let mut values = Vec::new();
        for piece in pieces {
            if !budget.consume() {
                return None;
            }
            if piece.first().is_some_and(|token| token.is("...")) {
                let spread = self.infer_expression(&piece[1..], scope);
                let spread = match spread {
                    Type::Readonly(value) => *value,
                    value => value,
                };
                match spread {
                    Type::Tuple(elements) => values.extend(elements),
                    Type::Array(element) => values.push(TupleTypeElement {
                        annotation: Type::Array(element),
                        rest: true,
                        optional: false,
                        label: None,
                    }),
                    _ => return None,
                }
                continue;
            }
            let expected = match context {
                Some(Type::Tuple(elements)) => elements
                    .get(values.len())
                    .map(TupleTypeElement::indexed_type),
                Some(Type::Array(element)) => Some((**element).clone()),
                _ => None,
            };
            let value = match expected {
                Some(expected) => self.infer_in_context(piece, scope, &expected),
                None => self.infer_expression(piece, scope),
            };
            values.push(TupleTypeElement::required(
                self.array_element_type(piece, scope, value),
            ));
        }
        crate::parser::require_tuple_positions_before_suffix(&mut values);
        Some(Type::Tuple(values))
    }
}

fn sequence_placeholder(value: &Type) -> Type {
    match value {
        Type::Tuple(elements) => Type::Tuple(
            elements
                .iter()
                .map(|element| TupleTypeElement {
                    annotation: if element.rest {
                        Type::Array(Box::new(Type::StrictUnknown))
                    } else {
                        Type::StrictUnknown
                    },
                    ..element.clone()
                })
                .collect(),
        ),
        _ => Type::Array(Box::new(Type::StrictUnknown)),
    }
}
