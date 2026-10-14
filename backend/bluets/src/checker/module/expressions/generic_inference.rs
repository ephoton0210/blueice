// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

//! Contextual result candidates share the argument instantiation path.

use super::*;

impl ModuleChecker<'_> {
    pub(in crate::checker::module) fn point_last_type_argument(
        &mut self,
        span: &SourceSpan,
        selected: Option<usize>,
    ) {
        let source = self.project.source(&span.module).unwrap_or("");
        let Ok(tokens) = crate::syntax::lex(&span.module, source) else {
            return;
        };
        let tokens = tokens
            .iter()
            .filter(|token| span.start <= token.start && token.end <= span.end)
            .cloned()
            .collect::<Vec<_>>();
        let Some(open) = tokens.iter().position(|token| token.is("<")) else {
            return;
        };
        let mut depth = 0usize;
        let mut start = open + 1;
        let mut argument = 0;
        for index in open + 1..tokens.len() {
            match tokens[index].text.as_str() {
                "<" | "(" | "[" | "{" => depth += 1,
                ">" if depth == 0 => {
                    let range = if selected.is_none() {
                        open + 1..index
                    } else if selected == Some(argument) {
                        start..index
                    } else {
                        return;
                    };
                    self.point_last_typescript(&tokens[range]);
                    return;
                }
                ">" | ")" | "]" | "}" => depth = depth.saturating_sub(1),
                "," if depth == 0 => {
                    if selected == Some(argument) {
                        self.point_last_typescript(&tokens[start..index]);
                        return;
                    }
                    argument += 1;
                    start = index + 1;
                }
                _ => {}
            }
        }
    }

    pub(in crate::checker::module) fn point_last_type_default(&mut self, span: &SourceSpan) {
        let source = self.project.source(&span.module).unwrap_or("");
        let Ok(tokens) = crate::syntax::lex(&span.module, source) else {
            return;
        };
        let tokens = tokens
            .iter()
            .filter(|token| span.start <= token.start && token.end <= span.end)
            .cloned()
            .collect::<Vec<_>>();
        if let Some(equal) = tokens.iter().position(|token| token.is("=")) {
            self.point_last_typescript(&tokens[equal + 1..]);
        }
    }

    pub(in crate::checker::module) fn infer_generic_result_in_context(
        &self,
        tokens: &[Token],
        scope: &BTreeMap<String, Type>,
        expected: &Type,
    ) -> Option<Type> {
        let call = direct_call_parts(strip_outer_parentheses(tokens))?;
        if call.generic {
            return None;
        }
        let signatures = self
            .function_value_signatures(&call.callee.text, scope, false)
            .or_else(|| self.functions.get(&call.callee.text).cloned())?;
        if signatures
            .iter()
            .all(|signature| signature.type_parameters.is_empty())
        {
            return None;
        }
        let arguments = split_call_arguments(call.arguments)?;
        let actuals = self
            .expanded_call_argument_types_for(&arguments, scope, &signatures)
            .ok()?;
        let signature = self
            .select_function_signature(&signatures, &actuals, None)
            .ok()??;
        let mut budget = TypeExpansionBudget::new(self.max_type_expansions);
        budget.checking = self.checking;
        let substitutions = crate::checker::inference::infer_contextual_substitutions(
            signature,
            &actuals,
            &self.types,
            &mut budget,
            Some(expected),
        );
        if budget.exhausted && self.enforce_types {
            if let (Some(first), Some(last)) = (tokens.first(), tokens.last()) {
                self.generic_inference_failure.set(Some(SourceSpan::new(
                    &self.module.id,
                    first.start,
                    last.end,
                )));
            }
        }
        Some(substitute_type(&signature.return_type, &substitutions).runtime_result())
    }
}
