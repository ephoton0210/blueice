// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

//! Property access and member mutation validation.

use super::*;

impl<'a> ModuleChecker<'a> {
    pub(in crate::checker::module) fn check_direct_property_access(
        &mut self,
        tokens: &[Token],
        scope: &BTreeMap<String, Type>,
        span: &SourceSpan,
    ) {
        let [base, dot, property] = tokens else {
            return;
        };
        if base.kind != TokenKind::Identifier && !base.is("this")
            || !dot.is(".")
            || !matches!(property.kind, TokenKind::Identifier | TokenKind::Keyword)
        {
            return;
        }
        let value = self.infer_expression(std::slice::from_ref(base), scope);
        if self.flow_nullish_error(base, &value) {
            return;
        }
        let mut budget = TypeExpansionBudget::new(self.max_type_expansions);
        match property_type(
            &value,
            &property.text,
            &self.types,
            &mut HashSet::new(),
            &mut budget,
        ) {
            PropertyType::Found { .. } | PropertyType::Indeterminate => {}
            // A private or protected member is diagnosed, with its accessibility,
            // by the restricted-member scan over the whole expression.
            PropertyType::Missing if self.hidden_member(&value, &property.text).is_some() => {}
            PropertyType::Missing => {
                let direct_span = SourceSpan::new(&span.module, base.start, property.end);
                let diagnostic_span = if self.is_bound_class_constructor_value(&base.text, scope)
                    || self.is_bound_class_instance_type(&value)
                    || self.is_bound_class_static_this(base, scope)
                {
                    &direct_span
                } else {
                    span
                };
                self.missing_property_error(
                    diagnostic_span,
                    &value,
                    &property.text,
                    Some(&base.text),
                );
            }
            PropertyType::Exhausted => self.type_error(
                span,
                format!(
                    "property lookup exceeds the {} generic-expansion limit",
                    self.max_type_expansions
                ),
                DiagnosticCode::ResourceLimit,
            ),
        }
    }

    pub(in crate::checker::module) fn check_member_assignment(
        &mut self,
        tokens: &[Token],
        scope: &BTreeMap<String, Type>,
        span: &SourceSpan,
    ) {
        let tokens = strip_outer_parentheses(tokens);
        let direct = member_mutation(tokens);
        self.check_nested_readonly_mutations(
            tokens,
            scope,
            span,
            direct.as_ref().map(|mutation| mutation.operator.start),
        );
        let Some(mutation) = direct else {
            return;
        };
        let owner = self.infer_expression(mutation.receiver, scope);
        if self.reject_computed_readonly_mutation(mutation.receiver, mutation.property, scope, span)
            || self.reject_opaque_readonly_receiver(mutation.receiver, scope, span)
        {
            return;
        }
        let mut budget = TypeExpansionBudget::new(self.max_type_expansions);
        let Some(property) = mutation.property else {
            match contains_readonly_member(&owner, &self.types, &mut HashSet::new(), &mut budget) {
                Ok(true) => self.type_error(
                    span,
                    "cannot prove a computed property write avoids readonly members".into(),
                    DiagnosticCode::TypeMismatch,
                ),
                Err(()) => self.type_error(
                    span,
                    format!(
                        "property lookup exceeds the {} generic-expansion limit",
                        self.max_type_expansions
                    ),
                    DiagnosticCode::ResourceLimit,
                ),
                Ok(false) => {}
            }
            return;
        };
        match property_type(
            &owner,
            property,
            &self.types,
            &mut HashSet::new(),
            &mut budget,
        ) {
            PropertyType::Found {
                value: expected,
                readonly,
            } => {
                let own_constructor_field = !mutation.operator.is("delete")
                    && matches!(strip_outer_parentheses(mutation.receiver), [receiver] if receiver.is("this"))
                    && self
                        .constructor_readonly_fields
                        .as_ref()
                        .is_some_and(|fields| fields.contains(property));
                if readonly && !own_constructor_field {
                    self.type_error(
                        span,
                        format!("cannot mutate readonly property `{property}`"),
                        DiagnosticCode::TypeMismatch,
                    );
                    return;
                }
                if !mutation.operator.is("=") {
                    return;
                }
                let actual = self.infer_in_context(mutation.value, scope, &expected);
                if !self.is_assignable_bounded(&actual, &expected, span) {
                    self.type_error(
                        span,
                        format!(
                            "assignment has type `{}`, which is not assignable to property `{}` of type `{}`",
                            type_label(&actual),
                            property,
                            type_label(&expected)
                        ),
                        DiagnosticCode::TypeMismatch,
                    );
                }
            }
            PropertyType::Missing if self.hidden_member(&owner, property).is_some() => {}
            PropertyType::Missing => self.type_error(
                span,
                format!(
                    "property `{}` does not exist on type `{}`",
                    property,
                    type_label(&owner)
                ),
                DiagnosticCode::TypeMismatch,
            ),
            PropertyType::Exhausted => self.type_error(
                span,
                format!(
                    "property lookup exceeds the {} generic-expansion limit",
                    self.max_type_expansions
                ),
                DiagnosticCode::ResourceLimit,
            ),
            PropertyType::Indeterminate => {}
        }
    }

