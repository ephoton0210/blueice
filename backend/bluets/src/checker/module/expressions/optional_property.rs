// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

//! Checked optional dot-property reads from the current lexical flow type.

use super::*;

pub(super) enum OptionalPropertyError {
    Unsupported,
    Missing(Type),
    Exhausted,
}

/// Start of an identifier/member/call receiver ending just before `end`.
/// Balanced argument lists keep reads inside them out of the receiver chain.
fn receiver_start(tokens: &[Token], end: usize) -> Option<usize> {
    let mut cursor = end.checked_sub(1)?;
    loop {
        if cursor >= 2 && matches!(tokens[cursor - 1].text.as_str(), "." | "?.") {
            cursor -= 2;
            continue;
        }
        if matches!(tokens[cursor].text.as_str(), ")" | "]") {
            let closing = tokens[cursor].text.as_str();
            let opening = if closing == ")" { "(" } else { "[" };
            let mut depth = 1usize;
            let mut open = cursor;
            while depth != 0 {
                open = open.checked_sub(1)?;
                if tokens[open].is(closing) {
                    depth += 1;
                } else if tokens[open].is(opening) {
                    depth -= 1;
                }
            }
            if open > 0
                && (tokens[open - 1].kind == TokenKind::Identifier
                    || matches!(tokens[open - 1].text.as_str(), "this" | ")" | "]"))
            {
                cursor = open - 1;
                continue;
            }
            return (opening == "(").then_some(open);
        }
        if tokens[cursor].kind != TokenKind::Identifier && !tokens[cursor].is("this") {
            return None;
        }
        return (!cursor
            .checked_sub(1)
            .is_some_and(|before| tokens[before].is("new")))
        .then_some(cursor);
    }
}

pub(super) fn optional_property_type(
    receiver: &Type,
    property: &str,
    aliases: &BTreeMap<String, TypeDefinition>,
    limit: usize,
) -> Result<Type, OptionalPropertyError> {
    let mut budget = TypeExpansionBudget::new(limit);
    let (parts, nullish) =
        optional_receiver_parts(receiver, aliases, &mut HashSet::new(), &mut budget)?;
    let non_nullable = match parts.as_slice() {
        [] => return Err(OptionalPropertyError::Missing(Type::Never)),
        [part] => part.clone(),
        _ => Type::Union(parts.clone()),
    };
    let mut values = Vec::new();
    for part in parts {
        match property_type(&part, property, aliases, &mut HashSet::new(), &mut budget) {
            PropertyType::Found { value, .. } => match value {
                Type::Union(parts) => values.extend(parts),
                value => values.push(value),
            },
            PropertyType::Missing => return Err(OptionalPropertyError::Missing(non_nullable)),
            PropertyType::Exhausted => return Err(OptionalPropertyError::Exhausted),
            PropertyType::Indeterminate => return Err(OptionalPropertyError::Unsupported),
        }
    }
    if nullish {
        values.push(Type::Undefined);
    }
    match values.len() {
        0 => Err(OptionalPropertyError::Unsupported),
        1 => Ok(values.remove(0)),
        _ => Ok(Type::Union(values)),
    }
}

