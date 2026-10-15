// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

//! Optional source evidence produced by the direct parser itself. A tree is
//! joined to the final AST only after an exact expression comparison.

use super::*;
use std::mem::{discriminant, size_of, Discriminant};

pub(crate) struct ExpressionOrigin {
    kind: Discriminant<bluejs::Expr>,
    start: usize,
    end: usize,
    // Generated JSX scaffolding can carry child evidence without a location.
    mapped: bool,
    // Class bodies use the separate checked-declaration evidence traversal.
    children: Option<Vec<ExpressionOrigin>>,
}

#[derive(Default)]
pub(super) struct OriginCapture {
    pub(super) roots: Vec<ExpressionOrigin>,
    allocated: usize,
}

#[derive(Clone, Copy)]
pub(crate) struct OriginScope {
    pub(super) base: usize,
    pub(super) start: usize,
}

fn invalid() -> BridgeError {
    BridgeError::ProvenanceAttachment("direct expression source evidence is inconsistent".into())
}

/// Immediate executable children, in the order the direct parser constructs
/// them. Noncomputed property names are data, not identifier evaluations.
fn children<'a>(expression: &'a bluejs::Expr, mut visit: impl FnMut(&'a bluejs::Expr)) {
    use bluejs::Expr;
    match expression {
        Expr::Unary { arg, .. } | Expr::Update { arg, .. } => visit(arg),
        Expr::Parenthesized(value) | Expr::Await(value) => visit(value),
        Expr::Yield {
            value: Some(value), ..
        } => visit(value),
        Expr::Binary { left, right, .. } | Expr::Logical { left, right, .. } => {
            visit(left);
            visit(right);
        }
        Expr::Assign { target, value, .. } => {
            visit(target);
            visit(value);
        }
        Expr::Conditional {
            test,
            consequent,
            alternate,
        } => {
            visit(test);
            visit(consequent);
            visit(alternate);
        }
        Expr::Sequence(values) => values.iter().for_each(&mut visit),
        Expr::Template { expressions, .. } => expressions.iter().for_each(&mut visit),
        Expr::Member {
            object,
            property,
            computed,
        }
        | Expr::OptionalMember {
            object,
            property,
            computed,
        } => {
            visit(object);
            if *computed {
                visit(property);
            }
        }
        Expr::Call { callee, args } | Expr::New { callee, args } => {
            visit(callee);
            for argument in args {
                match argument {
                    bluejs::Argument::Normal(value) | bluejs::Argument::Spread(value) => {
                        visit(value)
                    }
                }
            }
        }
        Expr::Array(elements) => {
            for element in elements.iter().flatten() {
                match element {
                    bluejs::ArrayElement::Normal(value) | bluejs::ArrayElement::Spread(value) => {
                        visit(value)
                    }
                }
            }
        }
        Expr::Object(properties) => {
            for property in properties {
                match property {
                    bluejs::ObjectProp::KeyValue { key, value, .. } => {
                        if let bluejs::PropertyKey::Computed(key) = key {
                            visit(key);
                        }
                        visit(value);
                    }
                    bluejs::ObjectProp::Spread(value) => visit(value),
                    // Direct object methods/accessors are not supported.
                    _ => {}
                }
            }
        }
        Expr::PrivateIn { object, .. } => visit(object),
        _ => {}
    }
}

impl ExpressionOrigin {
    pub(crate) fn bind(
        &self,
        expression: &bluejs::Expr,
        visit: &mut impl FnMut(&bluejs::Expr, usize),
    ) -> Result<(), BridgeError> {
        if self.kind != discriminant(expression) {
            return Err(invalid());
        }
        if self.mapped {
            visit(expression, self.start);
        }
        if let Some(origins) = &self.children {
            let mut index = 0;
            let mut result = Ok(());
            children(expression, |child| {
                if result.is_ok() {
                    result = origins
                        .get(index)
                        .ok_or_else(invalid)
                        .and_then(|origin| origin.bind(child, visit));
                }
                index += 1;
            });
            result?;
            if index != origins.len() {
                return Err(invalid());
            }
        }
        Ok(())
    }
}

