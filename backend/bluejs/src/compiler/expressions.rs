// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

use super::*;

impl Compiler {
    pub(super) fn expression(&mut self, expr: &Expr) -> Result<(), CompileError> {
        if optional_chain_root(expr) {
            let mut exits = Vec::new();
            self.optional_chain_expression(expr, &mut exits)?;
            let end = self.offset()?;
            for exit in exits {
                self.patch(exit, end);
            }
            Ok(())
        } else {
            self.expression_plain(expr)
        }
    }

    pub(super) fn expression_plain(&mut self, expr: &Expr) -> Result<(), CompileError> {
        match expr {
            Expr::RegExp { pattern, flags } => {
                self.constant(Value::String(pattern.clone()))?;
                self.constant(Value::String(flags.clone()))?;
                self.emit(Opcode::RegExpLiteral, 0)?;
            }
            Expr::TaggedTemplate {
                tag,
                raw,
                cooked,
                expressions,
            } => {
                if matches!(&**tag, Expr::Member { .. }) {
                    if private_member_name(tag).is_some() {
                        let owner = self.private_member_reference(tag)?;
                        self.emit(Opcode::PrivateGetMethod, owner)?;
                    } else {
                        self.member_reference(tag)?;
                        self.emit(Opcode::GetMethod, 0)?;
                    }
                } else {
                    self.expression(tag)?;
                    self.constant(Value::Undefined)?;
                }
                static NEXT_SITE: std::sync::atomic::AtomicU64 =
                    std::sync::atomic::AtomicU64::new(1);
                let id = NEXT_SITE
                    .fetch_update(
                        std::sync::atomic::Ordering::Relaxed,
                        std::sync::atomic::Ordering::Relaxed,
                        |n| n.checked_add(1),
                    )
                    .map_err(|_| CompileError::ProgramTooLarge)?;
                let site = self.bytecode.templates.len() as u32;
                self.bytecode.templates.push(crate::bytecode::TemplateSite {
                    id,
                    raw: raw.clone(),
                    cooked: cooked.clone(),
                });
                self.emit(Opcode::TemplateObject, site)?;
                for expression in expressions {
                    self.expression(expression)?;
                }
                self.emit(Opcode::Call, expressions.len() as u32 + 1)?;
            }
            Expr::Number(n) => self.constant(Value::Number(*n))?,
            Expr::BigInt(n) => self.constant(Value::BigInt(n.clone()))?,
            Expr::String(s) => self.constant(Value::String(s.clone()))?,
            Expr::Bool(b) => self.constant(Value::Bool(*b))?,
            Expr::Null => self.constant(Value::Null)?,
            Expr::Identifier(name) => {
                if self.bytecode.strict && matches!(name.as_str(), "yield" | "let") {
                    return Err(CompileError::InvalidSyntax(
                        "a reserved word cannot be used as an identifier in strict code",
                    ));
                }
                if self.with_depth != 0 {
                    let index = u32::try_from(self.bytecode.constants.len())
                        .map_err(|_| CompileError::ProgramTooLarge)?;
                    self.bytecode
                        .constants
                        .push(Value::String(name.clone().into()));
                    self.emit(Opcode::WithGet, index)?;
                } else if let Some(slot) = self.resolve(name) {
                    self.emit(Opcode::GetBinding, slot)?;
                } else {
                    match name.as_str() {
                        "undefined" => self.constant(Value::Undefined)?,
                        "NaN" => self.constant(Value::Number(f64::NAN))?,
                        "Infinity" => self.constant(Value::Number(f64::INFINITY))?,
                        "String" => {
                            self.emit(Opcode::GlobalString, 0)?;
                        }
                        "Symbol" | "RegExp" | "Object" | "Reflect" | "Math" | "Number"
                        | "Boolean" | "BigInt" | "Array" | "Date" | "Function" | "Proxy"
                        | "globalThis" | "ArrayBuffer" | "SharedArrayBuffer" | "DataView"
                        | "Int8Array" | "Uint8Array" | "Uint8ClampedArray" | "Int16Array"
                        | "Uint16Array" | "Int32Array" | "Uint32Array" | "Float32Array"
                        | "Float64Array" | "BigInt64Array" | "BigUint64Array" | "Atomics"
                        | "Intl" | "Promise" | "Error" | "TypeError" | "eval" | "isNaN"
                        | "isFinite" | "parseInt" | "parseFloat" | "encodeURI"
                        | "encodeURIComponent" | "decodeURI" | "decodeURIComponent" | "JSON"
                        | "RangeError" | "SyntaxError" | "ReferenceError" | "EvalError"
                        | "URIError" | "import" => {
                            let index = self.bytecode.constants.len() as u32;
                            self.bytecode
                                .constants
                                .push(Value::String(name.as_str().into()));
                            self.emit(Opcode::Global, index)?;
                        }
                        _ => {
                            let index = u32::try_from(self.bytecode.constants.len())
                                .map_err(|_| CompileError::ProgramTooLarge)?;
                            self.bytecode
                                .constants
                                .push(Value::String(name.clone().into()));
                            self.emit(Opcode::UnboundName, index)?;
                        }
                    }
                }
            }
            Expr::Unary { op, arg } => {
                if *op == UnaryOp::Void {
                    self.expression(arg)?;
                    self.emit(Opcode::Pop, 0)?;
                    self.constant(Value::Undefined)?;
                    return Ok(());
                }
                let opcode = match op {
                    UnaryOp::Neg => Opcode::Negate,
                    UnaryOp::Plus => Opcode::ToNumber,
                    UnaryOp::Not => Opcode::Not,
                    UnaryOp::BitNot => Opcode::BitNot,
                    UnaryOp::Typeof => Opcode::Typeof,
                    UnaryOp::Delete | UnaryOp::Void => Opcode::DeleteProperty,
                };
                if *op == UnaryOp::Delete {
                    if matches!(&**arg, Expr::Member { .. }) {
                        if private_member_name(arg).is_some() {
                            return Err(CompileError::InvalidSyntax(
                                "cannot delete a private element",
                            ));
                        }
                        self.member_reference(arg)?;
                        self.emit(opcode, 0)?;
                    } else if let Expr::Identifier(name) = &**arg {
                        if self.bytecode.strict {
                            return Err(CompileError::InvalidSyntax(
                                "cannot delete a binding in strict mode",
                            ));
                        }
                        if let Some(slot) = self.resolve(name) {
                            if self.bytecode.dynamic_eval_slots.contains(&slot) {
                                self.emit(Opcode::DeleteDynamicBinding, slot)?;
                            } else {
                                self.constant(Value::Bool(false))?;
                            }
                        } else {
                            let index = u32::try_from(self.bytecode.constants.len())
                                .map_err(|_| CompileError::ProgramTooLarge)?;
                            self.bytecode
                                .constants
                                .push(Value::String(name.clone().into()));
                            self.emit(Opcode::DeleteUnboundName, index)?;
                        }
                    } else {
                        self.expression(arg)?;
                        self.emit(Opcode::Pop, 0)?;
                        self.constant(Value::Bool(true))?;
                    }
                    return Ok(());
                }
                if *op == UnaryOp::Typeof
                    && matches!(&**arg, Expr::Identifier(name) if self.resolve(name).is_none() && !matches!(name.as_str(), "undefined" | "NaN" | "Infinity" | "String" | "Symbol" | "RegExp" | "Object" | "Reflect" | "Math" | "Number" | "Boolean" | "Array" | "Date" | "Function" | "Proxy" | "globalThis" | "ArrayBuffer" | "SharedArrayBuffer" | "DataView" | "Int8Array" | "Uint8Array" | "Uint8ClampedArray" | "Int16Array" | "Uint16Array" | "Int32Array" | "Uint32Array" | "Float32Array" | "Float64Array" | "BigInt64Array" | "BigUint64Array" | "Atomics" | "Intl" | "Error" | "TypeError" | "RangeError" | "SyntaxError" | "ReferenceError" | "EvalError" | "URIError" | "isNaN" | "isFinite" | "parseInt" | "parseFloat" | "encodeURI" | "encodeURIComponent" | "decodeURI" | "decodeURIComponent" | "JSON" | "import"))
                {
                    let Expr::Identifier(name) = &**arg else {
                        unreachable!()
                    };
                    let index = u32::try_from(self.bytecode.constants.len())
                        .map_err(|_| CompileError::ProgramTooLarge)?;
                    self.bytecode
                        .constants
                        .push(Value::String(name.as_str().into()));
                    self.emit(Opcode::TypeofName, index)?;
                } else {
                    self.expression(arg)?;
                    self.emit(opcode, 0)?;
                }
            }
            Expr::Binary { op, left, right } => {
                let opcode = binary_opcode(*op)?;
                self.expression(left)?;
                self.expression(right)?;
                self.emit(opcode, 0)?;
            }
            Expr::PrivateIn { name, object } => {
                let owner = self.resolve_private_name(name)?;
                self.expression(object)?;
                self.constant(Value::String(name.clone().into()))?;
                self.emit(Opcode::PrivateIn, owner)?;
            }
            Expr::Logical { op, left, right } => {
                self.expression(left)?;
                self.emit(Opcode::Dup, 0)?;
                let jump = self.emit(
                    match op {
                        LogicalOp::And => Opcode::JumpIfFalse,
                        LogicalOp::Or => Opcode::JumpIfTrue,
                        LogicalOp::Nullish => Opcode::JumpIfNotNullish,
                    },
                    0,
                )?;
                self.emit(Opcode::Pop, 0)?;
                self.expression(right)?;
                self.patch(jump, self.offset()?);
            }
            Expr::Sequence(expressions) => {
                for (index, expression) in expressions.iter().enumerate() {
                    self.expression(expression)?;
                    if index + 1 != expressions.len() {
                        self.emit(Opcode::Pop, 0)?;
                    }
                }
            }
            Expr::Conditional {
                test,
                consequent,
                alternate,
            } => {
                self.expression(test)?;
                let no = self.emit(Opcode::JumpIfFalse, 0)?;
                self.expression(consequent)?;
                let end = self.emit(Opcode::Jump, 0)?;
                self.patch(no, self.offset()?);
                self.expression(alternate)?;
                self.patch(end, self.offset()?);
            }
            Expr::Array(elements) => {
                if elements
                    .iter()
                    .any(|element| matches!(element, Some(ArrayElement::Spread(_))))
                {
                    self.emit(Opcode::NewArray, 0)?;
                    for element in elements {
                        let kind = match element {
                            None => {
                                self.constant(Value::Undefined)?;
                                1
                            }
                            Some(ArrayElement::Normal(value)) => {
                                self.expression(value)?;
                                0
                            }
                            Some(ArrayElement::Spread(value)) => {
                                self.expression(value)?;
                                2
                            }
                        };
                        self.emit(Opcode::ArrayPush, kind)?;
                    }
                    return Ok(());
                }
                let length =
                    u32::try_from(elements.len()).map_err(|_| CompileError::ProgramTooLarge)?;
                self.emit(Opcode::NewArray, length)?;
                for (index, element) in elements.iter().enumerate() {
                    let Some(element) = element else { continue };
                    let ArrayElement::Normal(value) = element else {
                        return Err(CompileError::Unsupported("array spread"));
                    };
                    self.emit(Opcode::Dup, 0)?;
                    self.constant(Value::String(index.to_string().into()))?;
                    self.expression(value)?;
                    self.emit(Opcode::DefineData, 0)?;
                    self.emit(Opcode::Pop, 0)?;
                }
            }
            Expr::Object(properties) => {
                self.emit(Opcode::NewObject, 0)?;
                let mut has_proto = false;
                for property in properties {
                    if let ObjectProp::Spread(value) = property {
                        self.expression(value)?;
                        self.emit(Opcode::CopyDataProperties, 0)?;
                        continue;
                    }
                    if let ObjectProp::Method { key, function }
                    | ObjectProp::Accessor { key, function, .. } = property
                    {
                        self.emit(Opcode::Dup, 0)?;
                        self.property_key(key)?;
                        self.function(function, false)?;
                        std::rc::Rc::get_mut(self.bytecode.functions.last_mut().unwrap())
                            .unwrap()
                            .constructible = false;
                        if let ObjectProp::Accessor { getter, .. } = property {
                            self.emit(Opcode::DefineAccessor, u32::from(!getter))?;
                        } else {
                            // The operand distinguishes object-literal
                            // methods (enumerable) from class methods.
                            self.emit(Opcode::DefineMethod, 1)?;
                        }
                        self.emit(Opcode::Pop, 0)?;
                        continue;
                    }
                    let ObjectProp::KeyValue {
                        key,
                        value,
                        shorthand,
                    } = property
                    else {
                        unreachable!("spread is handled above")
                    };
                    self.emit(Opcode::Dup, 0)?;
                    let prototype_key = match key {
                        PropertyKey::Identifier(name) => name == "__proto__",
                        PropertyKey::String(name) => name == "__proto__",
                        _ => false,
                    };
                    if !shorthand && prototype_key {
                        if has_proto {
                            return Err(CompileError::InvalidSyntax(
                                "duplicate literal __proto__ setter",
                            ));
                        }
                        has_proto = true;
                        self.expression(value)?;
                        self.emit(Opcode::SetLiteralPrototype, 0)?;
                    } else {
                        self.property_key(key)?;
                        self.expression(value)?;
                        self.emit(Opcode::DefineData, 0)?;
                        self.emit(Opcode::Pop, 0)?;
                    }
                }
            }
            Expr::Super => {
                return Err(CompileError::InvalidSyntax(
                    "super must be used as a property access or constructor call",
                ))
            }
            Expr::ImportMeta => {
                if !self.bytecode.import_meta_allowed {
                    return Err(CompileError::InvalidSyntax(
                        "import.meta is only valid in module code",
                    ));
                }
                self.emit(Opcode::ImportMeta, 0)?;
            }
            Expr::Member {
                object,
                property,
                computed,
            } if matches!(&**object, Expr::Super) => {
                self.super_property_key(property, *computed)?;
                self.emit(Opcode::SuperGet, 0)?;
            }
            Expr::Member { .. } if private_member_name(expr).is_some() => {
                let owner = self.private_member_reference(expr)?;
                self.emit(Opcode::PrivateGet, owner)?;
            }
            Expr::Member { .. } => {
                self.member_reference(expr)?;
                self.emit(Opcode::GetProperty, 0)?;
            }
            Expr::Parenthesized(expr) => self.expression(expr)?,
            Expr::OptionalMember { .. } => {
                return Err(CompileError::Unsupported("optional chaining"))
            }
            Expr::Assign { op, target, value } => self.assignment(*op, target, value)?,
            Expr::DestructureAssign { pattern, value } => {
                self.destructuring_assignment(pattern, value)?
            }
            Expr::Update { op, arg, prefix } => {
                if let Expr::Identifier(name) = &**arg {
                    let binding = self.resolve(name);
                    let name_index = if binding.is_none() {
                        Some(self.name_constant(name)?)
                    } else {
                        None
                    };
                    if let Some(slot) = binding {
                        self.emit(Opcode::ResolveBindingReference, slot)?;
                        self.emit(Opcode::LoadBindingReference, 0)?;
                    } else {
                        self.emit(Opcode::UnboundName, name_index.unwrap())?;
                    }
                    self.emit(Opcode::ToNumber, 0)?;
                    if !prefix {
                        self.emit(Opcode::Dup, 0)?;
                    }
                    self.constant(Value::Number(1.0))?;
                    self.emit(
                        if *op == UpdateOp::Inc {
                            Opcode::Add
                        } else {
                            Opcode::Subtract
                        },
                        0,
                    )?;
                    if binding.is_some() {
                        // Postfix update keeps the previous numeric value on
                        // the stack as its expression result.
                        self.emit(Opcode::StoreBindingReference, u32::from(!*prefix))?;
                    } else {
                        self.emit(Opcode::SetUnboundName, name_index.unwrap())?;
                    }
                    if !prefix {
                        self.emit(Opcode::Pop, 0)?;
                    }
                } else if let Expr::Member {
                    object,
                    property,
                    computed,
                } = arg.as_ref()
                {
                    if matches!(&**object, Expr::Super) {
                        self.super_property_key(property, *computed)?;
                        self.emit(
                            Opcode::SuperUpdate,
                            u32::from(*op == UpdateOp::Dec) | (u32::from(*prefix) << 1),
                        )?;
                    } else {
                        self.member_reference(arg)?;
                        self.emit(
                            Opcode::UpdateProperty,
                            u32::from(*op == UpdateOp::Dec) | (u32::from(*prefix) << 1),
                        )?;
                    }
                } else if matches!(&**arg, Expr::Call { .. }) {
                    if self.bytecode.strict {
                        return Err(CompileError::InvalidSyntax(
                            "a CallExpression cannot be an assignment target in strict code",
                        ));
                    }
                    self.expression(arg)?;
                    self.emit(Opcode::InvalidAssignmentTarget, 0)?;
                } else {
                    return Err(CompileError::InvalidSyntax("invalid assignment/member AST"));
                }
            }
            Expr::Template {
                quasis,
                expressions,
            } => {
                if quasis.len() != expressions.len() + 1 {
                    return Err(CompileError::InvalidSyntax("invalid template AST"));
                }
                self.constant(Value::String(quasis[0].clone()))?;
                for (expr, tail) in expressions.iter().zip(&quasis[1..]) {
                    self.expression(expr)?;
                    self.emit(Opcode::ToString, 0)?;
                    self.emit(Opcode::Add, 0)?;
                    self.constant(Value::String(tail.clone()))?;
                    self.emit(Opcode::Add, 0)?;
                }
            }
            Expr::Call { callee, args } | Expr::New { callee, args } => {
                let construct = matches!(expr, Expr::New { .. });
                if !construct && matches!(&**callee, Expr::Super) {
                    if args.iter().any(|arg| matches!(arg, Argument::Spread(_))) {
                        self.emit(Opcode::NewArray, 0)?;
                        for arg in args {
                            let (value, kind) = match arg {
                                Argument::Normal(value) => (value, 0),
                                Argument::Spread(value) => (value, 2),
                            };
                            self.expression(value)?;
                            self.emit(Opcode::ArrayPush, kind)?;
                        }
                        self.emit(Opcode::SuperCallSpread, 0)?;
                    } else {
                        for arg in args {
                            let Argument::Normal(expr) = arg else {
                                unreachable!("super call spreads take the array path")
                            };
                            self.expression(expr)?;
                        }
                        self.emit(
                            Opcode::SuperCall,
                            u32::try_from(args.len()).map_err(|_| CompileError::ProgramTooLarge)?,
                        )?;
                    }
                    return Ok(());
                }
                if !construct
                    && matches!(&**callee, Expr::Member { object, .. } if matches!(&**object, Expr::Super))
                {
                    let Expr::Member {
                        property, computed, ..
                    } = callee.as_ref()
                    else {
                        unreachable!()
                    };
                    self.super_property_key(property, *computed)?;
                    self.emit(Opcode::SuperGetMethod, 0)?;
                } else if !construct
                    && matches!(&**callee, Expr::Parenthesized(inner) if matches!(inner.as_ref(), Expr::Member { .. } | Expr::OptionalMember { .. }))
                {
                    let Expr::Parenthesized(inner) = callee.as_ref() else {
                        unreachable!()
                    };
                    self.parenthesized_optional_member_method(inner)?;
                } else if !construct && matches!(&**callee, Expr::Member { .. }) {
                    if private_member_name(callee).is_some() {
                        let owner = self.private_member_reference(callee)?;
                        self.emit(Opcode::PrivateGetMethod, owner)?;
                    } else {
                        self.member_reference(callee)?;
                        self.emit(Opcode::GetMethod, 0)?;
                    }
                } else {
                    self.expression(callee)?;
                    self.constant(Value::Undefined)?;
                }
                if args.iter().any(|arg| matches!(arg, Argument::Spread(_))) {
                    self.emit(Opcode::NewArray, 0)?;
                    for arg in args {
                        let (value, kind) = match arg {
                            Argument::Normal(value) => (value, 0),
                            Argument::Spread(value) => (value, 2),
                        };
                        self.expression(value)?;
                        self.emit(Opcode::ArrayPush, kind)?;
                    }
                    self.emit(
                        if !construct
                            && matches!(&**callee, Expr::Identifier(name) if name == "eval")
                        {
                            Opcode::DirectEvalSpread
                        } else {
                            Opcode::CallSpread
                        },
                        u32::from(construct),
                    )?;
                    return Ok(());
                }
                for arg in args {
                    let Argument::Normal(expr) = arg else {
                        unreachable!("spread calls are emitted above")
                    };
                    self.expression(expr)?;
                }
                self.emit(
                    if construct {
                        Opcode::Construct
                    } else if matches!(&**callee, Expr::Identifier(name) if name == "eval") {
                        Opcode::DirectEval
                    } else {
                        Opcode::Call
                    },
                    u32::try_from(args.len()).map_err(|_| CompileError::ProgramTooLarge)?,
                )?;
            }
            Expr::OptionalCall { .. } => unreachable!("optional calls are compiled by expression"),
            Expr::This => {
                self.emit(Opcode::This, 0)?;
            }
            Expr::NewTarget => {
                if !self.bytecode.new_target_allowed {
                    return Err(CompileError::InvalidSyntax(
                        "new.target is not valid in this context",
                    ));
                }
                self.emit(Opcode::NewTarget, 0)?;
            }
            Expr::Function(function) => self.function_expression(function)?,
            Expr::Class(class) => self.class_expression(class, None)?,
            Expr::Yield { value, delegate } => {
                if !self.bytecode.generator {
                    return Err(CompileError::InvalidSyntax(
                        "yield requires a generator function",
                    ));
                }
                if *delegate {
                    let value = value
                        .as_deref()
                        .ok_or(CompileError::InvalidSyntax("yield* requires an operand"))?;
                    self.expression(value)?;
                    if !self.bytecode.async_function {
                        // Keep the iterator record beneath the yielded value.
                        // On resumption the ordinary generator receives its
                        // next argument above that record, which IteratorNext
                        // forwards to the delegate on the following turn.
                        self.emit(Opcode::GetIterator, 0)?;
                        self.constant(Value::Undefined)?;
                        let next = self.offset()?;
                        self.emit(Opcode::IteratorNext, 1)?;
                        let done = self.emit(Opcode::IteratorStepValue, 0)?;
                        self.emit(Opcode::Yield, 0)?;
                        let resume = self.offset()?;
                        self.emit(Opcode::Jump, next)?;
                        let exit = self.offset()?;
                        self.patch(done, exit);
                        self.bytecode.yield_delegates.push((resume, exit));
                        return Ok(());
                    }
                    // Keep the async iterator record beneath the yielded
                    // value. A resumed generator receives its next argument
                    // above that record, which AsyncIteratorNext forwards to
                    // the delegate on the following loop turn.
                    self.emit(Opcode::GetAsyncIterator, 0)?;
                    self.constant(Value::Undefined)?;
                    let next = self.offset()?;
                    self.emit(Opcode::AsyncIteratorNext, 1)?;
                    self.emit(Opcode::Await, 0)?;
                    let done = self.emit(Opcode::AsyncIteratorStepValue, 0)?;
                    self.emit(Opcode::Yield, 0)?;
                    let resume = self.offset()?;
                    self.emit(Opcode::Jump, next)?;
                    let exit = self.offset()?;
                    self.patch(done, exit);
                    self.bytecode.async_yield_delegates.push((resume, exit));
                    return Ok(());
                }
                if let Some(value) = value {
                    self.expression(value)?;
                } else {
                    self.constant(Value::Undefined)?;
                }
                self.emit(Opcode::Yield, 0)?;
            }
            Expr::Await(expression) => {
                if !self.bytecode.async_function && !self.bytecode.module {
                    return Err(CompileError::InvalidSyntax(
                        "await is only valid in async functions or modules",
                    ));
                }
                self.expression(expression)?;
                self.emit(Opcode::Await, 0)?;
            }
            Expr::DynamicImport(specifier) => {
                self.expression(specifier)?;
                self.emit(Opcode::DynamicImport, 0)?;
            }
            Expr::Arrow {
                params,
                body,
                is_async,
            } => {
                let body = match body {
                    ArrowBody::Expr(expr) => vec![Stmt::Return(Some(*expr.clone()))],
                    ArrowBody::Block(body) => body.clone(),
                };
                self.function(
                    &Function {
                        name: None,
                        params: params.clone(),
                        body,
                        generator: false,
                        is_async: *is_async,
                    },
                    true,
                )?;
            }
        }
        Ok(())
    }

