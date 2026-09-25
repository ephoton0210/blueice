// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

use super::*;
use crate::bytecode::decoration as deco;

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
        // The class's own decorators are written before `class`, outside the
        // class: they are evaluated first, in the surrounding scope (the
        // class's name and private names are not visible to them) and in the
        // surrounding code's strictness, and wait on the operand stack for
        // `class_definition` to store them.
        if !class.decorators.is_empty() {
            self.decorator_list_on_stack(&class.decorators)?;
        }
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
        // The instructions that evaluate those parts inline run in this
        // function, so they need the strict runtime flag too; the handlers
        // restore the function's own strictness if one catches a throw.
        let result = if outer_strict {
            self.class_definition(class, inferred_name, binding)
        } else {
            self.emit(Opcode::SetStrictMode, 1)
                .and_then(|_| self.class_definition(class, inferred_name, binding))
                .and_then(|()| self.emit(Opcode::SetStrictMode, 0).map(|_| ()))
        };
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
        // A decorated element additionally keeps its evaluated decorators,
        // what its definition created, and the record its decorators produce.
        let mut decorations = BTreeMap::new();
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
            if let Some((kind, is_static)) = decorated_element(element) {
                // A decorated method or accessor half keeps its converted
                // computed key too: decorating it needs the key again.
                if matches!(
                    element,
                    ClassElement::Method {
                        key: PropertyKey::Computed(_),
                        ..
                    } | ClassElement::Accessor {
                        key: PropertyKey::Computed(_),
                        ..
                    }
                ) {
                    computed_key_bindings.insert(index, element_binding("key"));
                }
                decorations.insert(
                    index,
                    ElementDecoration {
                        decorators: element_binding("decorators"),
                        result: element_binding("decorated"),
                        original: element_binding("original"),
                        setter: element_binding("setter"),
                        kind,
                        is_static,
                    },
                );
            }
        }
        // The class itself: its decorators, and (whenever anything is
        // decorated) the metadata object every decorator's context shares.
        let has_class_decorators = !class.decorators.is_empty();
        let class_binding =
            |kind: &str| format!("{CLASS_ELEMENT_BINDING_PREFIX}{private_scope_id}_class_{kind}");
        let decorated_class = has_class_decorators || !decorations.is_empty();
        let class_decoration = decorated_class.then(|| ClassDecoration {
            decorators: class_binding("decorators"),
            metadata: class_binding("metadata"),
            extra_initializers: class_binding("extra_initializers"),
            decorated: class_binding("decorated"),
        });
        class_bindings.extend(
            computed_key_bindings
                .values()
                .chain(static_element_bindings.values())
                .map(|name| (name.clone(), DeclKind::Const)),
        );
        for decoration in decorations.values() {
            class_bindings.extend(
                [
                    &decoration.decorators,
                    &decoration.result,
                    &decoration.original,
                    &decoration.setter,
                ]
                .into_iter()
                .map(|name| (name.clone(), DeclKind::Const)),
            );
        }
        if let Some(class_decoration) = &class_decoration {
            class_bindings.extend(
                [
                    &class_decoration.decorators,
                    &class_decoration.metadata,
                    &class_decoration.extra_initializers,
                    &class_decoration.decorated,
                ]
                .into_iter()
                .map(|name| (name.clone(), DeclKind::Const)),
            );
        }
        let has_class_scope = !class_bindings.is_empty();
        if has_class_scope {
            self.enter_scope(class_bindings, &BTreeSet::new(), false)?;
        }
        // The class's own decorators were evaluated before the scope existed
        // (see `class_expression`) and are on the operand stack.
        if has_class_decorators {
            let class_decoration = class_decoration
                .as_ref()
                .expect("a class with decorators has a decoration plan");
            self.initialize_class_binding(&class_decoration.decorators)?;
        }
        if has_class_scope {
            self.private_scopes.push(private_scope.clone());
        }
        let constructor = class.elements.iter().find_map(|element| match element {
            ClassElement::Method {
                key,
                function,
                is_static: false,
                ..
            } if class_property_name(key).is_some_and(|name| name == "constructor") => {
                Some(function.clone())
            }
            _ => None,
        });
        let default_constructor = constructor.is_none();
        let mut constructor = constructor.unwrap_or_default();
        constructor.name = class.name.clone();
        // The constructor function object is the class: its source text is the
        // whole ClassDeclaration or ClassExpression, not the `constructor`
        // method (which a class without one does not even have).
        constructor.source_text = class.source_text.clone();
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
        // Every element is evaluated in order: its decorators, then its
        // computed key; methods and accessors are defined now, computed field
        // keys are converted now, and the functions that define each field or
        // run each static block are created now but only run afterwards.
        for (index, element) in class.elements.iter().enumerate() {
            let decoration = decorations.get(&index);
            if let Some(decoration) = decoration {
                self.decorator_list(element_decorators(element), &decoration.decorators)?;
            }
            match element {
                ClassElement::Method {
                    key,
                    function,
                    is_static,
                    ..
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
                        if let Some(binding_name) = computed_key_bindings.get(&index) {
                            self.emit(Opcode::Dup, 0)?;
                            let slot = self
                                .resolve(binding_name)
                                .expect("computed method key binding is in the class scope");
                            self.emit(Opcode::InitializeBinding, slot)?;
                        }
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
                        if let Some(decoration) = decoration {
                            self.emit(Opcode::Dup, 0)?;
                            self.initialize_class_binding(&decoration.original)?;
                        }
                        self.emit(Opcode::DefinePrivateMethod, u32::from(*is_static))?;
                    } else {
                        self.emit(Opcode::DefineMethod, 0)?;
                        match decoration {
                            Some(decoration) => {
                                self.initialize_class_binding(&decoration.original)?
                            }
                            None => {
                                self.emit(Opcode::Pop, 0)?;
                            }
                        }
                    }
                }
                ClassElement::Accessor {
                    key,
                    function,
                    getter,
                    is_static,
                    ..
                } => {
                    self.class_accessor_definition(
                        key,
                        None,
                        ElementCapture {
                            key: computed_key_bindings.get(&index),
                            function: decoration.map(|decoration| &decoration.original),
                        },
                        function,
                        *getter,
                        *is_static,
                    )?;
                }
                ClassElement::Field {
                    key,
                    initializer,
                    is_static,
                    accessor,
                    ..
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
                        self.initialize_class_binding(binding_name)?;
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
                                ElementCapture {
                                    key: None,
                                    function: decoration.map(|decoration| {
                                        if is_getter {
                                            &decoration.original
                                        } else {
                                            &decoration.setter
                                        }
                                    }),
                                },
                                &function,
                                is_getter,
                                *is_static,
                            )?;
                        }
                    }
                    if let Some(binding_name) = static_element_bindings.get(&index) {
                        let computed_binding = match field_key {
                            PropertyKey::Computed(_) => computed_key_bindings[&index].as_str(),
                            _ => "",
                        };
                        let field = class_field_definition(
                            field_key,
                            computed_binding,
                            initializer.as_ref(),
                        );
                        let field = match decoration {
                            Some(decoration) => Stmt::ClassDecoratedField {
                                field: Box::new(field),
                                record: decoration.result.clone(),
                            },
                            None => field,
                        };
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
        // Every element now exists. Apply the element decorators, each
        // element's own last to first: static methods and accessors first,
        // then instance ones, then static fields, then instance fields. Then
        // the class decorators see the finished class, and the metadata
        // object they all shared is published on the class they produced.
        if let Some(class_decoration) = &class_decoration {
            self.emit(Opcode::CreateMetadata, 0)?;
            self.initialize_class_binding(&class_decoration.metadata)?;
            for (fields, is_static) in [(false, true), (false, false), (true, true), (true, false)]
            {
                for (index, element) in class.elements.iter().enumerate() {
                    let Some(decoration) = decorations.get(&index) else {
                        continue;
                    };
                    if (decoration.kind == deco::FIELD) != fields
                        || decoration.is_static != is_static
                    {
                        continue;
                    }
                    self.decorate_class_element(
                        element,
                        decoration,
                        computed_key_bindings.get(&index),
                        &private_scope,
                        &class_decoration.metadata,
                    )?;
                }
            }
            if has_class_decorators {
                self.emit(Opcode::Dup, 0)?;
                self.get_class_binding(&class_decoration.decorators)?;
                self.constant(Value::String(
                    class.name.as_deref().or(inferred_name).unwrap_or("").into(),
                ))?;
                self.get_class_binding(&class_decoration.metadata)?;
                self.emit(Opcode::DecorateClass, 0)?;
                self.emit(Opcode::Dup, 0)?;
                self.constant(Value::String("1".into()))?;
                self.emit(Opcode::GetProperty, 0)?;
                self.initialize_class_binding(&class_decoration.decorated)?;
                self.constant(Value::String("0".into()))?;
                self.emit(Opcode::GetProperty, 0)?;
                self.initialize_class_binding(&class_decoration.extra_initializers)?;
                self.get_class_binding(&class_decoration.decorated)?;
            } else {
                self.emit(Opcode::Dup, 0)?;
            }
            self.get_class_binding(&class_decoration.metadata)?;
            self.emit(Opcode::DefineMetadata, 0)?;
        }
        // The inner name binding is initialized once every element has been
        // defined, so a computed key or method definition cannot observe the
        // class through it, while a static initializer can. It holds the class
        // the decorators finally produced.
        if let Some(slot) = binding {
            match class_decoration.as_ref().filter(|_| has_class_decorators) {
                Some(class_decoration) => {
                    self.get_class_binding(&class_decoration.decorated)?;
                }
                None => {
                    self.emit(Opcode::Dup, 0)?;
                }
            }
            self.emit(Opcode::InitializeBinding, slot)?;
        }
        // [[Fields]]: private methods and accessors first, then the extra
        // initializers of decorated methods, then every instance field in
        // order.
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
        for decoration in decorations.values() {
            if !decoration.is_static
                && matches!(decoration.kind, deco::METHOD | deco::GETTER | deco::SETTER)
            {
                instance_fields.push(Stmt::ClassExtraInitializers(decoration.result.clone()));
            }
        }
        for (index, element) in class.elements.iter().enumerate() {
            if let ClassElement::Field {
                key,
                initializer,
                is_static: false,
                accessor,
                ..
            } = element
            {
                let storage_key =
                    PropertyKey::Identifier(format!("#{}", auto_accessor_storage_name(index)));
                let field = if *accessor {
                    class_field_definition(&storage_key, "", initializer.as_ref())
                } else {
                    let computed_binding = match key {
                        PropertyKey::Computed(_) => computed_key_bindings[&index].as_str(),
                        _ => "",
                    };
                    class_field_definition(key, computed_binding, initializer.as_ref())
                };
                instance_fields.push(match decorations.get(&index) {
                    Some(decoration) => Stmt::ClassDecoratedField {
                        field: Box::new(field),
                        record: decoration.result.clone(),
                    },
                    None => field,
                });
            }
        }
        if !instance_fields.is_empty() {
            let initializer = Function {
                body: instance_fields,
                ..Function::default()
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
        // From here on the class is the one the decorators produced (`this` of
        // the static extra initializers, fields and blocks); its elements keep
        // the undecorated class as their home object.
        let final_class = class_decoration
            .as_ref()
            .filter(|_| has_class_decorators)
            .map(|class_decoration| &class_decoration.decorated);
        // Static methods' extra initializers run before any static field.
        for decoration in decorations.values() {
            if decoration.is_static
                && matches!(decoration.kind, deco::METHOD | deco::GETTER | deco::SETTER)
            {
                match final_class {
                    Some(decorated) => self.get_class_binding(decorated)?,
                    None => {
                        self.emit(Opcode::Dup, 0)?;
                    }
                }
                let slot = self
                    .resolve(&decoration.result)
                    .expect("decoration record binding is in the class scope");
                self.class_decoration_record_element(slot, 0)?;
                self.emit(Opcode::RunInitializers, 0)?;
            }
        }
        // Static fields and static blocks run last, in element order.
        for binding_name in static_element_bindings.values() {
            let slot = self
                .resolve(binding_name)
                .expect("static element binding is in the class scope");
            match final_class {
                Some(decorated) => {
                    self.get_class_binding(decorated)?;
                    self.emit(Opcode::GetBinding, slot)?;
                    self.emit(Opcode::CallDecoratedStaticElement, 0)?;
                }
                None => {
                    self.emit(Opcode::GetBinding, slot)?;
                    self.emit(Opcode::CallClassStaticBlock, 0)?;
                }
            }
        }
        // The class decorators' extra initializers see the finished class, and
        // a decorator that replaced the class is what the definition returns.
        if let Some(class_decoration) = class_decoration.as_ref().filter(|_| has_class_decorators) {
            self.get_class_binding(&class_decoration.decorated)?;
            self.emit(Opcode::Dup, 0)?;
            self.get_class_binding(&class_decoration.extra_initializers)?;
            self.emit(Opcode::RunInitializers, 0)?;
            self.emit(Opcode::Swap, 0)?;
            self.emit(Opcode::Pop, 0)?;
        }
        if has_class_scope {
            self.private_scopes.pop();
            self.leave_scope()?;
        }
        Ok(())
    }

    fn get_class_binding(&mut self, name: &str) -> Result<(), CompileError> {
        let slot = self
            .resolve(name)
            .expect("class element binding is in the class scope");
        self.emit(Opcode::GetBinding, slot)?;
        Ok(())
    }

    fn initialize_class_binding(&mut self, name: &str) -> Result<(), CompileError> {
        let slot = self
            .resolve(name)
            .expect("class element binding is in the class scope");
        self.emit(Opcode::InitializeBinding, slot)?;
        Ok(())
    }

    /// Evaluates `decorators` left to right into a decorator list (an Array of
    /// `decorator, receiver` pairs) on the operand stack. The decorator
    /// expressions run here, in source order with the surrounding class's other
    /// expressions; calling them is `DecorateElement`'s / `DecorateClass`'s
    /// job, later.
    fn decorator_list_on_stack(&mut self, decorators: &[Expr]) -> Result<(), CompileError> {
        self.emit(Opcode::NewArray, 0)?;
        for decorator in decorators {
            self.decorator_and_receiver(decorator)?;
            self.emit(Opcode::PushDecorator, 0)?;
        }
        Ok(())
    }

    /// Like `decorator_list_on_stack`, storing the list in the hidden binding.
    fn decorator_list(&mut self, decorators: &[Expr], binding: &str) -> Result<(), CompileError> {
        self.decorator_list_on_stack(decorators)?;
        self.initialize_class_binding(binding)
    }

    /// Pushes a decorator's value and the `this` it is called with: a
    /// property reference (`@a.b`, `@(a.b)`) calls with its base, exactly like
    /// the same expression as the callee of a call; everything else, a plain
    /// name, a call's result or any other parenthesized expression, with
    /// `undefined`.
    fn decorator_and_receiver(&mut self, decorator: &Expr) -> Result<(), CompileError> {
        match decorator {
            Expr::Member { .. } if !is_super_member(decorator) => {
                if let Some((object, name)) = private_member_parts(decorator) {
                    let owner = self.private_member_reference(object, name)?;
                    self.emit(Opcode::PrivateGetMethod, owner)?;
                } else {
                    self.member_reference(decorator)?;
                    self.emit(Opcode::GetMethod, 0)?;
                }
            }
            Expr::Parenthesized(inner)
                if matches!(
                    inner.as_ref(),
                    Expr::Member { .. } | Expr::OptionalMember { .. }
                ) && !is_super_member(inner) =>
            {
                self.parenthesized_optional_member_method(inner)?;
            }
            other => {
                self.expression(other)?;
                self.constant(Value::Undefined)?;
            }
        }
        Ok(())
    }

    /// Pushes an element's name as a decorator's context reports it: `#x` for
    /// a private name, otherwise the (already converted) property key.
    fn class_element_name(
        &mut self,
        key: &PropertyKey,
        key_binding: Option<&String>,
    ) -> Result<(), CompileError> {
        if let Some(name) = private_class_name(key) {
            self.constant(Value::String(format!("#{name}").into()))
        } else if let Some(binding_name) = key_binding {
            self.get_class_binding(binding_name)
        } else {
            self.property_key(key)
        }
    }

    /// Applies one element's decorators (`DecorateElement`) and puts the
    /// functions they returned back where the definition created the
    /// originals (`ReplaceClassElement`).
    fn decorate_class_element(
        &mut self,
        element: &ClassElement,
        decoration: &ElementDecoration,
        key_binding: Option<&String>,
        private_scope: &HashMap<String, String>,
        metadata: &str,
    ) -> Result<(), CompileError> {
        let (ClassElement::Method { key, .. }
        | ClassElement::Accessor { key, .. }
        | ClassElement::Field { key, .. }) = element
        else {
            return Err(CompileError::InvalidSyntax(
                "a static block cannot have decorators",
            ));
        };
        let private_name = private_class_name(key);
        let owner = private_name.map(|name| {
            private_scope
                .get(name)
                .expect("a private element has an owner binding")
                .clone()
        });
        let flags = decoration.kind
            | if decoration.is_static {
                deco::STATIC
            } else {
                0
            }
            | if private_name.is_some() {
                deco::PRIVATE
            } else {
                0
            };
        self.get_class_binding(&decoration.decorators)?;
        self.class_element_name(key, key_binding)?;
        match (&owner, private_name) {
            (Some(owner), Some(name)) => {
                self.get_class_binding(owner)?;
                self.constant(Value::String(name.into()))?;
            }
            _ => {
                self.constant(Value::Undefined)?;
                self.constant(Value::Undefined)?;
            }
        }
        match decoration.kind {
            deco::FIELD => {
                self.constant(Value::Undefined)?;
                self.constant(Value::Undefined)?;
            }
            deco::ACCESSOR => {
                self.get_class_binding(&decoration.original)?;
                self.get_class_binding(&decoration.setter)?;
            }
            _ => {
                self.get_class_binding(&decoration.original)?;
                self.constant(Value::Undefined)?;
            }
        }
        self.get_class_binding(metadata)?;
        self.emit(Opcode::DecorateElement, flags)?;
        self.initialize_class_binding(&decoration.result)?;
        let result_slot = self
            .resolve(&decoration.result)
            .expect("decoration record binding is in the class scope");
        // What to put back: (kind, binding of the original, index in the record).
        let replacements: &[(u32, &str, usize)] = match decoration.kind {
            deco::METHOD | deco::GETTER | deco::SETTER => {
                &[(decoration.kind, decoration.original.as_str(), 1)]
            }
            deco::ACCESSOR => &[
                (deco::GETTER, decoration.original.as_str(), 2),
                (deco::SETTER, decoration.setter.as_str(), 3),
            ],
            _ => &[],
        };
        for &(kind, original, position) in replacements {
            match (&owner, private_name) {
                (Some(owner), Some(name)) => {
                    self.get_class_binding(owner)?;
                    self.constant(Value::String(name.into()))?;
                }
                _ => {
                    self.class_property_target(decoration.is_static)?;
                    self.class_element_name(key, key_binding)?;
                }
            }
            self.get_class_binding(original)?;
            self.class_decoration_record_element(result_slot, position)?;
            self.emit(
                Opcode::ReplaceClassElement,
                kind | if private_name.is_some() {
                    deco::PRIVATE
                } else {
                    0
                },
            )?;
        }
        Ok(())
    }

    /// Defines one half of a class accessor pair on the class prototype (or on
    /// the constructor when static): a private accessor for a `#name`, else a
    /// public one. A getter or setter is named `get name` / `set name`. A
    /// computed key that was already converted (an auto-accessor's, shared by
    /// its getter and setter) is read back from its hidden binding instead of
    /// being evaluated again. `capture` says where a decorated element keeps
    /// the key it converts and the function it creates.
    fn class_accessor_definition(
        &mut self,
        key: &PropertyKey,
        key_binding: Option<&String>,
        capture: ElementCapture,
        function: &Function,
        getter: bool,
        is_static: bool,
    ) -> Result<(), CompileError> {
        self.class_property_target(is_static)?;
        if let Some(name) = private_class_name(key) {
            self.constant(Value::String(name.into()))?;
        } else if let Some(binding_name) = key_binding {
            self.get_class_binding(binding_name)?;
        } else {
            self.property_key(key)?;
            if let Some(binding_name) = capture.key {
                self.emit(Opcode::Dup, 0)?;
                self.initialize_class_binding(binding_name)?;
            }
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
            if let Some(binding_name) = capture.function {
                self.emit(Opcode::Dup, 0)?;
                self.initialize_class_binding(binding_name)?;
            }
            self.emit(
                Opcode::DefinePrivateAccessor,
                u32::from(!getter) | (u32::from(is_static) << 1),
            )?;
        } else {
            self.emit(Opcode::DefineClassAccessor, u32::from(!getter))?;
            match capture.function {
                Some(binding_name) => self.initialize_class_binding(binding_name)?,
                None => {
                    self.emit(Opcode::Pop, 0)?;
                }
            }
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
            body,
            ..Function::default()
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
        let child_budget = self.max_bytecode_bytes.saturating_sub(self.offset());
        let mut child = Compiler {
            bytecode: Bytecode::empty(),
            names: vec![HashMap::new()],
            private_scopes: self.private_scopes.clone(),
            next_private_scope: self.next_private_scope,
            scopes: Vec::new(),
            loops: Vec::new(),
            catch_var_slots: Vec::new(),
            max_bytecode_bytes: child_budget,
            max_metadata_entries: self.max_metadata_entries,
            max_list_items: self.max_list_items,
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
        // A direct eval in a sloppy function declares its `var`s in the
        // function's variable environment (§19.2.1.3), around the parameters
        // and the body (§10.2.11). It is entered like a `with` object at call
        // time, so every closure made in the parameters or the body captures
        // it and sees those vars whenever it runs, and a function defined
        // outside never does.
        let body_eval = crate::ast::body_contains_direct_eval(&function.body);
        let parameter_eval_scope = !child.bytecode.strict
            && !function.is_async
            && (body_eval || crate::ast::params_contain_direct_eval(&function.params));
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
        child.bytecode.source_text = function.source_text.clone();
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
            let index = child.metadata_index(child.bytecode.bindings.len())?;
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
            let slot = child.metadata_index(child.bytecode.bindings.len())?;
            child.names[0].insert(name.clone(), slot);
            child.bytecode.bindings.push(Binding {
                name,
                mutable: false,
                strict_immutable: false,
                lexical: true,
                catch_parameter: false,
                eval_var: false,
            });
            child.bytecode.self_slot = Some(slot);
        }
        if options.derived_constructor {
            // `super()` needs the active function; the frame initializes this
            // immutable binding to the callee, exactly like a named function
            // expression's own name.
            let slot = child.metadata_index(child.bytecode.bindings.len())?;
            child.names[0].insert(DERIVED_CONSTRUCTOR_BINDING.into(), slot);
            child.bytecode.bindings.push(Binding {
                name: DERIVED_CONSTRUCTOR_BINDING.into(),
                mutable: false,
                strict_immutable: true,
                lexical: true,
                catch_parameter: false,
                eval_var: false,
            });
            child.bytecode.self_slot = Some(slot);
        }
        let mut vars = top_level_var_names(&function.body);
        let parameters: BTreeSet<_> = function
            .params
            .iter()
            .flat_map(|param| pattern_names(&param.pattern))
            .collect();
        let lexical = lexical_names(&function.body);
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
        //
        // The object is also skipped when nothing in the function can observe
        // it (no `arguments` reference, no direct eval): building it costs
        // several property definitions and a heap cell per parameter on every
        // call, and no script can tell it was never made.
        let arguments_needed = !arrow
            && !parameters.contains("arguments")
            && (parameter_expressions || !lexical.iter().any(|(name, _)| name == "arguments"))
            && function_may_observe_arguments(function);
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
                // The arguments binding lives in the parameter environment. A
                // body `var arguments` is a second binding in the separate
                // body variable environment, initialized from this one below.
                parameter_bindings.push(("arguments".into(), DeclKind::Let));
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
                child.bytecode.arguments_mapped_slots =
                    child.mapped_argument_slots(&function.params);
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
            // A redeclared var starts with the value of the parameter (or of the
            // implicit `arguments` binding) it shares a name with. A function
            // declaration instead supplies its own value during hoisting.
            for name in vars.iter().filter(|name| {
                parameters.contains(*name) || (arguments_needed && *name == "arguments")
            }) {
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
        // The environment keeps receiving the vars of the body's own evals.
        if parameter_eval_scope && !body_eval {
            child.emit(Opcode::EndParameterEvalScope, 0)?;
        }
        if child.bytecode.generator {
            child.bytecode.generator_entry = child.offset();
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
        let child_offset = child.offset();
        self.finish_child_function(
            child.bytecode,
            child_budget,
            child.max_bytecode_bytes,
            child_offset,
        )
    }

    fn mapped_argument_slots(&self, params: &[Param]) -> Vec<Option<u32>> {
        let mut mapped_names = BTreeSet::new();
        let mut mapped_slots = vec![None; params.len()];
        for (index, parameter) in params.iter().enumerate().rev() {
            if let Pattern::Identifier(name) = &parameter.pattern {
                if mapped_names.insert(name.clone()) {
                    mapped_slots[index] = self.resolve(name);
                }
            }
        }
        mapped_slots
    }

    fn finish_child_function(
        &mut self,
        child: Bytecode,
        child_budget: u32,
        child_remaining: u32,
        child_offset: u32,
    ) -> Result<(), CompileError> {
        let child_bytes = child_budget
            .checked_sub(child_remaining)
            .and_then(|spent| spent.checked_add(child_offset))
            .ok_or(CompileError::ProgramTooLarge)?;
        self.max_bytecode_bytes = self
            .max_bytecode_bytes
            .checked_sub(child_bytes)
            .ok_or(CompileError::ProgramTooLarge)?;
        let index = self.bytecode.functions.len() as u32;
        self.bytecode.functions.push(std::rc::Rc::new(child));
        self.emit(Opcode::Closure, index)?;
        Ok(())
    }

    pub(super) fn self_tail_call_args<'a>(&self, value: &'a Expr) -> Option<Vec<&'a Expr>> {
        let Expr::Call { callee, args } = value else {
            return None;
        };
        let Expr::Identifier(name) = callee.as_ref() else {
            return None;
        };
        let slot = self.bytecode.self_slot?;
        if !self.bytecode.strict
            // A recursive call made by a generator or async function creates
            // a distinct generator/promise execution. Reusing this frame
            // would eagerly run it and changes the observable result.
            || self.bytecode.generator
            || self.bytecode.async_function
            || self.resolve(name) != Some(slot)
        {
            return None;
        }
        args.iter()
            .map(|argument| match argument {
                Argument::Normal(value) => Some(value),
                Argument::Spread(_) => None,
            })
            .collect()
    }
}

/// The hidden class-scope bindings one decorated class element uses (see
/// `class_definition`).
struct ElementDecoration {
    /// The element's evaluated decorators, in an Array.
    decorators: String,
    /// The record `DecorateElement` returns (`[extraInitializers, ...]`).
    result: String,
    /// What the class definition created: the function of a method or accessor
    /// half, or an auto-accessor's getter.
    original: String,
    /// An auto-accessor's setter.
    setter: String,
    /// The `deco::*` element kind.
    kind: u32,
    is_static: bool,
}

/// The hidden class-scope bindings a class with decorators uses.
struct ClassDecoration {
    /// The class's own decorators (in an Array).
    decorators: String,
    /// The metadata object every decorator's context shares.
    metadata: String,
    /// The extra initializers the class decorators added.
    extra_initializers: String,
    /// The class the class decorators produced.
    decorated: String,
}

/// Where a decorated class element keeps what its definition creates.
#[derive(Clone, Copy, Default)]
struct ElementCapture<'a> {
    /// The converted computed key of a method or accessor half.
    key: Option<&'a String>,
    /// The function the definition created.
    function: Option<&'a String>,
}

/// A decorated element's `deco::*` kind and whether it is static.
fn decorated_element(element: &ClassElement) -> Option<(u32, bool)> {
    match element {
        ClassElement::Method {
            decorators,
            is_static,
            ..
        } if !decorators.is_empty() => Some((deco::METHOD, *is_static)),
        ClassElement::Accessor {
            decorators,
            getter,
            is_static,
            ..
        } if !decorators.is_empty() => Some((
            if *getter { deco::GETTER } else { deco::SETTER },
            *is_static,
        )),
        ClassElement::Field {
            decorators,
            accessor,
            is_static,
            ..
        } if !decorators.is_empty() => Some((
            if *accessor {
                deco::ACCESSOR
            } else {
                deco::FIELD
            },
            *is_static,
        )),
        _ => None,
    }
}

fn element_decorators(element: &ClassElement) -> &[Expr] {
    match element {
        ClassElement::Method { decorators, .. }
        | ClassElement::Accessor { decorators, .. }
        | ClassElement::Field { decorators, .. } => decorators,
        ClassElement::StaticBlock(_) => &[],
    }
}

#[cfg(test)]
mod tests;
