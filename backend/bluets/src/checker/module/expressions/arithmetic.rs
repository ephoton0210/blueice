// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

//! Primitive arithmetic and equality checks.

use super::*;

impl<'a> ModuleChecker<'a> {
    /// Checks only arithmetic forms whose operand types are already known
    /// primitive values. Unknown, `any`, union and structural forms remain
    /// outside this deliberately bounded compatibility rule.
    pub(in crate::checker::module) fn check_arithmetic_operators(
        &mut self,
        tokens: &[Token],
        scope: &BTreeMap<String, Type>,
        span: &SourceSpan,
    ) {
        let tokens = strip_outer_parentheses(tokens);
        let generic_call = |start| self.module.generic_call_type_arguments.contains_key(&start);
        if let Some((condition, consequent, alternate)) = conditional_expression_parts(tokens) {
            self.check_arithmetic_operators(condition, scope, span);
            self.check_arithmetic_operators(consequent, scope, span);
            self.check_arithmetic_operators(alternate, scope, span);
            return;
        }
        if let Some((left, _, right)) = top_level_binary_parts(tokens, &["??"], generic_call) {
            self.check_arithmetic_operators(left, scope, span);
            self.check_arithmetic_operators(right, scope, span);
            return;
        }
        for operators in [&["||"][..], &["&&"][..]] {
            if let Some((left, _, right)) = top_level_binary_parts(tokens, operators, generic_call)
            {
                self.check_arithmetic_operators(left, scope, span);
                self.check_arithmetic_operators(right, scope, span);
                return;
            }
        }
        for operators in [&["|"][..], &["^"][..], &["&"][..]] {
            if let Some((left, operator, right)) =
                top_level_binary_parts(tokens, operators, generic_call)
            {
                self.check_arithmetic_operators(left, scope, span);
                self.check_arithmetic_operators(right, scope, span);
                self.check_known_numeric_operands(
                    &operator.text,
                    &self.infer_expression(left, scope),
                    &self.infer_expression(right, scope),
                    span,
                );
                return;
            }
        }
        if let Some((left, operator, right)) =
            top_level_binary_parts(tokens, &["===", "!=="], generic_call)
        {
            self.check_arithmetic_operators(left, scope, span);
            self.check_arithmetic_operators(right, scope, span);
            self.check_known_disjoint_strict_equality(
                operator,
                &self.infer_expression(left, scope),
                &self.infer_expression(right, scope),
                span,
            );
            return;
        }
        if let Some((left, _, right)) =
            top_level_binary_parts(tokens, &["==", "!=", "<", ">", "<=", ">="], generic_call)
        {
            self.check_arithmetic_operators(left, scope, span);
            self.check_arithmetic_operators(right, scope, span);
            return;
        }
        if let Some((left, operator, right)) = top_level_shift_parts(tokens, generic_call) {
            self.check_arithmetic_operators(left, scope, span);
            self.check_arithmetic_operators(right, scope, span);
            self.check_known_numeric_operands(
                operator.text(),
                &self.infer_expression(left, scope),
                &self.infer_expression(right, scope),
                span,
            );
            return;
        }
        if let Some((left, operator, right)) =
            top_level_binary_parts(tokens, &["+", "-"], generic_call)
        {
            self.check_arithmetic_operators(left, scope, span);
            self.check_arithmetic_operators(right, scope, span);
            self.check_known_arithmetic_operands(
                operator,
                &self.infer_expression(left, scope),
                &self.infer_expression(right, scope),
                span,
            );
            return;
        }
        if let Some((left, operator, right)) =
            top_level_binary_parts(tokens, &["*", "/", "%"], generic_call)
        {
            self.check_arithmetic_operators(left, scope, span);
            self.check_arithmetic_operators(right, scope, span);
            self.check_known_arithmetic_operands(
                operator,
                &self.infer_expression(left, scope),
                &self.infer_expression(right, scope),
                span,
            );
            return;
        }
        if let Some((left, operator, right)) = top_level_binary_parts(tokens, &["**"], generic_call)
        {
            self.check_arithmetic_operators(left, scope, span);
            self.check_arithmetic_operators(right, scope, span);
            self.check_known_arithmetic_operands(
                operator,
                &self.infer_expression(left, scope),
                &self.infer_expression(right, scope),
                span,
            );
        }
    }

    pub(in crate::checker::module) fn check_known_arithmetic_operands(
        &mut self,
        operator: &Token,
        left: &Type,
        right: &Type,
        span: &SourceSpan,
    ) {
        if !is_known_primitive_type(left) || !is_known_primitive_type(right) {
            return;
        }
        let accepted = (operator.is("+") && (left == &Type::String || right == &Type::String))
            || (left == &Type::Number && right == &Type::Number);
        if !accepted {
            self.type_error(
                span,
                format!(
                    "operator `{}` cannot be applied to types `{}` and `{}`",
                    operator.text,
                    type_label(left),
                    type_label(right),
                ),
                DiagnosticCode::TypeMismatch,
            );
        }
    }

    pub(in crate::checker::module) fn check_known_disjoint_strict_equality(
        &mut self,
        operator: &Token,
        left: &Type,
        right: &Type,
        span: &SourceSpan,
    ) {
        if is_strict_equality_primitive_type(left)
            && is_strict_equality_primitive_type(right)
            && left != right
        {
            self.type_error(
                span,
                format!(
                    "operator `{}` compares disjoint types `{}` and `{}`",
                    operator.text,
                    type_label(left),
                    type_label(right),
                ),
                DiagnosticCode::TypeMismatch,
            );
        }
    }

    pub(in crate::checker::module) fn check_known_numeric_operands(
        &mut self,
        operator: &str,
        left: &Type,
        right: &Type,
        span: &SourceSpan,
    ) {
        if !is_known_primitive_type(left) || !is_known_primitive_type(right) {
            return;
        }
        if left != &Type::Number || right != &Type::Number {
            self.type_error(
                span,
                format!(
                    "operator `{operator}` cannot be applied to types `{}` and `{}`",
                    type_label(left),
                    type_label(right),
                ),
                DiagnosticCode::TypeMismatch,
            );
        }
    }
}