    /// Compile an optional chain as one short-circuiting expression.  The
    /// parser keeps every suffix as an ordinary AST node except the `?.`
    /// member itself, so a nullish result must bypass *all* following member
    /// accesses and calls in the same chain rather than merely its immediate
    /// property lookup.
    pub(super) fn optional_chain_expression(
        &mut self,
        expr: &Expr,
        exits: &mut Vec<usize>,
    ) -> Result<(), CompileError> {
        match expr {
            Expr::Member {
                object,
                property,
                computed,
            } if matches!(&**object, Expr::Super) => {
                self.super_property_key(property, *computed)?;
                self.emit(Opcode::SuperGet, 0)?;
            }
            Expr::Member { .. } if private_member_name(expr).is_some() => {
                let owner = self.private_member_reference(expr)?;
                self.emit(Opcode::PrivateGet, owner)?;
            }
            Expr::Member { .. } | Expr::OptionalMember { .. } => {
                self.optional_chain_member_reference(expr, exits)?;
                self.emit(Opcode::GetProperty, 0)?;
            }
            Expr::Call { callee, args } | Expr::OptionalCall { callee, args } => {
                let optional_call = matches!(expr, Expr::OptionalCall { .. });
                if matches!(&**callee, Expr::Member { object, .. } if matches!(&**object, Expr::Super))
                {
                    let Expr::Member {
                        property, computed, ..
                    } = callee.as_ref()
                    else {
                        unreachable!()
                    };
                    self.super_property_key(property, *computed)?;
                    self.emit(Opcode::SuperGetMethod, 0)?;
                } else if matches!(
                    callee.as_ref(),
                    Expr::Member { .. } | Expr::OptionalMember { .. }
                ) {
                    self.optional_chain_member_reference(callee, exits)?;
                    self.emit(Opcode::GetMethod, 0)?;
                } else if matches!(&**callee, Expr::Parenthesized(inner) if matches!(inner.as_ref(), Expr::Member { .. } | Expr::OptionalMember { .. }))
                {
                    let Expr::Parenthesized(inner) = callee.as_ref() else {
                        unreachable!()
                    };
                    self.parenthesized_optional_member_method(inner)?;
                } else if optional_chain_root(callee) {
                    self.optional_chain_expression(callee, exits)?;
                    self.constant(Value::Undefined)?;
                } else {
                    self.expression(callee)?;
                    self.constant(Value::Undefined)?;
                }
                if optional_call {
                    // GetMethod leaves [callee, receiver]. Test the callee
                    // before evaluating arguments, then restore Call's
                    // ordinary [callee, receiver] layout.
                    self.emit(Opcode::Swap, 0)?;
                    self.emit(Opcode::Dup, 0)?;
                    let non_nullish = self.emit(Opcode::JumpIfNotNullish, 0)?;
                    self.emit(Opcode::Pop, 0)?;
                    self.emit(Opcode::Pop, 0)?;
                    self.constant(Value::Undefined)?;
                    exits.push(self.emit(Opcode::Jump, 0)?);
                    self.patch(non_nullish, self.offset()?);
                    self.emit(Opcode::Swap, 0)?;
                }
                if args.iter().any(|arg| matches!(arg, Argument::Spread(_))) {
                    self.emit(Opcode::NewArray, 0)?;
                    for arg in args {
                        let (value, kind) = match arg {
                            Argument::Normal(value) => (value, 0),
                            Argument::Spread(value) => (value, 2),
                        };
                        self.expression(value)?;
                        self.emit(Opcode::ArrayPush, kind)?;
                    }
                    self.emit(Opcode::CallSpread, 0)?;
                } else {
                    for arg in args {
                        let Argument::Normal(value) = arg else {
                            unreachable!("spread arguments use CallSpread")
                        };
                        self.expression(value)?;
                    }
                    self.emit(
                        Opcode::Call,
                        u32::try_from(args.len()).map_err(|_| CompileError::ProgramTooLarge)?,
                    )?;
                }
            }
            _ => self.expression_plain(expr)?,
        }
        Ok(())
    }

