// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

//! Bounded class constructor and method checks before runtime admission.

use super::functions::hoist_local_functions;
use super::*;
use crate::parser::{ClassConstructor, ClassDeclaration, ClassMemberKind, ClassMethod, Visibility};

mod accessors;
mod bodies;
mod types;

pub(in crate::checker::module) use types::class_constructor_side_type;
pub(in crate::checker::module) use types::class_is_fully_structured;
use types::*;
mod fields;
mod overrides;
mod super_calls;
mod visibility;

fn specialize_class_rest_annotation(
    annotation: &Type,
    types: &BTreeMap<String, TypeDefinition>,
    budget: &mut TypeExpansionBudget,
) -> Result<Type, TupleSpreadError> {
    let named = matches!(annotation, Type::Named { .. });
    let mut resolved = annotation.clone();
    let mut visited = HashSet::new();
    while matches!(resolved, Type::Named { .. }) {
        resolved = instantiate_named(&resolved, types, &mut visited, budget, "class rest").ok_or(
            if budget.exhausted {
                TupleSpreadError::Exhausted
            } else {
                TupleSpreadError::Unresolved
            },
        )?;
    }
    match resolved {
        Type::Tuple(elements)
            if named
                || elements.iter().any(|element| {
                    element.rest && !matches!(element.annotation, Type::Array(_))
                }) =>
        {
            Ok(Type::Tuple(expand_concrete_tuple_spreads(
                &elements,
                types,
                &mut HashSet::new(),
                budget,
            )?))
        }
        Type::Tuple(_) | Type::Array(_) => Ok(resolved),
        _ => Err(TupleSpreadError::Unsupported),
    }
}

