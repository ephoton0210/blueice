// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

//! Assignment and update targets.

use super::*;

impl<'a> ExpressionLowerer<'a> {
    pub(crate) fn parse_assignment(&mut self) -> Result<bluejs::Expr, BridgeError> {
        // `yield`, `yield* iterable` and `yield value` bind looser than assignment.
        if self
            .tokens
            .get(self.index)
            .is_some_and(|token| token.text == "yield")
        {
            self.index += 1;
            let delegate = self
                .tokens
                .get(self.index)
                .is_some_and(|token| token.text == "*");
            if delegate {
                self.index += 1;
            }
            let has_operand = self.tokens.get(self.index).is_some_and(|token| {
                !matches!(token.text.as_str(), ")" | "]" | "}" | "," | ";" | ":")
            });
            let value = if has_operand {
                Some(Box::new(self.parse_assignment()?))
            } else {
                None
            };
            return Ok(bluejs::Expr::Yield { value, delegate });
        }
        let target = self.parse_conditional()?;
        let op = self
            .tokens
            .get(self.index)
            .and_then(|token| match token.text.as_str() {
                "=" => Some(bluejs::AssignOp::Assign),
                "+=" => Some(bluejs::AssignOp::AddAssign),
                "-=" => Some(bluejs::AssignOp::SubAssign),
                "*=" => Some(bluejs::AssignOp::MulAssign),
                "**=" => Some(bluejs::AssignOp::ExponentAssign),
                "/=" => Some(bluejs::AssignOp::DivAssign),
                "%=" => Some(bluejs::AssignOp::ModAssign),
                "<<=" => Some(bluejs::AssignOp::ShiftLeftAssign),
                ">>=" => Some(bluejs::AssignOp::ShiftRightAssign),
                ">>>=" => Some(bluejs::AssignOp::UnsignedShiftRightAssign),
                "&=" => Some(bluejs::AssignOp::BitAndAssign),
                "^=" => Some(bluejs::AssignOp::BitXorAssign),
                "|=" => Some(bluejs::AssignOp::BitOrAssign),
                "&&=" => Some(bluejs::AssignOp::LogicalAndAssign),
                "||=" => Some(bluejs::AssignOp::LogicalOrAssign),
                "??=" => Some(bluejs::AssignOp::NullishAssign),
                _ => None,
            });
        let Some(op) = op else {
            return Ok(target);
        };
        let span = self
            .tokens
            .get(self.index)
            .map(|token| self.token_span(token))
            .expect("an assignment operator was just inspected");
        self.index += 1;
        if !matches!(
            &target,
            bluejs::Expr::Identifier(_) | bluejs::Expr::Member { .. }
        ) {
            return Err(unsupported(
                span,
                "only identifier and property assignment targets are in the v1 direct bridge subset",
            ));
        }
        Ok(bluejs::Expr::Assign {
            op,
            target: Box::new(target),
            value: Box::new(self.parse_assignment()?),
        })
    }

    pub(crate) fn parse_update(&mut self) -> Result<bluejs::Expr, BridgeError> {
        if let Some(op) = self.update_operator_at(self.index) {
            let span = self
                .tokens
                .get(self.index)
                .map(|token| self.token_span(token))
                .expect("an update operator was just inspected");
            self.index += 1;
            let arg = self.parse_unary()?;
            return self.lower_update(op, arg, true, span);
        }
        let expression = self.parse_primary()?;
        let Some(op) = self.update_operator_at(self.index) else {
            return Ok(expression);
        };
        let span = self
            .tokens
            .get(self.index)
            .map(|token| self.token_span(token))
            .expect("an update operator was just inspected");
        self.index += 1;
        self.lower_update(op, expression, false, span)
    }

    pub(crate) fn update_operator_at(&self, index: usize) -> Option<bluejs::UpdateOp> {
        self.tokens
            .get(index)
            .and_then(|token| match token.text.as_str() {
                "++" => Some(bluejs::UpdateOp::Inc),
                "--" => Some(bluejs::UpdateOp::Dec),
                _ => None,
            })
    }

    pub(crate) fn lower_update(
        &self,
        op: bluejs::UpdateOp,
        arg: bluejs::Expr,
        prefix: bool,
        span: SourceSpan,
    ) -> Result<bluejs::Expr, BridgeError> {
        if !matches!(
            arg,
            bluejs::Expr::Identifier(_) | bluejs::Expr::Member { .. }
        ) {
            return Err(unsupported(
                span,
                "only identifier and property update targets are in the v1 direct bridge subset",
            ));
        }
        Ok(bluejs::Expr::Update {
            op,
            arg: Box::new(arg),
            prefix,
        })
    }
}