    /// `name = value` for a variable declared with a type, and `array[i] = value`,
    /// read against the declared type, so a bracketed or braced literal on the
    /// right takes its shape from the target as it does in an initializer.
    pub(in crate::checker::module) fn check_variable_assignment(
        &mut self,
        tokens: &[Token],
        scope: &BTreeMap<String, Type>,
        span: &SourceSpan,
    ) {
        let tokens = strip_outer_parentheses(tokens);
        let Some((target, operator, value)) = top_level_binary_parts(tokens, &["="], |_| false)
        else {
            return;
        };
        if !operator.is("=") || value.is_empty() {
            return;
        }
        let (expected, what) = match target {
            [name] if name.kind == TokenKind::Identifier => {
                if !self.annotated_names.contains(&name.text) {
                    return;
                }
                let Some(expected) = scope.get(&name.text) else {
                    return;
                };
                (expected.clone(), format!("variable `{}`", name.text))
            }
            _ => {
                let Some((receiver, None)) = member_access_target(target) else {
                    return;
                };
                let Some(open) = target.iter().rposition(|token| token.is("[")) else {
                    return;
                };
                let index = &target[open + 1..target.len() - 1];
                let owner = self.infer_expression(receiver, scope);
                let mut budget = TypeExpansionBudget::new(self.max_type_expansions);
                let mut visited = HashSet::new();
                let mut owner = owner;
                while let Some(expanded) = instantiate_named(
                    &owner,
                    &self.types,
                    &mut visited,
                    &mut budget,
                    "element assignment",
                ) {
                    owner = expanded;
                }
                match (&owner, index) {
                    (Type::Array(item), _) => ((**item).clone(), "an array element".to_string()),
                    (Type::Tuple(elements), [literal])
                        if literal.kind == TokenKind::Number
                            && elements
                                .iter()
                                .all(|element| !element.rest && !element.optional) =>
                    {
                        let Some(position) = literal.text.parse::<usize>().ok() else {
                            return;
                        };
                        let Some(element) = elements.get(position) else {
                            return;
                        };
                        (element.annotation.clone(), "a tuple element".to_string())
                    }
                    _ => return,
                }
            }
        };
        if matches!(expected, Type::Unknown | Type::Any) {
            return;
        }
        let actual = self.infer_in_context(value, scope, &expected);
        if !self.is_assignable_bounded(&actual, &expected, span) {
            self.type_error(
                span,
                format!(
                    "assignment has type `{}`, which is not assignable to {what} of type `{}`",
                    type_label(&actual),
                    type_label(&expected)
                ),
                DiagnosticCode::TypeMismatch,
            );
        }
    }

