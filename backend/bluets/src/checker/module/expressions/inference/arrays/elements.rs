// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

//! Fresh literal elements widen in mutable arrays; regular literals retain their types.

use super::*;

impl ModuleChecker<'_> {
    pub(super) fn array_element_type(
        &self,
        tokens: &[Token],
        scope: &BTreeMap<String, Type>,
        value: Type,
    ) -> Type {
        let enum_member = match &value {
            Type::Literal(name) => Some(name),
            Type::Named { name, arguments } if arguments.is_empty() => Some(name),
            _ => None,
        };
        if let Some(name) = enum_member {
            if self
                .types
                .get(name)
                .is_some_and(|definition| definition.kind == TypeDefinitionKind::EnumMember)
            {
                if let Some((owner, _)) = name.rsplit_once('.') {
                    return Type::Named {
                        name: owner.into(),
                        arguments: Vec::new(),
                    };
                }
            }
        }
        if self.fresh_array_element(tokens, scope, &mut BTreeSet::new(), 0) {
            return crate::checker::module::return_inference::widen(value);
        }
        value
    }

    fn fresh_array_element(
        &self,
        tokens: &[Token],
        scope: &BTreeMap<String, Type>,
        seen: &mut BTreeSet<String>,
        depth: usize,
    ) -> bool {
        if depth >= 128 {
            return false;
        }
        let tokens = strip_outer_parentheses(tokens);
        if let [token] = tokens {
            if matches!(
                token.kind,
                TokenKind::Number | TokenKind::String | TokenKind::Template
            ) || token.is("true")
                || token.is("false")
            {
                return true;
            }
            if scope.get(&token.text) != self.values.get(&token.text)
                || !seen.insert(token.text.clone())
            {
                return false;
            }
            return self.module.declarations.iter().any(|declaration| {
                matches!(declaration, Declaration::Variable(variable)
                    if variable.name == token.text
                        && variable.kind == crate::parser::VariableKind::Const
                        && variable.annotation.is_none()
                        && self.fresh_array_element(&variable.initializer, scope, seen, depth + 1))
            });
        }
        let Some(call) = direct_call_parts(tokens).filter(|call| !call.generic) else {
            return false;
        };
        let Some(arguments) = split_call_arguments(call.arguments) else {
            return false;
        };
        let Some(signatures) = self
            .function_value_signatures(&call.callee.text, scope, false)
            .or_else(|| self.functions.get(&call.callee.text).cloned())
        else {
            return false;
        };
        let Ok(actuals) = self.expanded_call_argument_types_for(&arguments, scope, &signatures)
        else {
            return false;
        };
        let Ok(Some(signature)) = self.select_function_signature(&signatures, &actuals, None)
        else {
            return false;
        };
        let Type::Named {
            name,
            arguments: type_arguments,
        } = &signature.return_type
        else {
            return false;
        };
        if !type_arguments.is_empty()
            || !signature
                .type_parameters
                .iter()
                .any(|parameter| parameter.name == *name && !parameter.is_const)
        {
            return false;
        }
        signature.parameters.iter().zip(&arguments).any(|(parameter, argument)| {
            matches!(&parameter.annotation, Some(Type::Named { name: parameter_name, arguments })
                if parameter_name == name && arguments.is_empty())
                && self.fresh_array_element(argument, scope, &mut seen.clone(), depth + 1)
        })
    }
}

/// Absent keys of an anonymous object-literal union are optional undefined.
/// Only literal objects use this normalization; named values retain their shape.
pub(super) fn complete_object_union(values: &mut [Type]) {
    if values.iter().any(|value| !matches!(value, Type::Record(_))) {
        return;
    }
    let mut all = Vec::new();
    for value in values.iter() {
        if let Type::Record(fields) = value {
            for field in fields {
                if !all
                    .iter()
                    .any(|known: &crate::parser::TypeField| known.name == field.name)
                {
                    all.push(field.clone());
                }
            }
        }
    }
    for value in values {
        if let Type::Record(fields) = value {
            for field in &all {
                if !fields.iter().any(|existing| existing.name == field.name) {
                    fields.push(crate::parser::TypeField {
                        optional: true,
                        readonly: false,
                        value: Type::Undefined,
                        ..field.clone()
                    });
                }
            }
        }
    }
}