/// Remove short-circuit alternatives, including those behind a type alias.
/// Keep a non-nullable named receiver's identity for member diagnostics.
fn optional_receiver_parts(
    receiver: &Type,
    aliases: &BTreeMap<String, TypeDefinition>,
    visited: &mut HashSet<String>,
    budget: &mut TypeExpansionBudget,
) -> Result<(Vec<Type>, bool), OptionalPropertyError> {
    match receiver {
        Type::Null | Type::Undefined => Ok((Vec::new(), true)),
        Type::Union(parts) => {
            let mut values = Vec::new();
            let mut nullish = false;
            for part in parts {
                if !budget.consume() {
                    return Err(OptionalPropertyError::Exhausted);
                }
                let (alternatives, nullable) =
                    optional_receiver_parts(part, aliases, &mut visited.clone(), budget)?;
                values.extend(alternatives);
                nullish |= nullable;
            }
            Ok((values, nullish))
        }
        Type::Named { name, .. }
            if aliases
                .get(name)
                .is_some_and(|definition| definition.kind == TypeDefinitionKind::Alias) =>
        {
            let expanded = instantiate_named(receiver, aliases, visited, budget, "optional");
            if budget.exhausted {
                return Err(OptionalPropertyError::Exhausted);
            }
            if let Some(expanded) = expanded {
                let (parts, nullish) =
                    optional_receiver_parts(&expanded, aliases, visited, budget)?;
                if nullish {
                    return Ok((parts, true));
                }
            }
            Ok((vec![receiver.clone()], false))
        }
        _ => Ok((vec![receiver.clone()], false)),
    }
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
        if tokens
            .windows(2)
            .any(|pair| pair[0].is("?.") && pair[1].text.starts_with('#'))
        {
            self.typescript_type_error(
                span,
                "unsupported optional property read".into(),
                DiagnosticCode::TypeMismatch,
                18030,
                Vec::new(),
            );
            return true;
        }
        for (index, _) in tokens
            .iter()
            .enumerate()
            .filter(|(_, token)| token.is("?."))
        {
            let Some(property) = tokens.get(index + 1) else {
                continue;
            };
            if index == 0
                || property.kind != TokenKind::Identifier
                || tokens
                    .get(index + 2)
                    .is_some_and(|token| token.is("(") || token.is("["))
            {
                self.optional_property_error(span, "unsupported optional property read".into());
                return true;
            }
            let Some(start) = receiver_start(tokens, index) else {
                self.optional_property_error(span, "unsupported optional property read".into());
                return true;
            };
            let receiver = &tokens[start..index];
            let receiver_type = self.infer_expression(receiver, scope);
            let non_nullable = match &receiver_type {
                Type::Union(parts) => parts
                    .iter()
                    .find(|part| !matches!(part, Type::Null | Type::Undefined)),
                value => Some(value),
            };
            if let Some((visibility, owner)) =
                non_nullable.and_then(|value| self.hidden_member(value, &property.text))
            {
                self.typescript_type_error(
                    span,
                    "unsupported optional property read".into(),
                    DiagnosticCode::TypeMismatch,
                    if visibility == crate::parser::Visibility::Private {
                        2341
                    } else {
                        2445
                    },
                    vec![
                        property.text.clone(),
                        owner.split('@').next().unwrap_or(&owner).into(),
                    ],
                );
                return true;
            }
            match optional_property_type(
                &receiver_type,
                &property.text,
                &self.types,
                self.max_type_expansions,
            ) {
                Ok(_) => {}
                Err(OptionalPropertyError::Missing(receiver)) => {
                    self.missing_property_error(span, &receiver, &property.text, None);
                    if let Some(diagnostic) = self.diagnostics.last_mut() {
                        diagnostic.message = format!(
                            "property `{}` does not exist on optional receiver",
                            property.text
                        );
                    }
                    self.point_last_typescript(std::slice::from_ref(property));
                }
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
        }
        false
    }

    pub(super) fn infer_optional_chain(
        &self,
        tokens: &[Token],
        scope: &BTreeMap<String, Type>,
    ) -> Option<Type> {
        let optional = tokens.iter().position(|token| token.is("?."))?;
        if receiver_start(tokens, optional)? != 0 {
            return None;
        }
        let mut value = self.infer_expression(&tokens[..optional], scope);
        let mut short_circuit = false;
        for pair in tokens[optional..].chunks(2) {
            let [dot, property] = pair else {
                return None;
            };
            if !matches!(dot.text.as_str(), "." | "?.") || property.kind != TokenKind::Identifier {
                return None;
            }
            short_circuit |= dot.is("?.");
            value = if short_circuit {
                optional_property_type(
                    &value,
                    &property.text,
                    &self.types,
                    self.max_type_expansions,
                )
                .unwrap_or(Type::Unknown)
            } else {
                let mut budget = TypeExpansionBudget::new(self.max_type_expansions);
                match property_type(
                    &value,
                    &property.text,
                    &self.types,
                    &mut HashSet::new(),
                    &mut budget,
                ) {
                    PropertyType::Found { value, .. } => value,
                    _ => Type::Unknown,
                }
            };
        }
        Some(value)
    }

    fn optional_property_error(&mut self, span: &SourceSpan, message: String) {
        self.type_error(span, message, DiagnosticCode::TypeMismatch);
    }
}