impl ExpressionLowerer<'_> {
    pub(crate) fn origin_scope_at(&self, start: usize) -> Option<OriginScope> {
        self.origins.as_ref().map(|capture| OriginScope {
            base: capture.roots.len(),
            start,
        })
    }

    pub(super) fn origin_scope(&self) -> Option<OriginScope> {
        self.origins.as_ref().map(|capture| OriginScope {
            base: capture.roots.len(),
            start: self.tokens.get(self.index).map_or(0, |token| token.start),
        })
    }

    pub(super) fn prefix_origin_scope(&self) -> Option<OriginScope> {
        let capture = self.origins.as_ref()?;
        let root = capture.roots.last()?;
        Some(OriginScope {
            base: capture.roots.len() - 1,
            start: root.start,
        })
    }

    pub(super) fn capture_origin(
        &mut self,
        scope: Option<OriginScope>,
        expression: &bluejs::Expr,
        opaque: bool,
    ) -> Result<(), BridgeError> {
        let (Some(scope), Some(capture)) = (scope, &mut self.origins) else {
            return Ok(());
        };
        let end = self
            .tokens
            .get(self.index.checked_sub(1).ok_or_else(invalid)?)
            .ok_or_else(invalid)?
            .end;
        if scope.base > capture.roots.len() || scope.start > end {
            return Err(invalid());
        }
        let kind = discriminant(expression);
        // Grammar precedence tiers that return an unchanged expression do not
        // create nodes. Postfix non-null assertions likewise erase to a child.
        if capture.roots.len() == scope.base + 1 {
            let root = &capture.roots[scope.base];
            if root.kind == kind
                && root.start == scope.start
                && root.end <= end
                && self.tokens
                    [self.tokens.partition_point(|token| token.end <= root.end)..self.index]
                    .iter()
                    .all(|token| token.text == "!")
            {
                return Ok(());
            }
        }
        self.capture_lowered_origin(Some(scope), expression, end, true, opaque)
    }

    /// Explicit bounds belong to a lowered node, independently of the outer
    /// token cursor. Generated nodes remain unmapped while retaining children.
    pub(crate) fn capture_lowered_origin(
        &mut self,
        scope: Option<OriginScope>,
        expression: &bluejs::Expr,
        end: usize,
        mapped: bool,
        opaque: bool,
    ) -> Result<(), BridgeError> {
        let (Some(scope), Some(capture)) = (scope, &mut self.origins) else {
            return Ok(());
        };
        if scope.base > capture.roots.len() || scope.start > end {
            return Err(invalid());
        }
        let kind = discriminant(expression);
        let pending = &capture.roots[scope.base..];
        let mut count = 0;
        let mut consistent = true;
        if !opaque {
            children(expression, |child| {
                consistent &= pending
                    .get(count)
                    .is_some_and(|origin| origin.kind == discriminant(child));
                count += 1;
            });
        }
        if !consistent || count != pending.len() {
            return Err(invalid());
        }
        // Each node can own a child vector and occupy a roots vector slot.
        // Charge a conservative four slots before allocating either vector.
        capture.allocated = capture
            .allocated
            .checked_add(4 * size_of::<ExpressionOrigin>())
            .filter(|bytes| *bytes <= bluejs::diagnostics::MAX_DIAGNOSTIC_SOURCE_MAP_BYTES)
            .ok_or_else(|| {
                BridgeError::ProvenanceAttachment(
                    "direct expression source evidence exceeds its byte budget".into(),
                )
            })?;
        let children = capture.roots.split_off(scope.base);
        capture.roots.push(ExpressionOrigin {
            kind,
            start: scope.start,
            end,
            mapped,
            children: (!opaque).then_some(children),
        });
        Ok(())
    }

    pub(crate) fn parse_with_origins(
        mut self,
    ) -> Result<(bluejs::Expr, ExpressionOrigin), BridgeError> {
        self.origins = Some(OriginCapture::default());
        let (expression, capture) = self.parse_impl()?;
        let mut capture = capture.ok_or_else(invalid)?;
        if capture.roots.len() != 1 {
            return Err(invalid());
        }
        Ok((expression, capture.roots.pop().ok_or_else(invalid)?))
    }
}
