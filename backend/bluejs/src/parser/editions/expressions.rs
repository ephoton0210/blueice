// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

//! Expression features and RegExp grammar, independent of runtime support.

use super::*;
use SyntaxEdition::*;

impl<'a> Validator<'a> {
    pub(super) fn expr(&mut self, expr: &'a Expr, in_function: bool) -> Result<(), ParseError> {
        match expr {
            Expr::Number(_)
            | Expr::String(_)
            | Expr::Bool(_)
            | Expr::Null
            | Expr::This
            | Expr::Identifier(_) => {}
            Expr::BigInt(_) => self.require(Es2020, "BigInt literals")?,
            Expr::Parenthesized(expr)
            | Expr::Unary { arg: expr, .. }
            | Expr::Update { arg: expr, .. } => self.expression(expr, in_function),
            Expr::Template { expressions, .. } | Expr::TaggedTemplate { expressions, .. } => {
                self.require(Es2015, "template literals")?;
                if let Expr::TaggedTemplate { tag, .. } = expr {
                    self.expression(tag, in_function);
                }
                for expression in expressions {
                    self.expression(expression, in_function);
                }
            }
            Expr::RegExp { pattern, flags } => self.regexp(pattern, flags)?,
            Expr::Array(elements) => {
                for element in elements.iter().flatten() {
                    match element {
                        ArrayElement::Normal(expr) => self.expression(expr, in_function),
                        ArrayElement::Spread(expr) => {
                            self.require(Es2015, "array spread")?;
                            self.expression(expr, in_function);
                        }
                    }
                }
            }
            Expr::Object(properties) => {
                for property in properties {
                    match property {
                        ObjectProp::KeyValue {
                            key,
                            value,
                            shorthand,
                        } => {
                            if *shorthand {
                                self.require(Es2015, "shorthand properties")?;
                            }
                            self.push(Node::Key(key), in_function);
                            self.expression(value, in_function);
                        }
                        ObjectProp::Spread(expr) => {
                            self.require(Es2018, "object spread")?;
                            self.expression(expr, in_function);
                        }
                        ObjectProp::Method { key, function }
                        | ObjectProp::Accessor { key, function, .. } => {
                            if matches!(property, ObjectProp::Method { .. }) {
                                self.require(Es2015, "object methods")?;
                            }
                            self.push(Node::Key(key), in_function);
                            self.push(Node::Function(function), in_function);
                        }
                    }
                }
            }
            Expr::Function(function) => self.push(Node::Function(function), in_function),
            Expr::Class(class) => self.push(Node::Class(class), in_function),
            Expr::Super | Expr::NewTarget => self.require(Es2015, "super and new.target")?,
            Expr::Yield { value, .. } => {
                self.require(Es2015, "yield expressions")?;
                self.optional_expression(value.as_deref(), in_function);
            }
            Expr::Await(expr) => {
                self.require(
                    if in_function { Es2017 } else { Es2022 },
                    "await expressions",
                )?;
                self.expression(expr, in_function);
            }
            Expr::DynamicImport {
                specifier,
                options,
                phase,
            } => {
                self.require(Es2020, "dynamic import")?;
                if options.is_some() || *phase != ImportPhase::Evaluation {
                    self.require(EsNext, "import attributes and phases")?;
                }
                self.expression(specifier, in_function);
                self.optional_expression(options.as_deref(), in_function);
            }
            Expr::ImportMeta => self.require(Es2020, "import.meta")?,
            Expr::Arrow {
                params,
                body,
                is_async,
                ..
            } => {
                self.require(Es2015, "arrow functions")?;
                if *is_async {
                    self.require(Es2017, "async arrow functions")?;
                }
                self.parameters(params)?;
                match body {
                    ArrowBody::Expr(expr) => self.expression(expr, true),
                    ArrowBody::Block(body) => self.statements(body, true),
                }
            }
            Expr::Binary { op, left, right } => {
                if *op == BinaryOp::Exponent {
                    self.require(Es2016, "exponentiation")?;
                }
                self.expression(left, in_function);
                self.expression(right, in_function);
            }
            Expr::Logical { op, left, right } => {
                if *op == LogicalOp::Nullish {
                    self.require(Es2020, "nullish coalescing")?;
                }
                self.expression(left, in_function);
                self.expression(right, in_function);
            }
            Expr::Sequence(expressions) => {
                for expr in expressions {
                    self.expression(expr, in_function);
                }
            }
            Expr::Assign { op, target, value } => {
                if *op == AssignOp::ExponentAssign {
                    self.require(Es2016, "exponentiation assignment")?;
                }
                if matches!(
                    op,
                    AssignOp::LogicalAndAssign
                        | AssignOp::LogicalOrAssign
                        | AssignOp::NullishAssign
                ) {
                    self.require(Es2021, "logical assignment")?;
                }
                self.expression(target, in_function);
                self.expression(value, in_function);
            }
            Expr::DestructureAssign { pattern, value } => {
                self.push(Node::AssignmentPattern(pattern), in_function);
                self.expression(value, in_function);
            }
            Expr::Conditional {
                test,
                consequent,
                alternate,
            } => {
                self.expression(test, in_function);
                self.expression(consequent, in_function);
                self.expression(alternate, in_function);
            }
            Expr::Call { callee, args }
            | Expr::New { callee, args }
            | Expr::OptionalCall { callee, args } => {
                if matches!(expr, Expr::OptionalCall { .. }) {
                    self.require(Es2020, "optional calls")?;
                }
                self.expression(callee, in_function);
                for argument in args {
                    match argument {
                        Argument::Normal(expr) => self.expression(expr, in_function),
                        Argument::Spread(expr) => {
                            self.require(Es2015, "argument spread")?;
                            self.expression(expr, in_function);
                        }
                    }
                }
            }
            Expr::Member {
                object, property, ..
            }
            | Expr::OptionalMember {
                object, property, ..
            } => {
                if matches!(expr, Expr::OptionalMember { .. }) {
                    self.require(Es2020, "optional members")?;
                }
                if matches!(property.as_ref(), Expr::Identifier(name) if name.starts_with('#')) {
                    self.require(Es2022, "private names")?;
                }
                self.expression(object, in_function);
                self.expression(property, in_function);
            }
            Expr::PrivateIn { object, .. } => {
                self.require(Es2022, "private brand checks")?;
                self.expression(object, in_function);
            }
        }
        Ok(())
    }