    pub(super) fn optional_chain_member_reference(
        &mut self,
        expr: &Expr,
        exits: &mut Vec<usize>,
    ) -> Result<(), CompileError> {
        let (object, property, computed, optional) = match expr {
            Expr::Member {
                object,
                property,
                computed,
            } => (object, property, *computed, false),
            Expr::OptionalMember {
                object,
                property,
                computed,
            } => (object, property, *computed, true),
            _ => {
                return Err(CompileError::InvalidSyntax(
                    "invalid optional-chain member AST",
                ))
            }
        };
        if optional_chain_root(object) {
            self.optional_chain_expression(object, exits)?;
        } else {
            self.expression(object)?;
        }
        if optional {
            // Keep the base below the test.  A nullish base becomes the
            // chain's undefined result and jumps beyond every remaining
            // suffix; a non-nullish base remains for its property Reference.
            self.emit(Opcode::Dup, 0)?;
            let non_nullish = self.emit(Opcode::JumpIfNotNullish, 0)?;
            self.emit(Opcode::Pop, 0)?;
            self.constant(Value::Undefined)?;
            exits.push(self.emit(Opcode::Jump, 0)?);
            self.patch(non_nullish, self.offset()?);
        }
        if computed {
            self.expression(property)?;
        } else if let Expr::Identifier(name) = property.as_ref() {
            self.constant(Value::String(name.clone().into()))?;
        } else {
            return Err(CompileError::InvalidSyntax(
                "invalid non-computed optional-chain member AST",
            ));
        }
        self.emit(Opcode::PreparePropertyReference, 0)?;
        Ok(())
    }

