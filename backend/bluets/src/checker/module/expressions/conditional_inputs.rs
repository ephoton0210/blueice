// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

//! A constrained conditional branch supplies a literal's inference context.
use super::*;

impl ModuleChecker<'_> {
    pub(super) fn conditional_argument(
        &self,
        tokens: &[Token],
        scope: &BTreeMap<String, Type>,
        annotation: &Type,
        signature: &FunctionSignature,
        budget: &mut TypeExpansionBudget,
    ) -> Option<Type> {
        let mut template = annotation.clone();
        let mut visited = HashSet::new();
        while !matches!(template, Type::Conditional(_)) {
            template = instantiate_named(
                &template,
                &self.types,
                &mut visited,
                budget,
                "conditional inference context",
            )?;
        }
        let Type::Conditional(conditional) = template else {
            unreachable!();
        };
        let Type::Named { name, arguments } = &conditional.check else {
            return None;
        };
        if !arguments.is_empty()
            || !signature.type_parameters.iter().any(|p| p.name == *name)
            || !matches!(
                conditional.extends,
                Type::String | Type::Number | Type::Boolean
            )
        {
            return None;
        }
        let constraints = BTreeMap::from([(name.clone(), conditional.extends)]);
        Some(self.branch_argument(tokens, scope, &conditional.when_true, &constraints, budget))
    }

    fn branch_argument(
        &self,
        tokens: &[Token],
        scope: &BTreeMap<String, Type>,
        template: &Type,
        constraints: &BTreeMap<String, Type>,
        budget: &mut TypeExpansionBudget,
    ) -> Type {
        if !budget.consume() {
            return Type::Unknown;
        }
        let expected = substitute_type(template, constraints);
        if matches!(template, Type::Named {name, arguments} if arguments.is_empty() && constraints.contains_key(name))
        {
            if let [token] = strip_outer_parentheses(tokens) {
                if matches!(token.kind, TokenKind::String | TokenKind::Number)
                    || token.is("true")
                    || token.is("false")
                {
                    let literal = Type::Literal(token.text.clone());
                    if is_assignable(
                        &literal,
                        &expected,
                        &self.types,
                        &mut HashSet::new(),
                        budget,
                    ) {
                        return literal;
                    }
                }
            }
        }
        if let Type::Record(templates) = template {
            if tokens.first().is_some_and(|t| t.is("{")) {
                let mut actual = self.infer_in_context(tokens, scope, &expected);
                if let Type::Record(fields) = &mut actual {
                    let mut index = 1;
                    while index + 2 < tokens.len() && !tokens[index].is("}") {
                        if !tokens[index + 1].is(":") {
                            break;
                        }
                        let start = index + 2;
                        let Some(end) = inference::record_member_end(tokens, start) else {
                            break;
                        };
                        if let (Some(template), Some(field)) = (
                            templates.iter().find(|f| f.name == tokens[index].text),
                            fields.iter_mut().find(|f| f.name == tokens[index].text),
                        ) {
                            field.value = self.branch_argument(
                                &tokens[start..end],
                                scope,
                                &template.value,
                                constraints,
                                budget,
                            );
                        }
                        index = end + usize::from(tokens[end].is(","));
                    }
                }
                return actual;
            }
        }
        self.infer_in_context(tokens, scope, &expected)
    }
}
