// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

//! One bounded optional dot-property read on a module-local immutable binding.

use super::*;
use crate::parser::VariableKind;

pub(super) enum OptionalPropertyError {
    Unsupported,
    Missing,
    Exhausted,
}

pub(super) fn optional_property_type(
    receiver: &Type,
    property: &str,
    aliases: &BTreeMap<String, TypeDefinition>,
    limit: usize,
) -> Result<Type, OptionalPropertyError> {
    let Type::Union(parts) = receiver else {
        return Err(OptionalPropertyError::Unsupported);
    };
    let [first, second] = parts.as_slice() else {
        return Err(OptionalPropertyError::Unsupported);
    };
    let record = if matches!(first, Type::Null | Type::Undefined) {
        second
    } else if matches!(second, Type::Null | Type::Undefined) {
        first
    } else {
        return Err(OptionalPropertyError::Unsupported);
    };
    let expanded = match record {
        Type::Record(_) => record.clone(),
        Type::Named { name, arguments } if arguments.is_empty() => {
            let Some(definition) = aliases.get(name) else {
                return Err(OptionalPropertyError::Unsupported);
            };
            if definition.kind != TypeDefinitionKind::Interface || !definition.parameters.is_empty()
            {
                return Err(OptionalPropertyError::Unsupported);
            }
            let mut budget = TypeExpansionBudget::new(limit);
            let expanded = instantiate_named(
                record,
                aliases,
                &mut HashSet::new(),
                &mut budget,
                "optional receiver",
            );
            if budget.exhausted {
                return Err(OptionalPropertyError::Exhausted);
            }
            expanded.ok_or(OptionalPropertyError::Unsupported)?
        }
        _ => return Err(OptionalPropertyError::Unsupported),
    };
    let Type::Record(fields) = expanded else {
        return Err(OptionalPropertyError::Unsupported);
    };
    let [field] = fields.as_slice() else {
        return Err(OptionalPropertyError::Unsupported);
    };
    if field.name != property {
        return Err(OptionalPropertyError::Missing);
    }
    if field.optional || !matches!(field.value, Type::String | Type::Number | Type::Boolean) {
        return Err(OptionalPropertyError::Unsupported);
    }
    Ok(Type::Union(vec![field.value.clone(), Type::Undefined]))
}

impl ModuleChecker<'_> {
    pub(in crate::checker::module) fn check_optional_property_read(
        &mut self,
        tokens: &[Token],
        scope: &BTreeMap<String, Type>,
        span: &SourceSpan,
    ) -> bool {
        if !tokens.iter().any(|token| token.is("?.")) {
            return false;
        }
        let selected = match tokens {
            [receiver, optional, property] | [receiver, optional, property, _, ..]
                if receiver.kind == TokenKind::Identifier
                    && optional.is("?.")
                    && property.kind == TokenKind::Identifier
                    && (tokens.len() == 3
                        || (tokens.len() >= 5
                            && tokens[3].is("??")
                            && !tokens[4..].iter().any(|token| token.is("?.")))) =>
            {
                Some((receiver, property))
            }
            _ => None,
        };
        let Some((receiver, property)) = selected else {
            self.optional_property_error(span, "unsupported optional property read".into());
            return true;
        };
        let top_level = self.module.declarations.iter().any(|declaration| {
            declaration.span() == span
                && matches!(declaration, Declaration::Variable(_) | Declaration::Raw(_))
        });
        let binding = self.module.declarations.iter().find_map(|declaration| {
            let Declaration::Variable(variable) = declaration else {
                return None;
            };
            (variable.name == receiver.text
                && variable.kind == VariableKind::Const
                && !variable.declared
                && variable.span.end <= span.start
                && variable.annotation.as_ref() == scope.get(&receiver.text))
            .then_some(variable)
        });
        if !top_level || binding.is_none() {
            self.optional_property_error(span, "unsupported optional property read".into());
            return true;
        }
        let Some(receiver_type) = scope.get(&receiver.text) else {
            self.optional_property_error(span, "unsupported optional property read".into());
            return true;
        };
        match optional_property_type(
            receiver_type,
            &property.text,
            &self.types,
            self.max_type_expansions,
        ) {
            Ok(_) => {
                if tokens.len() > 3 {
                    self.check_direct_runtime_expression(&tokens[4..], scope, span);
                }
            }
            Err(OptionalPropertyError::Missing) => self.optional_property_error(
                span,
                format!(
                    "property `{}` does not exist on optional receiver",
                    property.text
                ),
            ),
            Err(OptionalPropertyError::Unsupported) => {
                self.optional_property_error(span, "unsupported optional property read".into())
            }
            Err(OptionalPropertyError::Exhausted) => self.type_error(
                span,
                format!(
                    "optional property lookup exceeds the {} generic-expansion limit",
                    self.max_type_expansions
                ),
                DiagnosticCode::ResourceLimit,
            ),
        }
        true
    }

    fn optional_property_error(&mut self, span: &SourceSpan, message: String) {
        self.type_error(span, message, DiagnosticCode::TypeMismatch);
    }
}