    fn regexp(&self, pattern: &crate::JsString, flags: &crate::JsString) -> Result<(), ParseError> {
        let flags = flags.as_code_units();
        for flag in flags {
            match *flag {
                0x75 | 0x79 => self.require(Es2015, "Unicode and sticky RegExp flags")?,
                0x73 => self.require(Es2018, "dotAll RegExp flags")?,
                0x64 => self.require(Es2022, "RegExp indices")?,
                0x76 => self.require(EsNext, "RegExp Unicode sets")?,
                _ => {}
            }
        }
        let units = pattern.as_code_units();
        let mut index = 0;
        let mut in_class = false;
        while index < units.len() {
            match units[index] {
                0x5c => {
                    // Property escapes are grammar only with Unicode mode.
                    // Escaped brackets/group punctuation never select a new
                    // production; the existing RegExp parser validates them.
                    if flags.contains(&0x75) && matches!(units.get(index + 1), Some(0x70 | 0x50)) {
                        self.require(Es2018, "RegExp Unicode property escapes")?;
                    }
                    index += 2;
                    continue;
                }
                0x5b => in_class = true,
                0x5d => in_class = false,
                0x28 if !in_class
                    && units.get(index + 1) == Some(&0x3f)
                    && units.get(index + 2) == Some(&0x3c) =>
                {
                    self.require(Es2018, "named captures and lookbehind")?;
                }
                _ => {}
            }
            index += 1;
        }
        Ok(())
    }
}