    /// A parenthesized OptionalMember ends its own chain but still retains a
    /// Reference when used as a call callee: `(object?.method)()` must call
    /// with `object` as `this`.  A nullish member becomes an ordinary call of
    /// `undefined`, which then raises TypeError as required.
    pub(super) fn parenthesized_optional_member_method(
        &mut self,
        expr: &Expr,
    ) -> Result<(), CompileError> {
        if matches!(expr, Expr::Member { .. }) {
            self.member_reference(expr)?;
            self.emit(Opcode::GetMethod, 0)?;
            return Ok(());
        }
        let Expr::OptionalMember {
            object,
            property,
            computed,
        } = expr
        else {
            return Err(CompileError::InvalidSyntax(
                "invalid parenthesized optional member AST",
            ));
        };
        self.expression(object)?;
        self.emit(Opcode::Dup, 0)?;
        let non_nullish = self.emit(Opcode::JumpIfNotNullish, 0)?;
        self.emit(Opcode::Pop, 0)?;
        self.constant(Value::Undefined)?;
        self.constant(Value::Undefined)?;
        let end = self.emit(Opcode::Jump, 0)?;
        self.patch(non_nullish, self.offset()?);
        if *computed {
            self.expression(property)?;
        } else if let Expr::Identifier(name) = property.as_ref() {
            self.constant(Value::String(name.clone().into()))?;
        } else {
            return Err(CompileError::InvalidSyntax(
                "invalid non-computed optional-chain member AST",
            ));
        }
        self.emit(Opcode::PreparePropertyReference, 0)?;
        self.emit(Opcode::GetMethod, 0)?;
        self.patch(end, self.offset()?);
        Ok(())
    }

