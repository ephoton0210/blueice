// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

use super::*;

impl Compiler {
    pub(super) fn function(
        &mut self,
        function: &Function,
        arrow: bool,
    ) -> Result<(), CompileError> {
        self.function_named(function, arrow, None, false)
    }

    pub(super) fn function_expression(&mut self, function: &Function) -> Result<(), CompileError> {
        self.function_named(function, false, None, function.name.is_some())
    }

    pub(super) fn class_expression(
        &mut self,
        class: &Class,
        inferred_name: Option<&str>,
    ) -> Result<(), CompileError> {
        let Some(name) = class.name.as_ref() else {
            return self.class_expression_with_binding(class, inferred_name, None);
        };
        self.enter_scope(
            vec![(name.clone(), DeclKind::Const)],
            &BTreeSet::new(),
            true,
        )?;
        let binding = self
            .resolve(name)
            .expect("class name was entered into its expression scope");
        let result = self.class_expression_with_binding(class, inferred_name, Some(binding));
        self.leave_scope()?;
        result
    }

    pub(super) fn class_expression_with_binding(
        &mut self,
        class: &Class,
        inferred_name: Option<&str>,
        binding: Option<u32>,
    ) -> Result<(), CompileError> {
        let private_declarations = class_private_declarations(class)?;
        let private_scope_id = self.next_private_scope;
        self.next_private_scope = self.next_private_scope.saturating_add(1);
        let mut private_scope = HashMap::new();
        let mut private_bindings = Vec::new();
        // A class containing only uninitialized fields has no executable
        // class-element source, so no direct eval can observe its individual
        // private lexical names. Every instance (or static) private name has
        // the same owner object; retain one binding per owner kind instead of
        // thousands of identical captures. Generated Unicode identifier
        // fixtures exercise this path with more than eight thousand fields.
        let compact_private_owners = class.elements.iter().all(|element| {
            matches!(
                element,
                ClassElement::Field {
                    initializer: None,
                    ..
                }
            )
        });
        let instance_owner_binding =
            format!("{PRIVATE_OWNER_BINDING_PREFIX}{private_scope_id}_instance_");
        let static_owner_binding =
            format!("{PRIVATE_OWNER_BINDING_PREFIX}{private_scope_id}_static_");
        for (name, is_static) in &private_declarations {
            // This cannot collide with source text (U+0000 is not a source
            // character), while preserving separate lexical environments for
            // nested classes that reuse a private name.
            let binding = if compact_private_owners {
                if *is_static {
                    static_owner_binding.clone()
                } else {
                    instance_owner_binding.clone()
                }
            } else {
                format!(
                    "{PRIVATE_OWNER_BINDING_PREFIX}{private_scope_id}_{}_{}",
                    if *is_static { "static" } else { "instance" },
                    name
                )
            };
            private_scope.insert(name.clone(), binding.clone());
            if !compact_private_owners
                || !private_bindings
                    .iter()
                    .any(|(existing, _)| existing == &binding)
            {
                private_bindings.push((binding, DeclKind::Const));
            }
        }
        let has_private_scope = !private_bindings.is_empty();
        if has_private_scope {
            self.enter_scope(private_bindings, &BTreeSet::new(), false)?;
            self.private_scopes.push(private_scope.clone());
        }
        let constructor = class.elements.iter().find_map(|element| match element {
            ClassElement::Method {
                key,
                function,
                is_static: false,
            } if class_property_name(key).is_some_and(|name| name == "constructor") => {
                Some(function.clone())
            }
            _ => None,
        });
        let default_constructor = constructor.is_none();
        let mut constructor = constructor.unwrap_or(Function {
            name: class.name.clone(),
            params: Vec::new(),
            body: Vec::new(),
            generator: false,
            is_async: false,
        });
        constructor.name = class.name.clone();
        let mut fields: Vec<_> = class
            .elements
            .iter()
            .filter_map(|element| match element {
                ClassElement::Field {
                    key,
                    initializer,
                    is_static: false,
                } => Some(class_instance_field(key, initializer.as_ref())),
                _ => None,
            })
            .collect();
        if let Some((name, _)) = private_declarations
            .iter()
            .find(|(_, is_static)| !*is_static)
        {
            // Private methods and accessors brand each constructed instance
            // even when the class has no private data field.  The marker is
            // deliberately before all instance field initializers, so an
            // earlier public initializer can access a declared private
            // method just as it can in ECMAScript.
            fields.insert(
                0,
                Stmt::ClassPrivateBrand(
                    private_scope
                        .get(name)
                        .expect("private instance declaration has an owner binding")
                        .clone(),
                ),
            );
        }
        let constructor_body = std::mem::take(&mut constructor.body);
        let body = if class.extends.is_some() {
            if default_constructor {
                fields.clone()
            } else {
                derived_constructor_body(constructor_body, fields.clone())?
            }
        } else {
            let mut body = fields.clone();
            body.extend(constructor_body);
            body
        };
        constructor.body = body;
        self.function_named_with(
            &constructor,
            false,
            inferred_name,
            false,
            FunctionCompileOptions {
                constructible: true,
                force_strict: true,
                class_constructor: true,
                derived_constructor: class.extends.is_some(),
                default_derived_constructor: class.extends.is_some() && default_constructor,
                class_method: false,
            },
        )?;
        self.emit(Opcode::SetClassHome, 0)?;
        if let Some(base) = &class.extends {
            self.expression(base)?;
            self.emit(Opcode::SetClassHeritage, 0)?;
        }
        if let Some(slot) = binding {
            self.emit(Opcode::Dup, 0)?;
            self.emit(Opcode::InitializeBinding, slot)?;
        }
        // The class object and its prototype now exist.  Initialize the
        // hidden owner cells before creating element closures, so every
        // ordinary nested function can capture the lexical private-name
        // environment rather than relying on a [[HomeObject]].
        let mut initialized_private_owners = HashSet::new();
        for (name, is_static) in &private_declarations {
            let private_binding = private_scope
                .get(name)
                .expect("private declaration has an owner binding")
                .clone();
            if compact_private_owners && !initialized_private_owners.insert(private_binding.clone())
            {
                continue;
            }
            self.class_property_target(*is_static)?;
            let owner = self
                .resolve(&private_binding)
                .expect("private owner binding is in the active class scope");
            self.emit(Opcode::InitializeBinding, owner)?;
        }
        for element in &class.elements {
            match element {
                ClassElement::Method {
                    key,
                    function,
                    is_static,
                } => {
                    if !is_static
                        && class_property_name(key).is_some_and(|name| name == "constructor")
                    {
                        continue;
                    }
                    self.class_property_target(*is_static)?;
                    if let Some(name) = private_class_name(key) {
                        self.constant(Value::String(name.into()))?;
                    } else {
                        self.property_key(key)?;
                    }
                    self.function_named_with(
                        function,
                        false,
                        None,
                        false,
                        FunctionCompileOptions::class_method(),
                    )?;
                    if private_class_name(key).is_some() {
                        self.emit(Opcode::DefinePrivateMethod, u32::from(*is_static))?;
                    } else {
                        self.emit(Opcode::DefineMethod, 0)?;
                        self.emit(Opcode::Pop, 0)?;
                    }
                }
                ClassElement::Accessor {
                    key,
                    function,
                    getter,
                    is_static,
                } => {
                    self.class_property_target(*is_static)?;
                    if let Some(name) = private_class_name(key) {
                        self.constant(Value::String(name.into()))?;
                    } else {
                        self.property_key(key)?;
                    }
                    self.function_named_with(
                        function,
                        false,
                        None,
                        false,
                        FunctionCompileOptions::class_method(),
                    )?;
                    if private_class_name(key).is_some() {
                        self.emit(
                            Opcode::DefinePrivateAccessor,
                            u32::from(!getter) | (u32::from(*is_static) << 1),
                        )?;
                    } else {
                        self.emit(Opcode::DefineClassAccessor, u32::from(!getter))?;
                        self.emit(Opcode::Pop, 0)?;
                    }
                }
                ClassElement::Field {
                    key,
                    initializer,
                    is_static: true,
                } => {
                    self.class_property_target(true)?;
                    if let Some(name) = private_class_name(key) {
                        self.constant(Value::String(name.into()))?;
                        self.emit(Opcode::DefinePrivateField, 1)?;
                        self.class_property_target(true)?;
                        self.constant(Value::String(name.into()))?;
                    } else {
                        self.property_key(key)?;
                    }
                    let value = initializer.clone().unwrap_or_else(undefined_expression);
                    let initializer = Function {
                        name: None,
                        params: Vec::new(),
                        body: vec![Stmt::Return(Some(value))],
                        generator: false,
                        is_async: false,
                    };
                    self.function_named_with(
                        &initializer,
                        false,
                        None,
                        false,
                        FunctionCompileOptions::class_method(),
                    )?;
                    self.emit(
                        if private_class_name(key).is_some() {
                            Opcode::DefinePrivateStaticField
                        } else {
                            Opcode::DefineClassStaticField
                        },
                        0,
                    )?;
                }
                ClassElement::Field {
                    key,
                    is_static: false,
                    ..
                } => {
                    if private_class_name(key).is_some() {
                        self.class_property_target(false)?;
                        self.constant(Value::String(
                            private_class_name(key)
                                .expect("private field check above")
                                .into(),
                        ))?;
                        self.emit(Opcode::DefinePrivateField, 0)?;
                    }
                }
                ClassElement::StaticBlock(body) => {
                    let block = Function {
                        name: None,
                        params: Vec::new(),
                        body: body.clone(),
                        generator: false,
                        is_async: false,
                    };
                    self.function_named_with(
                        &block,
                        false,
                        None,
                        false,
                        FunctionCompileOptions::class_method(),
                    )?;
                    self.emit(Opcode::CallClassStaticBlock, 0)?;
                }
            }
        }
        if has_private_scope {
            self.private_scopes.pop();
            self.leave_scope()?;
        }
        Ok(())
    }

