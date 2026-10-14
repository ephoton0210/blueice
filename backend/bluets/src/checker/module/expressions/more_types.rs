// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

//! Static assertions and readonly containers retain their operand boundaries.
use super::mutation::member_mutation;
use super::*;

impl ModuleChecker<'_> {
    pub(in crate::checker::module) fn fresh_symbol_call(
        &self,
        tokens: &[Token],
        scope: &BTreeMap<String, Type>,
    ) -> bool {
        self.library_values.contains("Symbol")
            && scope.get("Symbol") == self.values.get("Symbol")
            && tokens.first().is_some_and(|token| token.is("Symbol"))
            && (tokens.get(1).is_some_and(|token| token.is("("))
                || tokens.get(1).is_some_and(|token| token.is("."))
                    && tokens.get(2).is_some_and(|token| token.is("for"))
                    && tokens.get(3).is_some_and(|token| token.is("(")))
    }

    pub(in crate::checker::module) fn infer_const_assertion(
        &self,
        tokens: &[Token],
        scope: &BTreeMap<String, Type>,
    ) -> Type {
        self.const_operand(strip_outer_parentheses(tokens), scope, 0)
    }

    fn const_operand(
        &self,
        tokens: &[Token],
        scope: &BTreeMap<String, Type>,
        depth: usize,
    ) -> Type {
        if depth >= 128 || tokens.is_empty() {
            return Type::Unknown;
        }
        if let [literal] = tokens {
            if matches!(
                literal.kind,
                TokenKind::String | TokenKind::Number | TokenKind::Template
            ) || literal.is("true")
                || literal.is("false")
            {
                return Type::Literal(literal.text.clone());
            }
        }
        if tokens.first().is_some_and(|token| token.is("["))
            && tokens.last().is_some_and(|token| token.is("]"))
        {
            if let Some(parts) = Self::literal_elements(tokens) {
                return Type::Readonly(Box::new(Type::Tuple(
                    parts
                        .iter()
                        .map(|part| {
                            TupleTypeElement::required(self.const_operand(part, scope, depth + 1))
                        })
                        .collect(),
                )));
            }
        }
        if let Type::Record(mut fields) = self.infer_expression(tokens, scope) {
            for field in &mut fields {
                field.readonly = true;
                let start = tokens.partition_point(|token| token.start < field.span.start);
                let end = tokens.partition_point(|token| token.end <= field.span.end);
                let piece = &tokens[start..end];
                if piece.get(1).is_some_and(|token| token.is(":")) {
                    field.value = self.const_operand(&piece[2..], scope, depth + 1);
                }
            }
            return Type::Record(fields);
        }
        self.infer_expression(tokens, scope)
    }

    pub(in crate::checker::module) fn check_additional_expression(
        &mut self,
        tokens: &[Token],
        scope: &BTreeMap<String, Type>,
        span: &SourceSpan,
    ) -> bool {
        let operand = strip_outer_parentheses(erased_assertion_operand(tokens).unwrap_or(tokens));
        let literal = match operand {
            [sign, literal, ..] if sign.is("-") || sign.is("+") => Some(literal),
            [literal, ..] => Some(literal),
            [] => None,
        };
        if self.target < crate::EcmaTarget::Es2020 {
            if let Some(literal) =
                literal.filter(|token| token.kind == TokenKind::Number && token.text.ends_with('n'))
            {
                self.typescript_type_error(
                    &literal.span(&self.module.id),
                    "BigInt literals are not available when targeting lower than ES2020."
                        .to_string(),
                    DiagnosticCode::UnsupportedSyntax,
                    2737,
                    Vec::new(),
                );
            }
        }
        if let Some(operand) = erased_assertion_operand(tokens) {
            let marker = &tokens[operand.len()];
            if let Some(annotation) = self.module.type_assertions.get(&marker.start).cloned() {
                if annotation == Type::ConstAssertion {
                    let first = strip_outer_parentheses(operand).first();
                    let valid =
                        first.is_some_and(|token| {
                            matches!(
                                token.kind,
                                TokenKind::String | TokenKind::Number | TokenKind::Template
                            ) || matches!(
                                token.text.as_str(),
                                "[" | "{" | "true" | "false" | "-" | "+"
                            )
                        }) || matches!(self.infer_expression(operand, scope), Type::Literal(_));
                    if !valid {
                        let token = &tokens[operand.len() + 1];
                        self.typescript_type_error(
                            &operand.last().map_or_else(
                                || token.span(&self.module.id),
                                |operand| operand.span(&self.module.id),
                            ),
                            "invalid const assertion operand".into(),
                            DiagnosticCode::TypeMismatch,
                            1355,
                            vec![],
                        );
                        return true;
                    }
                } else if marker.is("as") && matches!(annotation, Type::Literal(_)) {
                    let actual = self.infer_expression(operand, scope);
                    let mut budget = TypeExpansionBudget::new(self.max_type_expansions);
                    budget.checking = self.checking;
                    let overlaps = is_assignable(
                        &actual,
                        &annotation,
                        &self.types,
                        &mut HashSet::new(),
                        &mut budget,
                    ) || is_assignable(
                        &annotation,
                        &actual,
                        &self.types,
                        &mut HashSet::new(),
                        &mut budget,
                    );
                    if !overlaps && !budget.exhausted {
                        let point = SourceSpan::new(
                            &self.module.id,
                            tokens.first().expect("assertion operand").start,
                            tokens.last().expect("assertion target").end,
                        );
                        self.typescript_type_error(
                            &point,
                            "literal assertion types do not overlap".into(),
                            DiagnosticCode::TypeMismatch,
                            2352,
                            vec![
                                crate::diagnostic::type_text::render_in(&actual, self.project),
                                crate::diagnostic::type_text::render_in(&annotation, self.project),
                            ],
                        );
                        return true;
                    }
                } else if marker.is("satisfies") {
                    self.check_type(&annotation, span);
                    let actual = self.infer_in_context(operand, scope, &annotation);
                    let mut budget = TypeExpansionBudget::new(self.max_type_expansions);
                    let expected = crate::checker::type_operators::expanded(
                        &annotation,
                        &self.types,
                        &mut HashSet::new(),
                        &mut budget,
                    );
                    if let (Type::Record(actual), Type::Record(expected)) = (&actual, &expected) {
                        if let Some(field) = actual
                            .iter()
                            .find(|field| !expected.iter().any(|other| other.name == field.name))
                        {
                            let field_span = SourceSpan::new(
                                &self.module.id,
                                field.span.start,
                                field.span.start + field.name.len(),
                            );
                            self.typescript_type_error(
                                &field_span,
                                "excess property in satisfies operand".into(),
                                DiagnosticCode::TypeMismatch,
                                2353,
                                vec![
                                    field.name.clone(),
                                    crate::diagnostic::type_text::render(&annotation),
                                ],
                            );
                            return true;
                        }
                        for field in actual {
                            let Some(expected_field) =
                                expected.iter().find(|other| other.name == field.name)
                            else {
                                continue;
                            };
                            if !is_assignable(
                                &field.value,
                                &expected_field.value,
                                &self.types,
                                &mut HashSet::new(),
                                &mut budget,
                            ) {
                                let point = SourceSpan::new(
                                    &self.module.id,
                                    field.span.start,
                                    field.span.start + field.name.len(),
                                );
                                let mut diagnostic = Diagnostic::error(
                                    DiagnosticCode::TypeMismatch,
                                    point,
                                    "satisfies property is incompatible",
                                )
                                .with_typescript(
                                    2322,
                                    vec![
                                        crate::diagnostic::type_text::render(&field.value),
                                        crate::diagnostic::type_text::render(&expected_field.value),
                                    ],
                                )
                                .with_source_position(&self.module.source);
                                let origin = SourceSpan::new(
                                    &self.module.id,
                                    expected_field.span.start,
                                    expected_field.span.start + expected_field.name.len(),
                                );
                                let cause = Diagnostic::error(
                                    DiagnosticCode::TypeMismatch,
                                    origin.clone(),
                                    "satisfies property origin",
                                )
                                .with_typescript(
                                    6500,
                                    vec![
                                        expected_field.name.clone(),
                                        crate::diagnostic::type_text::render(&annotation),
                                    ],
                                );
                                if let (Some(counterpart), Some(cause)) =
                                    (&mut diagnostic.typescript, cause.typescript)
                                {
                                    counterpart.related_information.push(
                                        crate::TypeScriptRelatedInformation {
                                            code: 6500,
                                            message: cause.message,
                                            span: origin,
                                            position: None,
                                        },
                                    );
                                }
                                self.diagnostics.push(diagnostic);
                                return true;
                            }
                        }
                    }
                    if !self.is_assignable_bounded(&actual, &annotation, span) {
                        self.assignment_error(
                            span,
                            "satisfies operand is not assignable to its target".into(),
                            DiagnosticCode::TypeMismatch,
                            &actual,
                            &annotation,
                        );
                        return true;
                    }
                }
            }
        }
        if let Some(mutation) = member_mutation(tokens) {
            let owner = self.infer_expression(mutation.receiver, scope);
            let original_owner = owner.clone();
            let mut budget = TypeExpansionBudget::new(self.max_type_expansions);
            let owner = crate::checker::type_operators::expanded(
                &owner,
                &self.types,
                &mut HashSet::new(),
                &mut budget,
            );
            let readonly = match &owner {
                Type::Readonly(_) => true,
                Type::IndexedRecord { object, indices } => {
                    let named = mutation.property.and_then(|property| {
                        match property_type(
                            object,
                            property,
                            &self.types,
                            &mut HashSet::new(),
                            &mut budget,
                        ) {
                            PropertyType::Found { readonly, .. } => Some(readonly),
                            _ => None,
                        }
                    });
                    named.unwrap_or_else(|| indices.iter().any(|index| index.readonly))
                }
                _ => false,
            };
            if readonly {
                let tuple = matches!(&owner, Type::Readonly(inner) if matches!(inner.as_ref(), Type::Tuple(_)));
                if tuple {
                    let literal_key = tokens
                        .iter()
                        .skip(mutation.receiver.len())
                        .find(|token| matches!(token.kind, TokenKind::Number | TokenKind::String));
                    if let Some(property) = mutation
                        .property
                        .or_else(|| literal_key.map(|token| token.text.trim_matches(['\'', '"'])))
                    {
                        let key = tokens.iter().find(|token| {
                            token.start >= mutation.receiver.last().map_or(0, |token| token.end)
                                && matches!(token.kind, TokenKind::Number | TokenKind::String)
                        });
                        let key_span =
                            key.map_or_else(|| span.clone(), |token| token.span(&self.module.id));
                        self.typescript_type_error(
                            &key_span,
                            "readonly tuple position cannot be written".into(),
                            DiagnosticCode::TypeMismatch,
                            2540,
                            vec![property.into()],
                        );
                        return true;
                    }
                }
                let end = tokens
                    .iter()
                    .take_while(|token| token.start < mutation.operator.start)
                    .last()
                    .map_or(mutation.operator.start, |token| token.end);
                let lhs = SourceSpan::new(&self.module.id, tokens[0].start, end);
                self.typescript_type_error(
                    &lhs,
                    "readonly index signature cannot be written".into(),
                    DiagnosticCode::TypeMismatch,
                    2542,
                    vec![crate::diagnostic::type_text::render(
                        if matches!(owner, Type::IndexedRecord { .. }) {
                            &original_owner
                        } else {
                            &owner
                        },
                    )],
                );
                return true;
            }
        }
        if let [base, dot, _] = strip_outer_parentheses(tokens) {
            let owner = self.infer_expression(std::slice::from_ref(base), scope);
            let mut budget = TypeExpansionBudget::new(self.max_type_expansions);
            let owner = crate::checker::type_operators::expanded(
                &owner,
                &self.types,
                &mut HashSet::new(),
                &mut budget,
            );
            if self.explicit_checking && dot.is(".") && owner == Type::StrictUnknown {
                self.typescript_type_error(
                    &base.span(&self.module.id),
                    "unknown receiver cannot be read".into(),
                    DiagnosticCode::TypeMismatch,
                    18046,
                    vec![base.text.clone()],
                );
                return true;
            }
        }
        false
    }
}