    pub(super) fn for_in(
        &mut self,
        left: &ForHead,
        right: &Expr,
        body: &Stmt,
        labels: Vec<String>,
    ) -> Result<(), CompileError> {
        self.for_each(left, right, body, true, false, labels)
    }

    pub(super) fn for_of(
        &mut self,
        left: &ForHead,
        right: &Expr,
        body: &Stmt,
        is_await: bool,
        labels: Vec<String>,
    ) -> Result<(), CompileError> {
        self.for_each(left, right, body, false, is_await, labels)
    }

    pub(super) fn for_each(
        &mut self,
        left: &ForHead,
        right: &Expr,
        body: &Stmt,
        for_in: bool,
        is_await: bool,
        labels: Vec<String>,
    ) -> Result<(), CompileError> {
        self.emit(Opcode::ClearCompletion, 0)?;
        let (pattern, kind, annex_b_initializer) = match left {
            ForHead::Decl(kind, pattern) => (Some(pattern), Some(*kind), None),
            ForHead::AnnexBVarInit(pattern, initializer) => {
                if self.bytecode.strict || !for_in {
                    return Err(CompileError::InvalidSyntax(
                        "a for-in declaration initializer is valid only in sloppy var code",
                    ));
                }
                (Some(pattern), Some(DeclKind::Var), Some(initializer))
            }
            ForHead::Assignment(_) => (None, None, None),
            ForHead::Expr(_) => (None, None, None),
        };
        let lexical = kind.is_some_and(|kind| kind != DeclKind::Var);
        let mut declarations = vec![("*iterator*".to_owned(), DeclKind::Let)];
        if lexical {
            declarations.extend(
                pattern_names(pattern.expect("declaration heads have a pattern"))
                    .into_iter()
                    .map(|name| (name, kind.unwrap())),
            );
        }
        self.enter_scope(declarations, &BTreeSet::new(), false)?;
        let iterator = self.resolve("*iterator*").unwrap();
        if let Some(initializer) = annex_b_initializer {
            self.expression(initializer)?;
            self.bind_pattern(
                pattern.expect("Annex B initializer has a declaration pattern"),
                DeclKind::Var,
            )?;
        }
        self.expression(right)?;
        if for_in {
            self.emit(Opcode::ForInKeys, 0)?;
        }
        self.emit(
            if is_await {
                Opcode::GetAsyncIterator
            } else {
                Opcode::GetIterator
            },
            0,
        )?;
        self.emit(Opcode::InitializeBinding, iterator)?;
        let start = self.offset()?;
        self.emit(Opcode::GetBinding, iterator)?;
        let exit = if is_await {
            self.emit(Opcode::AsyncIteratorNext, 0)?;
            self.emit(Opcode::Await, 0)?;
            self.emit(Opcode::AsyncIteratorStep, 0)?
        } else {
            self.emit(Opcode::IteratorStep, 0)?
        };
        self.loops.push(Loop {
            labels,
            breakable: true,
            scope_depth: self.scopes.len(),
            breaks: Vec::new(),
            continues: Some(Vec::new()),
            iterator: Some(iterator),
        });
        if lexical {
            self.enter_scope(
                pattern_names(pattern.expect("declaration heads have a pattern"))
                    .into_iter()
                    .map(|name| (name, kind.unwrap()))
                    .collect(),
                &BTreeSet::new(),
                false,
            )?;
        }
        match left {
            ForHead::Decl(kind, pattern) => self.bind_pattern(pattern, *kind)?,
            ForHead::AnnexBVarInit(pattern, _) => self.bind_pattern(pattern, DeclKind::Var)?,
            ForHead::Assignment(pattern) => self.assign_pattern(pattern)?,
            ForHead::Expr(target) => {
                if self.bytecode.strict {
                    return Err(CompileError::InvalidSyntax(
                        "a CallExpression cannot be an assignment target in strict code",
                    ));
                }
                self.expression(target)?;
                self.emit(Opcode::InvalidAssignmentTarget, 0)?;
            }
        }
        self.statement(body, false)?;
        if lexical {
            self.leave_scope()?;
        }
        self.emit(Opcode::Jump, start)?;
        let end = self.offset()?;
        self.patch(exit, end);
        let context = self.loops.pop().unwrap();
        for (jump, control) in context.breaks {
            self.patch(jump, end);
            self.bytecode.abrupt_jumps[control].target = end;
        }
        for (jump, control) in context.continues.expect("for-of loop has continue targets") {
            self.patch(jump, start);
            self.bytecode.abrupt_jumps[control].target = start;
        }
        self.leave_scope()?;
        Ok(())
    }

