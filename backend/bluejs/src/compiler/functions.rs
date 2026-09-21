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
        // Every part of a class, its heritage and computed keys included, is
        // strict mode code: a function written there is strict even when the
        // class sits in sloppy code.
        let outer_strict = std::mem::replace(&mut self.bytecode.strict, true);
        let result = self.class_definition(class, inferred_name, binding);
        self.bytecode.strict = outer_strict;
        result
    }

    fn class_definition(
        &mut self,
        class: &Class,
        inferred_name: Option<&str>,
        binding: Option<u32>,
    ) -> Result<(), CompileError> {
        let private_declarations = class_private_declarations(class)?;
        let private_scope_id = self.next_private_scope;
        self.next_private_scope = self.next_private_scope.saturating_add(1);
        let mut private_scope = HashMap::new();
        let mut class_bindings = Vec::new();
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
                    accessor: false,
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
                || !class_bindings
                    .iter()
                    .any(|(existing, _)| existing == &binding)
            {
                class_bindings.push((binding, DeclKind::Const));
            }
        }
        // A computed field key is evaluated once, when the class is defined,
        // and a static field or block runs only after every element has been
        // defined. Both outlive their place in the element list, so they live
        // in hidden bindings of the class scope that the functions running
        // them capture.
        let mut computed_key_bindings = BTreeMap::new();
        let mut static_element_bindings = BTreeMap::new();
        for (index, element) in class.elements.iter().enumerate() {
            let element_binding = |kind: &str| {
                format!("{CLASS_ELEMENT_BINDING_PREFIX}{private_scope_id}_{kind}_{index}")
            };
            match element {
                ClassElement::Field { key, is_static, .. } => {
                    if matches!(key, PropertyKey::Computed(_)) {
                        computed_key_bindings.insert(index, element_binding("key"));
                    }
                    if *is_static {
                        static_element_bindings.insert(index, element_binding("static"));
                    }
                }
                ClassElement::StaticBlock(_) => {
                    static_element_bindings.insert(index, element_binding("static"));
                }
                _ => {}
            }
        }
        class_bindings.extend(
            computed_key_bindings
                .values()
                .chain(static_element_bindings.values())
                .map(|name| (name.clone(), DeclKind::Const)),
        );
        let has_class_scope = !class_bindings.is_empty();
        if has_class_scope {
            self.enter_scope(class_bindings, &BTreeSet::new(), false)?;
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
                class_field_initializer: false,
            },
        )?;
        self.emit(Opcode::SetClassHome, 0)?;
        if let Some(base) = &class.extends {
            self.expression(base)?;
            self.emit(Opcode::SetClassHeritage, 0)?;
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
        // Every element is evaluated in order: methods and accessors are
        // defined now, computed field keys are converted now, and the
        // functions that define each field or run each static block are
        // created now but only run afterwards.
        for (index, element) in class.elements.iter().enumerate() {
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
                    if matches!(key, PropertyKey::Computed(_)) {
                        self.emit(Opcode::SetFunctionName, 0)?;
                    }
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
                    self.class_accessor_definition(key, None, function, *getter, *is_static)?;
                }
                ClassElement::Field {
                    key,
                    initializer,
                    is_static,
                    accessor,
                } => {
                    // What gets initialized per instance (or once for a
                    // static): the field itself, or an auto-accessor's hidden
                    // private storage field.
                    let storage_key = accessor.then(|| {
                        PropertyKey::Identifier(format!("#{}", auto_accessor_storage_name(index)))
                    });
                    let field_key = storage_key.as_ref().unwrap_or(key);
                    if let Some(name) = private_class_name(field_key) {
                        // Declare the private name on its owner now; the
                        // field itself is added when it is initialized.
                        self.class_property_target(*is_static)?;
                        self.constant(Value::String(name.into()))?;
                        self.emit(Opcode::DefinePrivateField, u32::from(*is_static))?;
                    }
                    if let Some(binding_name) = computed_key_bindings.get(&index) {
                        self.property_key(key)?;
                        let slot = self
                            .resolve(binding_name)
                            .expect("computed field key binding is in the class scope");
                        self.emit(Opcode::InitializeBinding, slot)?;
                    }
                    if *accessor {
                        let (getter, setter) = auto_accessor_functions(
                            index,
                            literal_property_key_name(key).as_deref(),
                        );
                        for (function, is_getter) in [(getter, true), (setter, false)] {
                            self.class_accessor_definition(
                                key,
                                computed_key_bindings.get(&index),
                                &function,
                                is_getter,
                                *is_static,
                            )?;
                        }
                    }
                    if let Some(binding_name) = static_element_bindings.get(&index) {
                        let field = class_field_definition(
                            field_key,
                            computed_key_bindings.get(&index).filter(|_| !*accessor),
                            initializer.as_ref(),
                        );
                        self.class_element_function(vec![field], binding_name, true)?;
                    }
                }
                ClassElement::StaticBlock(body) => {
                    let binding_name = static_element_bindings
                        .get(&index)
                        .expect("static block has a function binding");
                    self.class_element_function(body.clone(), binding_name, false)?;
                }
            }
        }
        // The inner name binding is initialized once every element has been
        // defined, so a computed key or method definition cannot observe the
        // class through it, while a static initializer can.
        if let Some(slot) = binding {
            self.emit(Opcode::Dup, 0)?;
            self.emit(Opcode::InitializeBinding, slot)?;
        }
        // [[Fields]]: private methods and accessors first, then every
        // instance field in order.
        let mut instance_fields = Vec::new();
        let instance_private_method = class.elements.iter().find_map(|element| match element {
            ClassElement::Method {
                key,
                is_static: false,
                ..
            }
            | ClassElement::Accessor {
                key,
                is_static: false,
                ..
            }
            | ClassElement::Field {
                key,
                is_static: false,
                accessor: true,
                ..
            } => private_class_name(key),
            _ => None,
        });
        if let Some(name) = instance_private_method {
            instance_fields.push(Stmt::ClassPrivateBrand(
                private_scope
                    .get(name)
                    .expect("private instance declaration has an owner binding")
                    .clone(),
            ));
        }
        for (index, element) in class.elements.iter().enumerate() {
            if let ClassElement::Field {
                key,
                initializer,
                is_static: false,
                accessor,
            } = element
            {
                let storage_key =
                    PropertyKey::Identifier(format!("#{}", auto_accessor_storage_name(index)));
                instance_fields.push(if *accessor {
                    class_field_definition(&storage_key, None, initializer.as_ref())
                } else {
                    class_field_definition(
                        key,
                        computed_key_bindings.get(&index),
                        initializer.as_ref(),
                    )
                });
            }
        }
        if !instance_fields.is_empty() {
            let initializer = Function {
                name: None,
                params: Vec::new(),
                body: instance_fields,
                generator: false,
                is_async: false,
            };
            self.function_named_with(
                &initializer,
                false,
                None,
                false,
                FunctionCompileOptions::class_field_initializer(),
            )?;
            self.emit(Opcode::SetClassFields, 0)?;
        }
        // Static fields and static blocks run last, in element order.
        for binding_name in static_element_bindings.values() {
            let slot = self
                .resolve(binding_name)
                .expect("static element binding is in the class scope");
            self.emit(Opcode::GetBinding, slot)?;
            self.emit(Opcode::CallClassStaticBlock, 0)?;
        }
        if has_class_scope {
            self.private_scopes.pop();
            self.leave_scope()?;
        }
        Ok(())
    }

    /// Defines one half of a class accessor pair on the class prototype (or on
    /// the constructor when static): a private accessor for a `#name`, else a
    /// public one. A getter or setter is named `get name` / `set name`. A
    /// computed key that was already converted (an auto-accessor's, shared by
    /// its getter and setter) is read back from its hidden binding instead of
    /// being evaluated again.
    fn class_accessor_definition(
        &mut self,
        key: &PropertyKey,
        key_binding: Option<&String>,
        function: &Function,
        getter: bool,
        is_static: bool,
    ) -> Result<(), CompileError> {
        self.class_property_target(is_static)?;
        if let Some(name) = private_class_name(key) {
            self.constant(Value::String(name.into()))?;
        } else if let Some(binding_name) = key_binding {
            let slot = self
                .resolve(binding_name)
                .expect("computed key binding is in the class scope");
            self.emit(Opcode::GetBinding, slot)?;
        } else {
            self.property_key(key)?;
        }
        let mut function = function.clone();
        if let Some(name) = literal_property_key_name(key) {
            function.name = Some(format!("{} {name}", if getter { "get" } else { "set" }));
        }
        self.function_named_with(
            &function,
            false,
            None,
            false,
            FunctionCompileOptions::class_method(),
        )?;
        if matches!(key, PropertyKey::Computed(_)) {
            self.emit(Opcode::SetFunctionName, if getter { 1 } else { 2 })?;
        }
        if private_class_name(key).is_some() {
            self.emit(
                Opcode::DefinePrivateAccessor,
                u32::from(!getter) | (u32::from(is_static) << 1),
            )?;
        } else {
            self.emit(Opcode::DefineClassAccessor, u32::from(!getter))?;
            self.emit(Opcode::Pop, 0)?;
        }
        Ok(())
    }

    /// Compiles the function that runs one static field definition or static
    /// block (with the class constructor as `this`) and stores it in the
    /// hidden class-scope binding `binding_name`.
    fn class_element_function(
        &mut self,
        body: Vec<Stmt>,
        binding_name: &str,
        field: bool,
    ) -> Result<(), CompileError> {
        let function = Function {
            name: None,
            params: Vec::new(),
            body,
            generator: false,
            is_async: false,
        };
        self.function_named_with(
            &function,
            false,
            None,
            false,
            if field {
                FunctionCompileOptions::class_field_initializer()
            } else {
                FunctionCompileOptions::class_method()
            },
        )?;
        let slot = self
            .resolve(binding_name)
            .expect("class element binding is in the class scope");
        self.emit(Opcode::InitializeBinding, slot)?;
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
                class_field_initializer: false,
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
            // A function created inside `with` resolves its free names
            // through the same with objects (captured when it is created);
            // its own parameters and locals sit inside that with scope.
            with_depth: self.with_depth,
            with_scope_depths: vec![1; self.with_depth],
            annex_b_parameter_names: BTreeSet::new(),
            tail_call_blockers: 0,
            tail_call_pending: false,
        };
        child.bytecode.with_depth = self.with_depth as u32;
        child.bytecode.strict =
            options.force_strict || self.bytecode.strict || strict_body(&function.body);
        // A direct eval in the parameter list of a sloppy function declares
        // its `var`s in an environment that sits outside the parameters and
        // around the whole function (§10.2.11). It is entered like a `with`
        // object at call time, so every closure made in the parameters or the
        // body captures it and sees those vars whenever it runs.
        let parameter_eval_scope = !child.bytecode.strict
            && !function.generator
            && !function.is_async
            && !function.params.iter().all(|param| {
                !param.rest
                    && param.default.is_none()
                    && matches!(param.pattern, Pattern::Identifier(_))
            })
            && crate::ast::params_contain_direct_eval(&function.params);
        if parameter_eval_scope {
            child.with_depth += 1;
            child.with_scope_depths = vec![1; child.with_depth];
            child.bytecode.parameter_eval_scope = true;
        }
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
        // An arrow function shares its creator's (lack of an) `arguments`
        // binding, so it stays a field initializer for direct-eval purposes.
        child.bytecode.class_field_initializer =
            options.class_field_initializer || (arrow && self.bytecode.class_field_initializer);
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
            // Only an arrow function shares its creator's `this` and derived
            // constructor; any other function has its own receiver.
            if !arrow && (name == DERIVED_THIS_BINDING || name == DERIVED_CONSTRUCTOR_BINDING) {
                continue;
            }
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
        if options.derived_constructor {
            // `super()` needs the active function; the frame initializes this
            // immutable binding to the callee, exactly like a named function
            // expression's own name.
            let slot = u32::try_from(child.bytecode.bindings.len())
                .map_err(|_| CompileError::ProgramTooLarge)?;
            child.names[0].insert(DERIVED_CONSTRUCTOR_BINDING.into(), slot);
            child.bytecode.bindings.push(Binding {
                name: DERIVED_CONSTRUCTOR_BINDING.into(),
                mutable: false,
                strict_immutable: true,
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
            // Annex B.3.2.1 exempts `parameterNames`. That list holds only the
            // formal parameters: the implicit `arguments` binding is added to
            // the separate `parameterBindings`, so a block function named
            // `arguments` is still hoisted over the arguments object.
            child.annex_b_parameter_names = parameters.clone();
            vars.extend(
                annex_b_function_names(&function.body, &lexical)
                    .into_iter()
                    .filter(|name| !lexical.iter().any(|(lexical_name, _)| lexical_name == name))
                    .filter(|name| !child.annex_b_parameter_names.contains(name)),
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
        //
        // A body lexical `arguments` suppresses the object only when there are
        // no parameter expressions (FunctionDeclarationInstantiation step 22):
        // otherwise the parameter initializers still see it and the body's own
        // declaration shadows it afterwards.
        let arguments_needed = !arrow
            && !parameters.contains("arguments")
            && (parameter_expressions || !lexical.iter().any(|(name, _)| name == "arguments"));
        if parameter_expressions {
            // Parameter expressions must not resolve into body declarations.
            // All parameter cells exist, uninitialized, before the first
            // initializer; closures keep those cells when the body later
            // creates a separate variable environment.
            let mut parameter_bindings: Vec<_> = parameters
                .iter()
                .map(|name| (name.clone(), DeclKind::Let))
                .collect();
            if options.derived_constructor {
                parameter_bindings.push((DERIVED_THIS_BINDING.into(), DeclKind::Let));
            }
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
            let mut first_scope = lexical.clone();
            if options.derived_constructor {
                first_scope.push((DERIVED_THIS_BINDING.into(), DeclKind::Let));
            }
            child.enter_scope(first_scope, &vars, true)?;
        }
        if options.derived_constructor {
            child.bytecode.derived_this_slot = child.resolve(DERIVED_THIS_BINDING);
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
        if parameter_eval_scope {
            child.emit(Opcode::EndParameterEvalScope, 0)?;
        }
        if child.bytecode.generator {
            child.bytecode.generator_entry = child.offset()?;
        }
        if options.default_derived_constructor {
            child.super_call_prologue()?;
            child.emit(Opcode::SuperCallForward, 0)?;
            child.super_call_epilogue()?;
            child.emit(Opcode::Pop, 0)?;
        }
        child.statements_with_disposal(&function.body)?;
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
            // A recursive call made by a generator or async function creates
            // a distinct generator/promise execution. Reusing this frame
            // would eagerly run it and changes the observable result.
            && !self.bytecode.generator
            && !self.bytecode.async_function
            && self.resolve(name) == Some(slot)
            && args
                .iter()
                .all(|argument| matches!(argument, Argument::Normal(_))))
        .then_some(args)
    }
}