    pub(super) fn class_property_target(&mut self, is_static: bool) -> Result<(), CompileError> {
        self.emit(Opcode::Dup, 0)?;
        if !is_static {
            self.constant(Value::String("prototype".into()))?;
            self.emit(Opcode::GetProperty, 0)?;
        }
        Ok(())
    }

    pub(super) fn function_named(
        &mut self,
        function: &Function,
        arrow: bool,
        inferred_name: Option<&str>,
        named_expression: bool,
    ) -> Result<(), CompileError> {
        self.function_named_with(
            function,
            arrow,
            inferred_name,
            named_expression,
            FunctionCompileOptions {
                constructible: !arrow && !function.generator && !function.is_async,
                force_strict: false,
                class_constructor: false,
                derived_constructor: false,
                default_derived_constructor: false,
                class_method: false,
            },
        )
    }

    pub(super) fn function_named_with(
        &mut self,
        function: &Function,
        arrow: bool,
        inferred_name: Option<&str>,
        named_expression: bool,
        options: FunctionCompileOptions,
    ) -> Result<(), CompileError> {
        let child_budget = self.max_bytecode_bytes.saturating_sub(self.offset()?);
        let mut child = Compiler {
            bytecode: Bytecode::empty(),
            names: vec![HashMap::new()],
            private_scopes: self.private_scopes.clone(),
            next_private_scope: self.next_private_scope,
            scopes: Vec::new(),
            loops: Vec::new(),
            catch_var_slots: Vec::new(),
            max_bytecode_bytes: child_budget,
            function: true,
            local_scope: 1,
            with_depth: 0,
        };
        child.bytecode.strict =
            options.force_strict || self.bytecode.strict || strict_body(&function.body);
        validate_function_early_errors(
            function,
            child.bytecode.strict,
            !options.class_method && !options.class_constructor,
            arrow,
        )?;
        child.bytecode.arrow = arrow;
        // Arrow functions inherit the containing function's `new.target`
        // syntactic context. A regular nested function introduces its own
        // context (whose runtime value may still be `undefined`).
        child.bytecode.new_target_allowed = !arrow || self.bytecode.new_target_allowed;
        child.bytecode.import_meta_allowed = self.bytecode.import_meta_allowed;
        child.bytecode.generator = function.generator;
        child.bytecode.async_function = function.is_async;
        child.bytecode.constructible = options.constructible;
        child.bytecode.class_constructor = options.class_constructor;
        child.bytecode.derived_constructor = options.derived_constructor;
        child.bytecode.function_name = function
            .name
            .clone()
            .or_else(|| inferred_name.map(str::to_owned))
            .unwrap_or_default();
        child.bytecode.function_length = function
            .params
            .iter()
            .take_while(|p| !p.rest && p.default.is_none())
            .count() as u32;
        let mut visible = std::collections::BTreeMap::new();
        for scope in &self.names {
            visible.extend(scope.iter().map(|(name, slot)| (name.clone(), *slot)));
        }
        for (name, slot) in visible {
            let index = child.bytecode.bindings.len() as u32;
            child.names[0].insert(name, index);
            child
                .bytecode
                .bindings
                .push(self.bytecode.bindings[slot as usize].clone());
            child.bytecode.captures.push(slot);
        }
        if named_expression {
            let name = function
                .name
                .as_ref()
                .expect("named function expression has a name")
                .clone();
            let slot = u32::try_from(child.bytecode.bindings.len())
                .map_err(|_| CompileError::ProgramTooLarge)?;
            child.names[0].insert(name.clone(), slot);
            child.bytecode.bindings.push(Binding {
                name,
                mutable: false,
                strict_immutable: false,
                lexical: true,
                catch_parameter: false,
            });
            child.bytecode.self_slot = Some(slot);
        }
        let mut vars = top_level_var_names(&function.body)?;
        let parameters: BTreeSet<_> = function
            .params
            .iter()
            .flat_map(|param| pattern_names(&param.pattern))
            .collect();
        let lexical = lexical_names(&function.body)?;
        if !child.bytecode.strict {
            vars.extend(
                annex_b_function_names(&function.body, &lexical)
                    .into_iter()
                    .filter(|name| !lexical.iter().any(|(lexical_name, _)| lexical_name == name)),
            );
        }
        if let Some((name, _)) = lexical.iter().find(|(name, _)| parameters.contains(name)) {
            return Err(CompileError::DuplicateBinding(name.clone()));
        }
        let simple_parameter_list = function.params.iter().all(|param| {
            !param.rest
                && param.default.is_none()
                && matches!(param.pattern, Pattern::Identifier(_))
        });
        // Non-simple formal parameters need the separate parameter/body
        // environment even when a destructuring pattern has no computed key
        // or default. The same distinction selects unmapped arguments.
        let parameter_expressions = !simple_parameter_list;
        child.bytecode.generator_initializes_parameters = parameter_expressions;
        // Arrow functions inherit `arguments`; ordinary functions introduce a
        // fresh binding unless a formal or a function-body lexical declaration
        // already occupies that name.  A `var arguments` declaration shares
        // this function binding rather than creating another one.
        let arguments_needed = !arrow
            && !parameters.contains("arguments")
            && !lexical.iter().any(|(name, _)| name == "arguments");
        if parameter_expressions {
            // Parameter expressions must not resolve into body declarations.
            // All parameter cells exist, uninitialized, before the first
            // initializer; closures keep those cells when the body later
            // creates a separate variable environment.
            let mut parameter_bindings: Vec<_> = parameters
                .iter()
                .map(|name| (name.clone(), DeclKind::Let))
                .collect();
            if arguments_needed {
                parameter_bindings.push(("arguments".into(), DeclKind::Let));
                // The arguments binding lives in the parameter environment.
                // A body `var arguments` is its redeclaration, not a second
                // binding in the body variable environment.
                vars.remove("arguments");
            }
            child.enter_scope(parameter_bindings, &BTreeSet::new(), true)?;
        } else {
            vars.extend(parameters.iter().cloned());
            if arguments_needed {
                vars.insert("arguments".into());
            }
            child.enter_scope(lexical.clone(), &vars, true)?;
        }
        if arguments_needed {
            let slot = child
                .resolve("arguments")
                .expect("function arguments binding was entered");
            child.bytecode.arguments_slot = Some(slot);
            if !child.bytecode.strict && simple_parameter_list {
                child.bytecode.arguments_mapped = true;
                let mut mapped_names = BTreeSet::new();
                let mut mapped_slots = vec![None; function.params.len()];
                for (index, parameter) in function.params.iter().enumerate().rev() {
                    let Pattern::Identifier(name) = &parameter.pattern else {
                        unreachable!("simple parameter list contains only identifiers")
                    };
                    if mapped_names.insert(name.clone()) {
                        mapped_slots[index] = child.resolve(name);
                    }
                }
                child.bytecode.arguments_mapped_slots = mapped_slots;
            }
            child.emit(Opcode::ArgumentsObject, 0)?;
        }
        for (index, param) in function.params.iter().enumerate() {
            child.emit(
                if param.rest {
                    Opcode::RestArguments
                } else {
                    Opcode::Argument
                },
                index as u32,
            )?;
            child.binding_pattern_default(param.default.as_ref(), &param.pattern)?;
            child.bind_pattern(&param.pattern, DeclKind::Let)?;
        }
        if parameter_expressions {
            let parameter_slots = child.names.last().unwrap().clone();
            child.local_scope = child.names.len();
            child.enter_scope(lexical, &vars, true)?;
            child.bytecode.variable_scope = child.scopes.last().copied().unwrap();
            // A redeclared var starts with the parameter's value. A function
            // declaration instead supplies its own value during hoisting.
            for name in vars.intersection(&parameters) {
                if function.body.iter().any(|statement| matches!(statement, Stmt::FunctionDecl(function) if function.name.as_ref() == Some(name))) {
                    continue;
                }
                child.emit(Opcode::GetBinding, parameter_slots[name])?;
                child.emit(
                    Opcode::InitializeBinding,
                    child.names[child.local_scope][name],
                )?;
            }
        }
        if child.bytecode.generator {
            child.bytecode.generator_entry = child.offset()?;
        }
        if options.default_derived_constructor {
            child.emit(Opcode::SuperCallForward, 0)?;
            child.emit(Opcode::Pop, 0)?;
        }
        child.statements(&function.body)?;
        child.constant(Value::Undefined)?;
        child.emit(Opcode::Return, 0)?;
        let child_bytes = child_budget - child.max_bytecode_bytes + child.offset()?;
        self.max_bytecode_bytes = self
            .max_bytecode_bytes
            .checked_sub(child_bytes)
            .ok_or(CompileError::ProgramTooLarge)?;
        let index = self.bytecode.functions.len() as u32;
        self.bytecode
            .functions
            .push(std::rc::Rc::new(child.bytecode));
        self.emit(Opcode::Closure, index)?;
        Ok(())
    }

    pub(super) fn self_tail_call_args<'a>(&self, value: &'a Expr) -> Option<&'a [Argument]> {
        let Expr::Call { callee, args } = value else {
            return None;
        };
        let Expr::Identifier(name) = callee.as_ref() else {
            return None;
        };
        let slot = self.bytecode.self_slot?;
        (self.bytecode.strict
            && self.resolve(name) == Some(slot)
            && args
                .iter()
                .all(|argument| matches!(argument, Argument::Normal(_))))
        .then_some(args)
    }
}