    pub(super) fn assignment(
        &mut self,
        op: AssignOp,
        target: &Expr,
        value: &Expr,
    ) -> Result<(), CompileError> {
        let logical_assignment = is_logical_assignment(op);
        // AssignmentExpression gives an anonymous function definition the
        // syntactic IdentifierReference target's name. Member references and
        // compound assignments deliberately do not participate.
        let inferred_name = match (op, target) {
            (AssignOp::Assign, Expr::Identifier(name)) if !logical_assignment => {
                Some(name.as_str())
            }
            (_, Expr::Identifier(name)) if logical_assignment => Some(name.as_str()),
            _ => None,
        };
        // A CoverParenthesizedExpression can still evaluate to a reference,
        // but it is not an IdentifierReference for SetFunctionName.
        let target = match target {
            Expr::Parenthesized(inner) => inner.as_ref(),
            target => target,
        };
        if matches!(target, Expr::Call { .. }) {
            if self.bytecode.strict {
                return Err(CompileError::InvalidSyntax(
                    "a CallExpression cannot be an assignment target in strict code",
                ));
            }
            // Annex B's web-compat extension evaluates the call but never
            // evaluates the RHS or performs coercion on the returned value.
            self.expression(target)?;
            self.emit(Opcode::InvalidAssignmentTarget, 0)?;
            return Ok(());
        }
        if let Expr::Member {
            object,
            property,
            computed,
        } = target
        {
            if matches!(&**object, Expr::Super) {
                self.super_property_key(property, *computed)?;
                if logical_assignment {
                    self.emit(Opcode::Dup, 0)?;
                    self.emit(Opcode::SuperGet, 0)?;
                    self.logical_assignment(op, 1, value, inferred_name, Opcode::SuperSet, 0)?;
                    return Ok(());
                }
                if op != AssignOp::Assign {
                    self.emit(Opcode::Dup, 0)?;
                    self.emit(Opcode::SuperGet, 0)?;
                }
                self.expression_with_name(value, inferred_name)?;
                if let Some(opcode) = compound_assignment_opcode(op) {
                    self.emit(opcode, 0)?;
                }
                self.emit(Opcode::SuperSet, 0)?;
                return Ok(());
            }
        }
        if private_member_name(target).is_some() {
            let owner = self.private_member_reference(target)?;
            if logical_assignment {
                self.emit(Opcode::Dup2, 0)?;
                self.emit(Opcode::PrivateGet, owner)?;
                self.logical_assignment(op, 2, value, inferred_name, Opcode::PrivateSet, owner)?;
                return Ok(());
            }
            if op != AssignOp::Assign {
                self.emit(Opcode::Dup2, 0)?;
                self.emit(Opcode::PrivateGet, owner)?;
            }
            self.expression_with_name(value, inferred_name)?;
            if let Some(opcode) = compound_assignment_opcode(op) {
                self.emit(opcode, 0)?;
            }
            self.emit(Opcode::PrivateSet, owner)?;
            return Ok(());
        }
        if let Expr::Identifier(name) = target {
            if self.with_depth != 0 {
                let index = self.name_constant(name)?;
                // Resolve the object-environment binding before evaluating
                // the RHS. A deletion or eval in that RHS must not redirect
                // PutValue to a later binding lookup.
                self.emit(Opcode::ResolveWithReference, index)?;
                if logical_assignment {
                    self.emit(Opcode::LoadWithReference, 0)?;
                    self.logical_assignment(
                        op,
                        2,
                        value,
                        inferred_name,
                        Opcode::StoreWithReference,
                        0,
                    )?;
                    return Ok(());
                }
                if op != AssignOp::Assign {
                    self.emit(Opcode::LoadWithReference, 0)?;
                }
                self.expression_with_name(value, inferred_name)?;
                if let Some(opcode) = compound_assignment_opcode(op) {
                    self.emit(opcode, 0)?;
                }
                self.emit(Opcode::StoreWithReference, 0)?;
                return Ok(());
            }
        }
        if let Expr::Identifier(name) = target {
            if self.resolve(name).is_none() {
                let index = self.name_constant(name)?;
                if logical_assignment {
                    self.emit(Opcode::UnboundName, index)?;
                    self.logical_assignment(
                        op,
                        0,
                        value,
                        inferred_name,
                        Opcode::SetUnboundName,
                        index,
                    )?;
                    return Ok(());
                }
                if op != AssignOp::Assign {
                    self.emit(Opcode::UnboundName, index)?;
                }
                self.expression_with_name(value, inferred_name)?;
                if let Some(opcode) = compound_assignment_opcode(op) {
                    self.emit(opcode, 0)?;
                }
                self.emit(Opcode::SetUnboundName, index)?;
                return Ok(());
            }
        }
        let binding = if let Expr::Identifier(name) = target {
            if let Some(slot) = self.resolve(name) {
                Some(slot)
            } else {
                let index = u32::try_from(self.bytecode.constants.len())
                    .map_err(|_| CompileError::ProgramTooLarge)?;
                self.bytecode
                    .constants
                    .push(Value::String("globalThis".into()));
                self.emit(Opcode::Global, index)?;
                self.constant(Value::String(name.clone().into()))?;
                self.emit(Opcode::ToPropertyKey, 0)?;
                None
            }
        } else {
            if op == AssignOp::Assign {
                // A simple assignment evaluates the computed property
                // expression with its base first, but ToPropertyKey runs in
                // PutValue after the RHS. Keep the raw key on the stack for
                // SetProperty to convert at that later point.
                self.member_reference_uncoerced(target)?;
            } else {
                self.member_reference(target)?;
            }
            None
        };
        if let Some(slot) = binding {
            // Evaluate an IdentifierReference before the RHS, as required by
            // PutValue. In particular, a sloppy direct eval in the RHS may
            // introduce a same-named var binding, but it cannot retarget the
            // reference that was already resolved here.
            self.emit(Opcode::ResolveBindingReference, slot)?;
        }
        if logical_assignment {
            if binding.is_some() {
                self.emit(Opcode::LoadBindingReference, 0)?;
                self.logical_assignment(
                    op,
                    2,
                    value,
                    inferred_name,
                    Opcode::StoreBindingReference,
                    0,
                )?;
            } else {
                self.emit(Opcode::Dup2, 0)?;
                self.emit(Opcode::GetProperty, 0)?;
                self.logical_assignment(op, 2, value, inferred_name, Opcode::SetProperty, 0)?;
            }
            return Ok(());
        }
        if op != AssignOp::Assign {
            if binding.is_some() {
                self.emit(Opcode::LoadBindingReference, 0)?;
            } else {
                self.emit(Opcode::Dup2, 0)?;
                self.emit(Opcode::GetProperty, 0)?;
            }
        }
        self.expression_with_name(value, inferred_name)?;
        if let Some(opcode) = compound_assignment_opcode(op) {
            self.emit(opcode, 0)?;
        }
        if binding.is_some() {
            self.emit(Opcode::StoreBindingReference, 0)?;
        } else {
            self.emit(Opcode::SetProperty, 0)?;
        }
        Ok(())
    }