impl ModuleChecker<'_> {
    pub(in crate::checker::module) fn check_type_only_class_value_uses(
        &mut self,
        tokens: &[Token],
        scope: &BTreeMap<String, Type>,
        span: &SourceSpan,
    ) {
        let mut inspected = 0usize;
        for (index, token) in tokens.iter().enumerate() {
            if token.kind != TokenKind::Identifier
                || !self.type_only_classes.contains(&token.text)
                || scope.contains_key(&token.text)
            {
                continue;
            }
            let previous = index.checked_sub(1).and_then(|index| tokens.get(index));
            let next = tokens.get(index + 1);
            let runtime_use = tokens.len() == 1
                || previous.is_some_and(|previous| previous.is("new"))
                || next.is_some_and(|next| next.is("(") || next.is("."))
                || previous.is_some_and(|previous| {
                    matches!(
                        previous.text.as_str(),
                        "(" | "[" | "{" | "," | "=" | "return" | "throw"
                    )
                }) && !next.is_some_and(|next| next.is(">") || next.is(":"));
            if !runtime_use {
                continue;
            }
            inspected += 1;
            if inspected > self.max_type_expansions {
                self.type_error(
                    span,
                    format!(
                        "type-only class value scan exceeds the {} generic-expansion limit",
                        self.max_type_expansions
                    ),
                    DiagnosticCode::ResourceLimit,
                );
                return;
            }
            self.type_error(
                &SourceSpan::new(&span.module, token.start, token.end),
                format!("type-only class {} cannot be used as a value", token.text),
                DiagnosticCode::UnknownName,
            );
        }
    }

    pub(super) fn bind_imported_class(
        &mut self,
        local_name: &str,
        class: &ExportedClass,
        span: &SourceSpan,
    ) {
        if self.types.contains_key(local_name) || self.values.contains_key(local_name) {
            self.duplicate(local_name, span.clone());
            return;
        }
        self.types.insert(
            local_name.to_string(),
            TypeDefinition {
                kind: TypeDefinitionKind::Class,
                parameters: Vec::new(),
                value: specialize_imported_class_type(
                    &class.instance_type,
                    &class.source_name,
                    local_name,
                ),
            },
        );
        self.insert_value(
            local_name,
            specialize_imported_class_type(&class.constructor_type, &class.source_name, local_name),
            span.clone(),
            SymbolKind::Import,
            false,
        );
        let signatures = class
            .constructor_binding
            .signatures
            .iter()
            .map(|signature| {
                let mut signature = signature.clone();
                for parameter in &mut signature.parameters {
                    parameter.annotation = parameter.annotation.as_ref().map(|annotation| {
                        specialize_imported_class_type(annotation, &class.source_name, local_name)
                    });
                }
                signature.return_type = specialize_imported_class_type(
                    &signature.return_type,
                    &class.source_name,
                    local_name,
                );
                signature
            })
            .collect();
        self.class_constructors.insert(
            local_name.to_string(),
            ClassConstructorBinding {
                signatures,
                inherited: class.constructor_binding.inherited,
                visibility: class.constructor_binding.visibility,
            },
        );
    }

    pub(in crate::checker::module) fn is_bound_class_constructor_value(
        &self,
        name: &str,
        scope: &BTreeMap<String, Type>,
    ) -> bool {
        self.class_constructors.contains_key(name)
            && scope
                .get(name)
                .is_some_and(|value| self.values.get(name) == Some(value))
    }

    pub(in crate::checker::module) fn is_bound_class_instance_type(&self, value: &Type) -> bool {
        matches!(value, Type::Named { name, .. } if self.types.get(name).is_some_and(|definition| definition.kind == TypeDefinitionKind::Class))
    }

    pub(in crate::checker::module) fn is_bound_class_static_this(
        &self,
        receiver: &Token,
        scope: &BTreeMap<String, Type>,
    ) -> bool {
        receiver.is("this") && matches!(scope.get("this"), Some(Type::Record(_)))
    }

    pub(super) fn bind_class(&mut self, class: &ClassDeclaration) {
        let existing_type = self
            .types
            .get(&class.name)
            .map(|definition| definition.kind);
        if self.values.contains_key(&class.name)
            || matches!(
                existing_type,
                Some(TypeDefinitionKind::Alias | TypeDefinitionKind::Class)
            )
        {
            self.duplicate(&class.name, class.name_span.clone());
            return;
        }

        // TypeScript permits an interface with the same name as a class.
        // Retain the prior interface surface until declaration merging is
        // checked later; class output is refused throughout this phase.
        if existing_type.is_none() {
            self.types.insert(
                class.name.clone(),
                TypeDefinition {
                    kind: TypeDefinitionKind::Class,
                    parameters: Vec::new(),
                    value: class_instance_type(class),
                },
            );
        }
        self.values
            .insert(class.name.clone(), class_constructor_side_type(class));
        self.class_constructors.insert(
            class.name.clone(),
            ClassConstructorBinding {
                signatures: class_constructor_signatures(class),
                inherited: class.extends_name.is_some() && !class_declares_constructor(class),
                visibility: class_constructor_visibility(class),
            },
        );
    }

    pub(super) fn validate_class_heritage_name(&mut self, class: &ClassDeclaration) {
        let (Some(base_name), Some(span)) = (&class.extends_name, &class.extends_span) else {
            return;
        };
        // Cycle diagnostics are checked separately, including the direct
        // self-reference. Do not report it as an unknown or forward base.
        if base_name == &class.name {
            return;
        }
        if self.class_constructors.contains_key(base_name) {
            let declared_later = self.module.declarations.iter().any(|declaration| {
                matches!(declaration, Declaration::Class(base)
                    if &base.name == base_name && base.name_span.start > class.name_span.start)
            });
            if declared_later {
                self.type_error(
                    span,
                    format!("TS2449: class {base_name} is used before its declaration"),
                    DiagnosticCode::UsedBeforeDeclaration,
                );
            }
            return;
        }
        match self.values.get(base_name) {
            None => {
                let message = format!("unknown class heritage name {base_name}");
                if let Some((namespace, member)) = base_name
                    .rsplit_once('.')
                    .filter(|(namespace, _)| self.values.contains_key(*namespace))
                {
                    self.typescript_type_error(
                        span,
                        message,
                        DiagnosticCode::UnknownName,
                        2339,
                        vec![member.into(), format!("typeof {namespace}")],
                    );
                } else {
                    self.typescript_type_error(
                        span,
                        message,
                        DiagnosticCode::UnknownName,
                        2304,
                        vec![base_name.clone()],
                    );
                }
            }
            Some(Type::Any | Type::Unknown) => {}
            Some(_) if self.functions.contains_key(base_name) => {}
            Some(_) => self.type_error(
                span,
                format!("class heritage {base_name} is not a constructor"),
                DiagnosticCode::TypeMismatch,
            ),
        }
    }

    pub(super) fn validate_class_heritage_cycles(&mut self) {
        let classes = self
            .module
            .declarations
            .iter()
            .filter_map(|declaration| match declaration {
                Declaration::Class(class) => Some((class.name.as_str(), class)),
                _ => None,
            })
            .collect::<BTreeMap<_, _>>();
        let mut failures = Vec::new();
        for class in classes.values() {
            let mut current = *class;
            let mut visited = BTreeSet::new();
            let mut edges = 0usize;
            while let Some(base_name) = current.extends_name.as_deref() {
                if edges >= self.max_type_expansions {
                    failures.push((
                        class.name_span.clone(),
                        DiagnosticCode::ResourceLimit,
                        format!(
                            "class heritage scan exceeds the {} generic-expansion limit",
                            self.max_type_expansions
                        ),
                    ));
                    break;
                }
                edges += 1;
                if base_name == class.name {
                    failures.push((
                        class.name_span.clone(),
                        DiagnosticCode::TypeMismatch,
                        format!("class {} has a cyclic base expression", class.name),
                    ));
                    break;
                }
                if !visited.insert(base_name) {
                    break;
                }
                let Some(base) = classes.get(base_name) else {
                    break;
                };
                current = base;
            }
        }
        for (span, code, message) in failures {
            self.type_error(&span, message, code);
        }
    }

    pub(super) fn bind_inherited_class_instance_methods(&mut self) {
        let classes = self
            .module
            .declarations
            .iter()
            .filter_map(|declaration| match declaration {
                Declaration::Class(class) => Some(class.clone()),
                _ => None,
            })
            .collect::<Vec<_>>();
        let local_classes = classes
            .iter()
            .map(|class| (class.name.clone(), self.class_with_inferred_returns(class)))
            .collect::<BTreeMap<_, _>>();
        let mut surfaces = Vec::new();
        for class in local_classes.values() {
            if !self
                .types
                .get(&class.name)
                .is_some_and(|definition| definition.kind == TypeDefinitionKind::Class)
            {
                continue;
            }
            let mut fields = class_method_fields(class, false);
            let mut names = fields
                .iter()
                .map(|field| field.name.clone())
                .collect::<BTreeSet<_>>();
            let mut visited = BTreeSet::new();
            let mut base_name = class.extends_name.as_deref();
            for _ in 0..self.max_type_expansions {
                let Some(name) = base_name else {
                    break;
                };
                if !visited.insert(name) {
                    break;
                }
                let inherited = if let Some(base) = local_classes.get(name) {
                    base_name = base.extends_name.as_deref();
                    class_method_fields(base, false)
                } else if let Some(TypeDefinition {
                    kind: TypeDefinitionKind::Class,
                    value: Type::Record(fields),
                    ..
                }) = self.types.get(name)
                {
                    base_name = None;
                    fields.clone()
                } else {
                    break;
                };
                fields.extend(
                    inherited
                        .iter()
                        .filter(|field| !names.contains(&field.name))
                        .cloned(),
                );
                names.extend(inherited.iter().map(|field| field.name.clone()));
            }
            surfaces.push((class.name.clone(), Type::Record(fields)));
        }
        for (name, value) in surfaces {
            if let Some(definition) = self.types.get_mut(&name) {
                definition.value = value;
            }
        }
    }

    pub(super) fn bind_inherited_class_static_methods(&mut self) {
        let classes = self
            .module
            .declarations
            .iter()
            .filter_map(|declaration| match declaration {
                Declaration::Class(class) => Some(class.clone()),
                _ => None,
            })
            .collect::<Vec<_>>();
        let local_classes = classes
            .iter()
            .map(|class| (class.name.clone(), self.class_with_inferred_returns(class)))
            .collect::<BTreeMap<_, _>>();
        let mut surfaces = Vec::new();
        for class in local_classes.values() {
            if !self.class_constructors.contains_key(&class.name) {
                continue;
            }
            let Type::Record(mut fields) = class_constructor_side_type(class) else {
                unreachable!("class constructor side is a record")
            };
            let mut names = fields
                .iter()
                .map(|field| field.name.clone())
                .collect::<BTreeSet<_>>();
            let mut visited = BTreeSet::new();
            let mut base_name = class.extends_name.as_deref();
            for _ in 0..self.max_type_expansions {
                let Some(name) = base_name else {
                    break;
                };
                if !visited.insert(name) {
                    break;
                }
                let inherited = if let Some(base) = local_classes.get(name) {
                    base_name = base.extends_name.as_deref();
                    class_constructor_side_type(base)
                } else if self.class_constructors.contains_key(name) {
                    base_name = None;
                    self.values.get(name).cloned().unwrap_or(Type::Unknown)
                } else {
                    break;
                };
                let Type::Record(inherited) = inherited else {
                    break;
                };
                fields.extend(
                    inherited
                        .iter()
                        .filter(|field| !names.contains(&field.name))
                        .cloned(),
                );
                names.extend(inherited.iter().map(|field| field.name.clone()));
            }
            surfaces.push((class.name.clone(), Type::Record(fields)));
        }
        for (name, value) in surfaces {
            self.values.insert(name, value);
        }
    }

    pub(super) fn bind_inherited_class_constructors(&mut self) {
        for declaration in &self.module.declarations {
            let Declaration::Class(class) = declaration else {
                continue;
            };
            let Some(base_name) = class.extends_name.as_deref() else {
                continue;
            };
            if class_declares_constructor(class) {
                continue;
            }
            let Some(base) = self.class_constructors.get(base_name).cloned() else {
                continue;
            };
            if base.inherited {
                continue;
            }
            let signatures = base
                .signatures
                .into_iter()
                .map(|mut signature| {
                    signature.return_type = Type::Named {
                        name: class.name.clone(),
                        arguments: Vec::new(),
                    };
                    signature
                })
                .collect();
            self.class_constructors.insert(
                class.name.clone(),
                ClassConstructorBinding {
                    signatures,
                    inherited: false,
                    visibility: base.visibility,
                },
            );
        }
    }

    pub(super) fn validate_class_constructor_group(&mut self, class: &ClassDeclaration) {
        let constructors = class
            .members
            .iter()
            .enumerate()
            .filter_map(|(index, member)| member.constructor.as_ref().map(|value| (index, value)))
            .collect::<Vec<_>>();
        let implementations = constructors
            .iter()
            .filter(|(_, constructor)| constructor.body.is_some())
            .collect::<Vec<_>>();

        for (_, constructor) in &constructors {
            self.validate_class_parameters(
                &constructor.parameters,
                constructor.body.is_some(),
                "constructor",
            );
        }

        if implementations.len() > 1 {
            for (_, constructor) in implementations {
                self.type_error(
                    &constructor.span,
                    "multiple constructor implementations are not allowed".to_string(),
                    DiagnosticCode::DuplicateDeclaration,
                );
            }
            return;
        }

        let Some(&(implementation_index, implementation)) = implementations.first() else {
            if let Some((_, signature)) = constructors.last() {
                self.type_error(
                    &signature.span,
                    "constructor implementation is missing".to_string(),
                    DiagnosticCode::TypeMismatch,
                );
            }
            return;
        };

        for &(signature_index, signature) in &constructors {
            if signature.body.is_some() {
                continue;
            }
            if signature_index >= *implementation_index
                || class.members[signature_index + 1..*implementation_index]
                    .iter()
                    .any(|member| member.constructor.is_none())
            {
                self.type_error(
                    &signature.span,
                    "constructor overload requires an immediately following implementation"
                        .to_string(),
                    DiagnosticCode::TypeMismatch,
                );
                continue;
            }
            if signature
                .parameters
                .iter()
                .any(|parameter| parameter.default.is_some())
            {
                continue;
            }
            match class_constructor_overload_is_compatible(
                signature,
                implementation,
                &self.types,
                self.max_type_expansions,
            ) {
                Ok(true) => {}
                Ok(false) => self.type_error(
                    &signature.span,
                    "constructor overload is incompatible with its implementation".to_string(),
                    DiagnosticCode::TypeMismatch,
                ),
                Err(()) => self.type_error(
                    &signature.span,
                    format!(
                        "constructor overload compatibility exceeds the {} generic-expansion limit",
                        self.max_type_expansions
                    ),
                    DiagnosticCode::ResourceLimit,
                ),
            }
        }
    }

    fn validate_class_parameters(&mut self, parameters: &[Parameter], has_body: bool, kind: &str) {
        let mut scope = self.values.clone();
        for (index, parameter) in parameters.iter().enumerate() {
            if parameter.rest && index + 1 != parameters.len() {
                self.type_error(
                    &parameter.span,
                    "a rest parameter must be last".to_string(),
                    DiagnosticCode::TypeMismatch,
                );
            }
            if parameter.rest && parameter.optional {
                self.typescript_type_error(
                    &parameter.span,
                    "a rest parameter cannot be optional or have a default initializer".to_string(),
                    DiagnosticCode::TypeMismatch,
                    if parameter.default.is_some() {
                        1048
                    } else {
                        2370
                    },
                    Vec::new(),
                );
            }
            if parameter.rest
                && parameter.annotation.as_ref().is_some_and(|annotation| {
                    !matches!(
                        annotation,
                        Type::Array(_) | Type::Tuple(_) | Type::Named { .. }
                    )
                })
            {
                self.type_error(
                    &parameter.span,
                    "a rest parameter requires an array or tuple annotation".to_string(),
                    DiagnosticCode::TypeMismatch,
                );
            }
            if parameter.rest {
                if let Some(annotation @ (Type::Tuple(_) | Type::Named { .. })) =
                    parameter.annotation.as_ref()
                {
                    let mut budget = TypeExpansionBudget::new(self.max_type_expansions);
                    if let Err(error) =
                        specialize_class_rest_annotation(annotation, &self.types, &mut budget)
                    {
                        let code = if error == TupleSpreadError::Exhausted {
                            DiagnosticCode::ResourceLimit
                        } else {
                            DiagnosticCode::UnsupportedSyntax
                        };
                        self.type_error(
                            &parameter.span,
                            "class tuple rest annotation cannot be specialized within the type budget"
                                .to_string(),
                            code,
                        );
                    }
                }
            }
            if let Some(annotation) = &parameter.annotation {
                self.check_type(annotation, &parameter.span);
            }
            if let Some(default) = &parameter.default {
                if !has_body {
                    self.type_error(
                        &parameter.span,
                        format!("a {kind} overload signature cannot have a default initializer"),
                        DiagnosticCode::TypeMismatch,
                    );
                } else {
                    self.check_direct_runtime_expression(default, &scope, &parameter.span);
                    let actual = self.infer_expression(default, &scope);
                    let expected = parameter.annotation.clone().unwrap_or(Type::Unknown);
                    if !self.is_assignable_bounded(&actual, &expected, &parameter.span) {
                        self.type_error(
                            &parameter.span,
                            format!(
                                "default initializer has type `{}`, which is not assignable to parameter `{}` of type `{}`",
                                type_label(&actual),
                                parameter.name,
                                type_label(&expected)
                            ),
                            DiagnosticCode::TypeMismatch,
                        );
                    }
                }
            }
            scope.insert(
                parameter.name.clone(),
                parameter.annotation.clone().unwrap_or(Type::Unknown),
            );
        }
    }

    pub(super) fn validate_class_method_groups(&mut self, class: &ClassDeclaration) {
        let mut implementations: BTreeMap<(bool, &str), usize> = BTreeMap::new();
        for group in &class.method_groups {
            if group.implementation_member_index.is_some() {
                *implementations
                    .entry((group.is_static, &group.name))
                    .or_default() += 1;
            }
        }

        for group in &class.method_groups {
            if implementations
                .get(&(group.is_static, group.name.as_str()))
                .copied()
                .unwrap_or(0)
                > 1
            {
                for index in group
                    .signature_member_indices
                    .iter()
                    .copied()
                    .chain(group.implementation_member_index)
                {
                    self.type_error(
                        &class.members[index].span,
                        format!("duplicate implementation of class method `{}`", group.name),
                        DiagnosticCode::DuplicateDeclaration,
                    );
                }
                continue;
            }

            let Some(implementation_index) = group.implementation_member_index else {
                if let Some(&last_signature) = group.signature_member_indices.last() {
                    self.type_error(
                        &class.members[last_signature].span,
                        format!(
                            "overload signature for class method `{}` requires an immediately following implementation",
                            group.name
                        ),
                        DiagnosticCode::TypeMismatch,
                    );
                }
                continue;
            };
            let implementation = class.members[implementation_index]
                .method
                .as_ref()
                .expect("method group implementation is parsed");
            for &signature_index in &group.signature_member_indices {
                let signature = class.members[signature_index]
                    .method
                    .as_ref()
                    .expect("method group signature is parsed");
                if signature
                    .parameters
                    .iter()
                    .any(|parameter| parameter.default.is_some())
                {
                    continue;
                }
                match class_method_overload_is_compatible(
                    signature,
                    implementation,
                    &self.types,
                    self.max_type_expansions,
                ) {
                    Ok(true) => {}
                    Ok(false) => self.type_error(
                        &signature.span,
                        format!(
                            "overload signature for class method `{}` is incompatible with its implementation",
                            group.name
                        ),
                        DiagnosticCode::TypeMismatch,
                    ),
                    Err(()) => self.type_error(
                        &signature.span,
                        format!(
                            "class method overload compatibility exceeds the {} generic-expansion limit",
                            self.max_type_expansions
                        ),
                        DiagnosticCode::ResourceLimit,
                    ),
                }
            }
        }
    }

    fn check_class_construction(
        &mut self,
        tokens: &[Token],
        scope: &BTreeMap<String, Type>,
        span: &SourceSpan,
    ) {
        let Some(call) = constructor_call_parts(tokens) else {
            return;
        };
        if call
            .receiver
            .is_some_and(|receiver| scope.get(&receiver.text) != self.values.get(&receiver.text))
        {
            return;
        }
        if !self.is_bound_class_constructor_value(&call.callee.text, scope) {
            if self
                .symbols
                .iter()
                .any(|symbol| symbol.kind == SymbolKind::Import && symbol.name == call.callee.text)
                && scope
                    .get(&call.callee.text)
                    .is_some_and(|value| !matches!(value, Type::Any | Type::Unknown))
            {
                let callable = self.functions.contains_key(&call.callee.text)
                    || matches!(scope.get(&call.callee.text), Some(Type::Function { .. }));
                self.typescript_type_error(
                    &call.callee.span(&span.module),
                    format!(
                        "imported value `{}` has no construct signature",
                        call.callee.text
                    ),
                    DiagnosticCode::TypeMismatch,
                    if callable { 7009 } else { 2351 },
                    Vec::new(),
                );
            }
            return;
        }
        let Some(binding) = self.class_constructors.get(&call.callee.text) else {
            return;
        };
        let call_span = SourceSpan::new(
            span.module.clone(),
            tokens.first().expect("constructor call has new").start,
            tokens
                .last()
                .expect("constructor call has closing paren")
                .end,
        );
        let constructor_visibility = binding.visibility;
        if !self.constructor_is_accessible(&call.callee.text, constructor_visibility) {
            self.type_error(
                &call_span,
                format!(
                    "constructor of class {} is {} and is not accessible here",
                    call.callee.text,
                    constructor_visibility.keyword()
                ),
                DiagnosticCode::TypeMismatch,
            );
        }
        let Some(binding) = self.class_constructors.get(&call.callee.text) else {
            return;
        };
        if binding.inherited {
            // Keep the unresolved base and its refusal. TypeScript diagnoses
            // arguments against the omitted constructor's zero-argument
            // recovery signature; this metadata cannot admit the class.
            let arguments = split_call_arguments(call.arguments)
                .expect("constructor call has a balanced argument list");
            if !arguments.is_empty() {
                self.typescript_type_error(
                    &call_span,
                    format!(
                        "no constructor of class {} accepts the supplied argument types",
                        call.callee.text
                    ),
                    DiagnosticCode::TypeMismatch,
                    2554,
                    vec!["0".into(), arguments.len().to_string()],
                );
            }
            return;
        }
        let arguments = split_call_arguments(call.arguments)
            .expect("constructor call has a balanced argument list");
        if let Some(alternatives) = self.optional_spread_scopes(&arguments, scope) {
            let before = self.diagnostics.len();
            for alternative in &alternatives {
                self.check_class_construction(tokens, alternative, span);
            }
            self.dedupe_diagnostics_since(before);
            return;
        }
        let signatures = binding.signatures.clone();
        let Ok(actuals) = self.expanded_call_argument_types_for(&arguments, scope, &signatures)
        else {
            self.type_error(
                &call_span,
                "a class constructor spread must have a fixed-length tuple type".to_string(),
                DiagnosticCode::TypeMismatch,
            );
            return;
        };
        match self.select_function_signature(&binding.signatures, &actuals, None) {
            Ok(Some(_)) => {}
            Ok(None) => self.rejected_call_error(
                &call_span,
                format!(
                    "no constructor of class {} accepts the supplied argument types",
                    call.callee.text
                ),
                &signatures,
                &actuals,
                self.library_values.contains(&call.callee.text)
                    && crate::diagnostic::templates::constructor_overloads(&call.callee.text) > 1,
            ),
            Err(()) => self.type_error(
                &call_span,
                format!(
                    "class constructor selection exceeds the {} generic-expansion limit",
                    self.max_type_expansions
                ),
                DiagnosticCode::ResourceLimit,
            ),
        }
    }

    pub(in crate::checker::module) fn check_class_constructions_in_expression(
        &mut self,
        tokens: &[Token],
        scope: &BTreeMap<String, Type>,
        span: &SourceSpan,
    ) {
        if !tokens.iter().any(|token| token.is("new")) {
            return;
        }
        let mut openings = Vec::new();
        let mut closes = vec![None; tokens.len()];
        for (index, token) in tokens.iter().enumerate() {
            if token.is("(") {
                openings.push(index);
            } else if token.is(")") {
                if let Some(opening) = openings.pop() {
                    closes[opening] = Some(index);
                }
            }
        }
        let mut checked = 0usize;
        for start in 0..tokens.len().saturating_sub(2) {
            if !tokens[start].is("new") || tokens[start + 1].kind != TokenKind::Identifier {
                continue;
            }
            let mut opening = start + 2;
            while tokens.get(opening).is_some_and(|token| token.is("."))
                && tokens
                    .get(opening + 1)
                    .is_some_and(|token| token.kind == TokenKind::Identifier)
            {
                opening += 2;
            }
            if !tokens.get(opening).is_some_and(|token| token.is("(")) {
                continue;
            }
            let Some(end) = closes[opening] else {
                continue;
            };
            checked += 1;
            if checked > self.max_type_expansions {
                self.type_error(
                    span,
                    format!(
                        "class construction exceeds the {} generic-expansion limit",
                        self.max_type_expansions
                    ),
                    DiagnosticCode::ResourceLimit,
                );
                return;
            }
            self.check_class_construction(&tokens[start..=end], scope, span);
        }
    }

    pub(in crate::checker::module) fn check_bound_class_calls_in_expression(
        &mut self,
        tokens: &[Token],
        scope: &BTreeMap<String, Type>,
        span: &SourceSpan,
    ) {
        if !tokens.iter().any(|token| token.is("(")) {
            return;
        }
        let mut openings = Vec::new();
        let mut closes = vec![None; tokens.len()];
        for (index, token) in tokens.iter().enumerate() {
            if token.is("(") {
                openings.push(index);
            } else if token.is(")") {
                if let Some(opening) = openings.pop() {
                    closes[opening] = Some(index);
                }
            }
        }
        let mut inspected = 0usize;
        for start in 0..tokens.len().saturating_sub(1) {
            let callee = &tokens[start];
            if callee.kind != TokenKind::Identifier
                || !tokens[start + 1].is("(")
                || start > 0 && (tokens[start - 1].is("new") || tokens[start - 1].is("."))
                || !self.is_bound_class_constructor_value(&callee.text, scope)
            {
                continue;
            }
            let Some(end) = closes[start + 1] else {
                continue;
            };
            inspected += 1;
            if inspected > self.max_type_expansions {
                self.type_error(
                    span,
                    format!(
                        "class call scan exceeds the {} generic-expansion limit",
                        self.max_type_expansions
                    ),
                    DiagnosticCode::ResourceLimit,
                );
                return;
            }
            self.type_error(
                &SourceSpan::new(&span.module, callee.start, tokens[end].end),
                format!("class {} cannot be called without `new`", callee.text),
                DiagnosticCode::TypeMismatch,
            );
        }
    }
}

pub(in crate::checker) fn class_export(class: &ClassDeclaration) -> ExportedClass {
    ExportedClass {
        source_name: class.name.clone(),
        instance_type: class_instance_type(class),
        constructor_type: class_constructor_side_type(class),
        constructor_binding: ClassConstructorBinding {
            signatures: class_constructor_signatures(class),
            inherited: class.extends_name.is_some() && !class_declares_constructor(class),
            visibility: class_constructor_visibility(class),
        },
        value_exported: class.exported,
        heritage_depth: 0,
    }
}

pub(in crate::checker) fn class_instance_type(class: &ClassDeclaration) -> Type {
    Type::Record(class_method_fields(class, false))
}
