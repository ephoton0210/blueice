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
        let [receiver @ .., dot, property] = tokens else {
            return;
        };
        let Some(base) = receiver.first() else {
            return;
        };
        if (receiver.len() == 1 && base.kind != TokenKind::Identifier && !base.is("this"))
            || (receiver.len() > 1 && constructor_call_parts(receiver).is_none())
            || !dot.is(".")
            || !matches!(property.kind, TokenKind::Identifier | TokenKind::Keyword)
        {
            return;
        }
        let value = self.infer_expression(receiver, scope);
        if receiver.len() == 1 && self.flow_nullish_error(base, &value) {
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
                    (receiver.len() == 1).then_some(base.text.as_str()),
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

    fn computed_mutation_key(
        &self,
        key: &[Token],
        scope: &BTreeMap<String, Type>,
    ) -> Option<String> {
        if let [literal] = key {
            if literal.kind == TokenKind::String {
                return crate::enum_eval::decode_plain_string(&literal.text);
            }
            if literal.kind == TokenKind::Number {
                return crate::enum_eval::parse_number(&literal.text)
                    .map(crate::enum_eval::js_number_text);
            }
        }
        let Type::Literal(text) = self.infer_expression(key, scope) else {
            return None;
        };
        if text.starts_with(['\'', '"', '`']) {
            crate::enum_eval::decode_plain_string(&text)
        } else {
            crate::enum_eval::parse_number(&text).map(crate::enum_eval::js_number_text)
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
        let inferred_property = mutation
            .computed
            .filter(|_| mutation.property.is_none())
            .and_then(|key| self.computed_mutation_key(key, scope));
        let property = mutation.property.or(inferred_property.as_deref());
        let owner = self.infer_expression(mutation.receiver, scope);
        if self.reject_computed_readonly_mutation(mutation.receiver, property, scope, span)
            || self.reject_opaque_readonly_receiver(mutation.receiver, scope, span)
        {
            return;
        }
        // Bracket access on `never` has an indexed value type of `never`;
        // dotted access still reports a missing property. Check retained owner
        // readonly provenance above before applying this indexed-write rule.
        if matches!(owner, Type::Never) && mutation.computed.is_some() {
            if mutation.operator.is("=") {
                let actual = self.infer_in_context(mutation.value, scope, &Type::Never);
                if !self.is_assignable_bounded(&actual, &Type::Never, span) {
                    self.type_error(
                        span,
                        format!(
                            "assignment has type `{}`, which is not assignable to an indexed property of type `never`",
                            type_label(&actual)
                        ),
                        DiagnosticCode::TypeMismatch,
                    );
                }
            }
            return;
        }
        let mut budget = TypeExpansionBudget::new(self.max_type_expansions);
        let Some(property) = property else {
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
        match property_write_type(
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
                if self.check_fresh_properties(mutation.value, &expected, scope, span) {
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
                if !(self.annotated_names.contains(&name.text)
                    || self.module.id.ends_with(".js") && self.check_javascript == Some(true))
                {
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
        if self.check_fresh_properties(value, &expected, scope, span) {
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
            self.explain_last_type_pair(&actual, &expected);
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
                let inferred_property = (property.is_none()
                    && target.last().is_some_and(|token| token.is("]")))
                .then(|| {
                    self.computed_mutation_key(&target[receiver.len() + 1..target.len() - 1], scope)
                })
                .flatten();
                let property = property.or(inferred_property.as_deref());
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
    computed: Option<&'a [Token]>,
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
                    [literal]
                        if literal.kind == TokenKind::Number
                            && crate::enum_eval::parse_number(&literal.text)
                                .map(crate::enum_eval::js_number_text)
                                .as_deref()
                                == Some(literal.text.as_str()) =>
                    {
                        Some(literal.text.as_str())
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
        computed: target
            .last()
            .is_some_and(|token| token.is("]"))
            .then(|| &target[receiver.len() + 1..target.len() - 1]),
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