    fn check_nested_readonly_mutations(
        &mut self,
        tokens: &[Token],
        scope: &BTreeMap<String, Type>,
        span: &SourceSpan,
        direct_operator_start: Option<usize>,
    ) {
        let mut budget = TypeExpansionBudget::new(self.max_type_expansions);
        let mut inspected = 0usize;
        let operator_limit = self.max_type_expansions.saturating_mul(16).max(256);
        let scan_limit = self.max_type_expansions.saturating_mul(32).max(64);
        for (index, operator) in tokens.iter().enumerate() {
            let delete_prefix = operator.is("delete")
                && (index == 0
                    || is_mutation_target_boundary(&tokens[index - 1])
                    || matches!(tokens[index - 1].text.as_str(), "(" | "[" | "{"));
            let prefix = operator.is("++") || operator.is("--") || delete_prefix;
            let assignment = MEMBER_ASSIGNMENT_OPERATORS.contains(&operator.text.as_str());
            if !prefix && !assignment || direct_operator_start == Some(operator.start) {
                continue;
            }
            inspected += 1;
            if inspected > operator_limit {
                self.type_error(
                    span,
                    format!("member mutation scan exceeds its {operator_limit}-operator limit"),
                    DiagnosticCode::ResourceLimit,
                );
                return;
            }
            for forward in [false, true] {
                if forward && !prefix || !forward && delete_prefix {
                    continue;
                }
                let target = if forward {
                    forward_mutation_target(tokens, index, scan_limit)
                } else {
                    backward_mutation_target(tokens, index, scan_limit)
                };
                let target = match target {
                    Ok(target) => target,
                    Err(()) => {
                        self.type_error(
                            span,
                            "member mutation scan exceeds its bounded token window".into(),
                            DiagnosticCode::ResourceLimit,
                        );
                        return;
                    }
                };
                let Some((receiver, property)) = member_access_target(target) else {
                    continue;
                };
                if self.reject_opaque_readonly_receiver(receiver, scope, span) {
                    return;
                }
                let owner = self.infer_expression(receiver, scope);
                let possible_readonly = match self
                    .possible_readonly_computed_receiver(receiver, property, scope)
                {
                    Ok(readonly) => readonly,
                    Err(()) => {
                        self.type_error(
                            span,
                            "computed readonly receiver exceeds its type-expansion limit".into(),
                            DiagnosticCode::ResourceLimit,
                        );
                        return;
                    }
                };
                let readonly = possible_readonly
                    || if let Some(property) = property {
                        match property_type(
                            &owner,
                            property,
                            &self.types,
                            &mut HashSet::new(),
                            &mut budget,
                        ) {
                            PropertyType::Found { readonly, .. } => readonly,
                            PropertyType::Exhausted => {
                                self.type_error(
                                    span,
                                    format!(
                                        "property lookup exceeds the {} generic-expansion limit",
                                        self.max_type_expansions
                                    ),
                                    DiagnosticCode::ResourceLimit,
                                );
                                return;
                            }
                            PropertyType::Missing | PropertyType::Indeterminate => false,
                        }
                    } else {
                        match contains_readonly_member(
                            &owner,
                            &self.types,
                            &mut HashSet::new(),
                            &mut budget,
                        ) {
                            Ok(readonly) => readonly,
                            Err(()) => {
                                self.type_error(
                                    span,
                                    format!(
                                        "property lookup exceeds the {} generic-expansion limit",
                                        self.max_type_expansions
                                    ),
                                    DiagnosticCode::ResourceLimit,
                                );
                                return;
                            }
                        }
                    };
                let own_constructor_field = !operator.is("delete")
                    && matches!(strip_outer_parentheses(receiver), [receiver] if receiver.is("this"))
                    && self
                        .constructor_readonly_fields
                        .as_ref()
                        .is_some_and(|fields| property.is_some_and(|name| fields.contains(name)));
                if readonly && !own_constructor_field {
                    let mutation_span = SourceSpan::new(
                        &span.module,
                        target
                            .first()
                            .expect("member target is nonempty")
                            .start
                            .min(operator.start),
                        target
                            .last()
                            .expect("member target is nonempty")
                            .end
                            .max(operator.end),
                    );
                    self.type_error(
                        &mutation_span,
                        format!(
                            "cannot mutate readonly property{} inside an expression",
                            property.map_or(String::new(), |name| format!(" `{name}`"))
                        ),
                        DiagnosticCode::TypeMismatch,
                    );
                    break;
                }
            }
        }
    }
}

pub(super) struct MemberMutation<'a> {
    pub(super) receiver: &'a [Token],
    pub(super) property: Option<&'a str>,
    pub(super) operator: &'a Token,
    pub(super) value: &'a [Token],
}

const MEMBER_ASSIGNMENT_OPERATORS: &[&str] = &[
    "=", "+=", "-=", "*=", "/=", "%=", "**=", "<<=", ">>=", ">>>=", "&=", "|=", "^=", "&&=", "||=",
    "??=",
];