    /// The logical-assignment productions retain their original Reference
    /// across the truthiness/nullish decision. A bypass returns the existing
    /// value without evaluating the RHS or invoking PutValue; an assignment
    /// consumes the retained reference with the supplied store opcode.
    pub(super) fn logical_assignment(
        &mut self,
        op: AssignOp,
        reference_values: u32,
        value: &Expr,
        inferred_name: Option<&str>,
        store: Opcode,
        store_operand: u32,
    ) -> Result<(), CompileError> {
        self.emit(Opcode::Dup, 0)?;
        let bypass = self.emit(
            match op {
                AssignOp::LogicalAndAssign => Opcode::JumpIfFalse,
                AssignOp::LogicalOrAssign => Opcode::JumpIfTrue,
                AssignOp::NullishAssign => Opcode::JumpIfNotNullish,
                _ => unreachable!("logical assignment helper has a logical operator"),
            },
            0,
        )?;
        self.emit(Opcode::Pop, 0)?;
        self.expression_with_name(value, inferred_name)?;
        self.emit(store, store_operand)?;
        let done = self.emit(Opcode::Jump, 0)?;
        self.patch(bypass, self.offset()?);
        if reference_values != 0 {
            self.emit(Opcode::DiscardReference, reference_values)?;
        }
        self.patch(done, self.offset()?);
        Ok(())
    }

    /// AssignmentPatternEvaluation. The first copy of the RHS is the
    /// expression's result; the second is consumed by the recursive pattern.
    pub(super) fn destructuring_assignment(
        &mut self,
        pattern: &AssignmentPattern,
        value: &Expr,
    ) -> Result<(), CompileError> {
        self.expression(value)?;
        self.emit(Opcode::Dup, 0)?;
        self.assign_pattern(pattern)
    }

    pub(super) fn assign_pattern(
        &mut self,
        pattern: &AssignmentPattern,
    ) -> Result<(), CompileError> {
        match pattern {
            AssignmentPattern::Target(target) => self.assign_pattern_target(target)?,
            AssignmentPattern::Array(elements) => {
                self.emit(Opcode::GetIterator, 0)?;
                for element in elements {
                    let Some(element) = element else {
                        self.emit(Opcode::IteratorElision, 0)?;
                        continue;
                    };
                    if element.rest {
                        match &element.pattern {
                            AssignmentPattern::Target(target)
                                if matches!(&**target, Expr::Member { .. }) =>
                            {
                                self.emit(Opcode::Dup, 0)?;
                                self.member_reference_uncoerced(target)?;
                                self.emit(Opcode::IteratorRestReference, 0)?;
                                self.assign_prepared_pattern_target(target)?;
                                // IteratorRestReference keeps the original
                                // record below the prepared reference while
                                // collecting. Rest exhaustion marks it done,
                                // so discard that retained record now.
                                self.emit(Opcode::Pop, 0)?;
                            }
                            _ => {
                                self.emit(Opcode::IteratorRest, 0)?;
                                self.assign_pattern(&element.pattern)?;
                            }
                        }
                        return Ok(());
                    }
                    let prepared_member_target = match &element.pattern {
                        AssignmentPattern::Target(target)
                            if matches!(&**target, Expr::Member { .. }) =>
                        {
                            self.emit(Opcode::Dup, 0)?;
                            self.member_reference_uncoerced(target)?;
                            self.array_pattern_reference_value()?;
                            Some(target.as_ref())
                        }
                        _ => {
                            self.array_pattern_value()?;
                            None
                        }
                    };
                    self.assignment_pattern_default(element.default.as_ref(), &element.pattern)?;
                    if let Some(target) = prepared_member_target {
                        self.assign_prepared_pattern_target(target)?;
                    } else {
                        self.assign_pattern(&element.pattern)?;
                    }
                }
                self.emit(Opcode::IteratorFinish, 0)?;
            }
            AssignmentPattern::Object(properties) => {
                self.emit(Opcode::RequireObject, 0)?;
                self.emit(Opcode::NewArray, 0)?;
                for property in properties {
                    match property {
                        AssignmentPatternProp::KeyValue {
                            key,
                            value,
                            default,
                        } => {
                            self.property_key(key)?;
                            let prepared_member_target = match value {
                                AssignmentPattern::Target(target)
                                    if matches!(&**target, Expr::Member { .. }) =>
                                {
                                    // Preserve the already-coerced source
                                    // key while evaluating the assignment
                                    // target reference before GetV(source,
                                    // key), as KeyedDestructuringAssignment
                                    // Evaluation requires.
                                    self.emit(Opcode::Dup, 0)?;
                                    self.member_reference_uncoerced(target)?;
                                    self.emit(Opcode::DestructurePropertyReference, 0)?;
                                    Some(target.as_ref())
                                }
                                _ => {
                                    self.emit(Opcode::DestructureProperty, 0)?;
                                    None
                                }
                            };
                            self.assignment_pattern_default(default.as_ref(), value)?;
                            if let Some(target) = prepared_member_target {
                                self.assign_prepared_pattern_target(target)?;
                            } else {
                                self.assign_pattern(value)?;
                            }
                        }
                        AssignmentPatternProp::Rest(pattern) => {
                            self.emit(Opcode::ObjectRest, 0)?;
                            self.assign_pattern(pattern)?;
                            return Ok(());
                        }
                    }
                }
                self.emit(Opcode::Pop, 0)?;
                self.emit(Opcode::Pop, 0)?;
            }
        }
        Ok(())
    }