pub(in crate::checker::module) fn member_access_target(
    tokens: &[Token],
) -> Option<(&[Token], Option<&str>)> {
    let tokens = strip_outer_parentheses(tokens);
    let len = tokens.len();
    if len >= 3
        && tokens[len - 2].is(".")
        && matches!(
            tokens[len - 1].kind,
            TokenKind::Identifier | TokenKind::Keyword
        )
    {
        return Some((&tokens[..len - 2], Some(&tokens[len - 1].text)));
    }
    if !tokens.last()?.is("]") {
        return None;
    }
    let mut depth = 0usize;
    for index in (0..len).rev() {
        if tokens[index].is("]") {
            depth += 1;
        } else if tokens[index].is("[") {
            depth = depth.checked_sub(1)?;
            if depth == 0 {
                if index == 0 {
                    return None;
                }
                let property = match &tokens[index + 1..len - 1] {
                    [literal] if literal.kind == TokenKind::String => {
                        unescaped_property_name(&literal.text)
                    }
                    _ => None,
                };
                return Some((&tokens[..index], property));
            }
        }
    }
    None
}

pub(super) fn member_mutation(tokens: &[Token]) -> Option<MemberMutation<'_>> {
    let (target, operator, value) = match tokens {
        [operator, target @ ..]
            if operator.is("++") || operator.is("--") || operator.is("delete") =>
        {
            (target, operator, &[][..])
        }
        [target @ .., operator] if operator.is("++") || operator.is("--") => {
            (target, operator, &[][..])
        }
        _ => top_level_binary_parts(tokens, MEMBER_ASSIGNMENT_OPERATORS, |_| false)?,
    };
    let target = top_level_binary_parts(target, MEMBER_ASSIGNMENT_OPERATORS, |_| false)
        .map(|(_, _, last)| last)
        .unwrap_or(target);
    let (receiver, property) = member_access_target(target)?;
    Some(MemberMutation {
        receiver,
        property,
        operator,
        value,
    })
}

fn is_mutation_target_boundary(token: &Token) -> bool {
    MEMBER_ASSIGNMENT_OPERATORS.contains(&token.text.as_str())
        || matches!(
            token.text.as_str(),
            "," | ";"
                | ":"
                | "?"
                | "+"
                | "-"
                | "*"
                | "/"
                | "%"
                | "**"
                | "&&"
                | "||"
                | "??"
                | "&"
                | "|"
                | "^"
                | "=="
                | "==="
                | "!="
                | "!=="
                | "<"
                | ">"
                | "<="
                | ">="
                | "<<"
                | ">>"
                | ">>>"
                | "in"
                | "instanceof"
                | "=>"
                | "++"
                | "--"
        )
}

fn backward_mutation_target(
    tokens: &[Token],
    operator_index: usize,
    scan_limit: usize,
) -> Result<&[Token], ()> {
    let mut depth = 0usize;
    let mut start = 0usize;
    for (steps, index) in (0..operator_index).rev().enumerate() {
        if steps >= scan_limit {
            return Err(());
        }
        let token = &tokens[index];
        match token.text.as_str() {
            ")" | "]" | "}" => depth += 1,
            "(" | "[" | "{" if depth > 0 => depth -= 1,
            "(" | "[" | "{" => {
                start = index + 1;
                break;
            }
            _ if depth == 0 && is_mutation_target_boundary(token) => {
                start = index + 1;
                break;
            }
            _ => {}
        }
    }
    Ok(&tokens[start..operator_index])
}

fn forward_mutation_target(
    tokens: &[Token],
    operator_index: usize,
    scan_limit: usize,
) -> Result<&[Token], ()> {
    let mut depth = 0usize;
    let mut end = tokens.len();
    for (steps, index) in (operator_index + 1..tokens.len()).enumerate() {
        if steps >= scan_limit {
            return Err(());
        }
        let token = &tokens[index];
        match token.text.as_str() {
            "(" | "[" | "{" => depth += 1,
            ")" | "]" | "}" if depth > 0 => depth -= 1,
            ")" | "]" | "}" => {
                end = index;
                break;
            }
            _ if depth == 0 && is_mutation_target_boundary(token) => {
                end = index;
                break;
            }
            _ => {}
        }
    }
    Ok(&tokens[operator_index + 1..end])
}

pub(super) fn unescaped_property_name(text: &str) -> Option<&str> {
    let value = text
        .strip_prefix('\'')
        .and_then(|value| value.strip_suffix('\''))
        .or_else(|| {
            text.strip_prefix('"')
                .and_then(|value| value.strip_suffix('"'))
        })?;
    (!value.contains('\\')).then_some(value)
}