    /// Consumes a leaf value while assigning an existing binding or member;
    /// the outer assignment pattern keeps its duplicate RHS beneath it.
    pub(super) fn assign_pattern_target(&mut self, target: &Expr) -> Result<(), CompileError> {
        if let Expr::Identifier(name) = target {
            if let Some(slot) = self.resolve(name) {
                self.emit(Opcode::StoreBinding, slot)?;
            } else {
                let index = self.name_constant(name)?;
                self.emit(Opcode::SetUnboundName, index)?;
            }
        } else {
            self.member_reference(target)?;
            self.emit(Opcode::SetDestructureProperty, 0)?;
        }
        self.emit(Opcode::Pop, 0)?;
        Ok(())
    }

    /// Completes a member assignment whose object and raw key were evaluated
    /// before IteratorStep. Destructuring requires that ordering, while
    /// ToPropertyKey and PutValue happen only after the element is obtained.
    pub(super) fn assign_prepared_pattern_target(
        &mut self,
        target: &Expr,
    ) -> Result<(), CompileError> {
        if !matches!(target, Expr::Member { .. }) {
            return Err(CompileError::InvalidSyntax(
                "prepared destructuring target must be a member reference",
            ));
        }
        self.emit(Opcode::SetDestructurePropertyReference, 0)?;
        self.emit(Opcode::Pop, 0)?;
        Ok(())
    }

    /// Like [`Self::array_pattern_value`], but an already-evaluated member
    /// reference is above the iterator record on the operand stack.
    pub(super) fn array_pattern_reference_value(&mut self) -> Result<(), CompileError> {
        let exhausted = self.emit(Opcode::IteratorStepReference, 0)?;
        let joined = self.emit(Opcode::Jump, 0)?;
        self.patch(exhausted, self.offset()?);
        self.constant(Value::Undefined)?;
        self.patch(joined, self.offset()?);
        Ok(())
    }

    pub(super) fn member_reference(&mut self, target: &Expr) -> Result<(), CompileError> {
        self.member_reference_with_key(target, true)
    }

    pub(super) fn member_reference_uncoerced(&mut self, target: &Expr) -> Result<(), CompileError> {
        self.member_reference_with_key(target, false)
    }

    pub(super) fn member_reference_with_key(
        &mut self,
        target: &Expr,
        coerce_key: bool,
    ) -> Result<(), CompileError> {
        let Expr::Member {
            object,
            property,
            computed,
        } = target
        else {
            return Err(CompileError::InvalidSyntax("invalid assignment/member AST"));
        };
        if matches!(&**object, Expr::Super) {
            return Err(CompileError::InvalidSyntax(
                "super member requires a dedicated operation",
            ));
        }
        self.expression(object)?;
        if *computed {
            self.expression(property)?;
        } else if let Expr::Identifier(name) = &**property {
            self.constant(Value::String(name.clone().into()))?;
        } else {
            return Err(CompileError::InvalidSyntax(
                "invalid non-computed member AST",
            ));
        }
        if coerce_key {
            // Computed-property Reference evaluation requires the base to be
            // object-coercible before it converts the property key. Keep the
            // resulting canonical key beside the base so a later PutValue
            // does not repeat observable ToPropertyKey work.
            self.emit(Opcode::PreparePropertyReference, 0)?;
        }
        Ok(())
    }

    pub(super) fn private_member_reference(&mut self, target: &Expr) -> Result<u32, CompileError> {
        let Expr::Member {
            object,
            property,
            computed: false,
        } = target
        else {
            return Err(CompileError::InvalidSyntax("invalid private member AST"));
        };
        let Expr::Identifier(name) = property.as_ref() else {
            return Err(CompileError::InvalidSyntax("invalid private member name"));
        };
        let Some(name) = name.strip_prefix('#') else {
            return Err(CompileError::InvalidSyntax("invalid private member name"));
        };
        let owner = self.resolve_private_name(name)?;
        self.expression(object)?;
        self.constant(Value::String(name.into()))?;
        Ok(owner)
    }

    pub(super) fn name_constant(&mut self, name: &str) -> Result<u32, CompileError> {
        let index = u32::try_from(self.bytecode.constants.len())
            .map_err(|_| CompileError::ProgramTooLarge)?;
        self.bytecode.constants.push(Value::String(name.into()));
        Ok(index)
    }

    pub(super) fn super_property_key(
        &mut self,
        property: &Expr,
        computed: bool,
    ) -> Result<(), CompileError> {
        if computed {
            self.expression(property)?;
        } else if let Expr::Identifier(name) = property {
            self.constant(Value::String(name.clone().into()))?;
        } else {
            return Err(CompileError::InvalidSyntax(
                "invalid non-computed super member AST",
            ));
        }
        // SuperGet/SuperSet own ToPropertyKey. Keeping the raw computed key
        // here makes simple `super[key] = rhs` evaluate the RHS before key
        // conversion, as PutValue requires.
        Ok(())
    }

    pub(super) fn property_key(&mut self, key: &PropertyKey) -> Result<(), CompileError> {
        match key {
            PropertyKey::Identifier(name) => self.constant(Value::String(name.clone().into()))?,
            PropertyKey::String(name) => self.constant(Value::String(name.clone()))?,
            PropertyKey::Number(n) => self.constant(Value::Number(*n))?,
            PropertyKey::Computed(expr) => self.expression(expr)?,
        }
        self.emit(Opcode::ToPropertyKey, 0)?;
        Ok(())
    }
}
