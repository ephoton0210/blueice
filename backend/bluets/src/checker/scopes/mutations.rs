// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

//! Writes resolve against lexical identities, rather than a name-only type map.

use super::expressions::is_value_name;
use super::*;

pub(super) struct MutationUse {
    pub(super) scope: ScopeId,
    pub(super) target: Vec<Token>,
    pub(super) operator: Token,
    pattern: bool,
}

impl BindingKind {
    fn refusal(self) -> Option<(&'static str, &'static str)> {
        match self {
            Self::Mutable => None,
            Self::Const => Some(("TS2588", "a constant")),
            Self::Import | Self::NamespaceImport => Some(("TS2632", "an import")),
            Self::Function => Some(("TS2630", "a function")),
            Self::Class => Some(("TS2629", "a class")),
            Self::Enum => Some(("TS2628", "an enum")),
            Self::Namespace => Some(("TS2631", "a namespace")),
        }
    }
}

impl ScopeModel<'_> {
    pub(super) fn class_this(&mut self, scope: ScopeId, name: &str, is_static: bool) {
        let path = &self.scopes[scope].namespace_path;
        let name = if path.is_empty() {
            name.to_string()
        } else {
            format!("{path}.{name}")
        };
        self.value(
            scope,
            "this",
            0,
            false,
            false,
            Type::Named {
                name: if is_static {
                    format!("typeof {name}")
                } else {
                    name
                },
                arguments: Vec::new(),
            },
        );
    }

    pub(super) fn variable_initializer(
        &mut self,
        scope: ScopeId,
        name: &str,
        tokens: &[Token],
        start: usize,
        ready: usize,
    ) {
        let after = tokens.partition_point(|token| token.start < ready);
        let Some(declaration) = tokens.get(start + 1..after) else {
            return;
        };
        let Some(equal) = targets::top_level(declaration, "=") else {
            return;
        };
        if let (Some(first), Some(last), Some(binding)) = (
            declaration.get(equal + 1),
            declaration.last(),
            self.scopes[scope].values.get_mut(name),
        ) {
            binding.initializer = Some((first.start, last.end));
        }
    }

    pub(super) fn binding_kind(&mut self, scope: ScopeId, name: &str, kind: BindingKind) {
        if let Some(binding) = self.scopes[scope].values.get_mut(name) {
            binding.kind = kind;
        }
    }

    pub(super) fn mutation(&mut self, tokens: &[Token], index: usize, scope: ScopeId) {
        let operator = &tokens[index];
        if self.erased(operator) {
            return;
        }
        let limit = self.max_type_expansions.saturating_mul(32).max(64);
        let target = if targets::ASSIGNMENTS.contains(&operator.text.as_str()) {
            targets::backward(tokens, index, limit)
        } else if operator.is("++") || operator.is("--") {
            if index > 0
                && (is_value_name(&tokens[index - 1])
                    || tokens[index - 1].text.starts_with('#')
                    || tokens[index - 1].is(")")
                    || tokens[index - 1].is("]"))
            {
                targets::backward(tokens, index, limit)
            } else {
                targets::forward(tokens, index + 1, limit)
            }
        } else {
            return;
        };
        match target {
            Ok(target) => self.write_target(target, operator, scope, false),
            Err(()) => self.mutation_limit(operator),
        }
    }

    pub(super) fn iteration_mutation(&mut self, tokens: &[Token], index: usize, scope: ScopeId) {
        if let Some((target, operator)) = targets::iteration(tokens, index) {
            self.write_target(target, operator, scope, true);
        }
    }

    fn write_target(
        &mut self,
        target: &[Token],
        operator: &Token,
        scope: ScopeId,
        iteration: bool,
    ) {
        let target: Vec<Token> = target
            .iter()
            .filter(|token| !self.erased(token))
            // Namespace parsing merges `Area.value` for the existing type
            // checker. Writes still need each original identifier's span.
            .flat_map(|token| {
                if token.kind == TokenKind::Identifier && token.text.contains('.') {
                    let start = self
                        .tokens
                        .partition_point(|source| source.start < token.start);
                    let end = self
                        .tokens
                        .partition_point(|source| source.start < token.end);
                    self.tokens[start..end].to_vec()
                } else {
                    vec![token.clone()]
                }
            })
            .collect();
        let target = targets::strip(&target);
        let pattern = iteration
            || target
                .first()
                .is_some_and(|token| token.is("[") || token.is("{"));
        let mut leaves = Vec::new();
        if targets::pattern(target, &mut leaves, 0).is_err() {
            self.mutation_limit(operator);
            return;
        }
        if self.mutations.len().saturating_add(leaves.len())
            > self.max_type_expansions.saturating_mul(16).max(256)
        {
            self.mutation_limit(operator);
            return;
        }
        for leaf in leaves {
            if !leaf.is_empty() {
                self.mutations.push(MutationUse {
                    scope,
                    target: leaf.to_vec(),
                    operator: operator.clone(),
                    pattern,
                });
            }
        }
    }

    pub(super) fn mutation_limit(&mut self, token: &Token) {
        if self.mutation_error.is_none() {
            self.mutation_error = Some(self.mutation_limit_diagnostic(token));
        }
    }

    fn mutation_limit_diagnostic(&self, token: &Token) -> Diagnostic {
        Diagnostic::error(
            DiagnosticCode::ResourceLimit,
            token.span(&self.module.id),
            "mutation checking exceeds its bounded target/type-expansion limit",
        )
    }

    pub(super) fn mutation_diagnostics(&self) -> Vec<Diagnostic> {
        let mut diagnostics: Vec<_> = self.mutation_error.iter().cloned().collect();
        let mut seen = BTreeSet::new();
        for usage in &self.mutations {
            let target = targets::strip(&usage.target);
            let diagnostic = if let [name] = target {
                if self.declaration_names.contains(&name.start) {
                    continue;
                }
                let Some((_, Some(binding))) =
                    self.resolve(usage.scope, &name.text, Meaning::Value)
                else {
                    continue;
                };
                if binding.type_only {
                    continue;
                }
                binding.kind.refusal().map(|(ts, kind)| {
                    Diagnostic::error(
                        DiagnosticCode::ImmutableAssignment,
                        name.span(&self.module.id),
                        format!(
                            "{ts}: cannot assign to `{}` because it is {kind}",
                            name.text
                        ),
                    )
                })
            } else {
                self.readonly_target(usage)
            };
            if let Some(diagnostic) = diagnostic {
                if seen.insert((diagnostic.span.start, diagnostic.code.to_string())) {
                    diagnostics.push(diagnostic);
                }
            }
        }
        diagnostics
    }

    fn readonly_target(&self, usage: &MutationUse) -> Option<Diagnostic> {
        if usage
            .target
            .iter()
            .filter(|token| token.is(".") || token.is("["))
            .count()
            > 128
        {
            return Some(self.mutation_limit_diagnostic(&usage.operator));
        }
        let (receiver, _, token) = targets::member(&usage.target)?;
        let known_property = self.member_key(&usage.target, usage.scope);
        let property = known_property.as_deref();
        let private_slot = property
            .filter(|_| token.kind == TokenKind::Identifier && token.text.starts_with('#'))
            .and_then(|property| self.private_slot(usage.scope, property));
        let private_readonly = private_slot.is_some_and(|slot| slot.readonly);
        let receiver = targets::strip(receiver);
        let namespace_readonly = self
            .namespace_binding(receiver, usage.scope, 0)
            .is_some_and(|binding| {
                binding.kind == BindingKind::NamespaceImport
                    || binding
                        .namespace
                        .as_ref()
                        .and_then(|id| self.namespace_members.get(id))
                        .and_then(|members| {
                            property.and_then(|property| members.values.get(property))
                        })
                        .is_some_and(|member| member.kind == BindingKind::Const)
            });
        if !namespace_readonly && !usage.pattern && !private_readonly {
            return None;
        }
        let mut budget = TypeExpansionBudget::new(self.max_type_expansions);
        let readonly = namespace_readonly
            || private_readonly
            || self
                .target_type(receiver, usage.scope, &mut BTreeSet::new(), &mut budget, 0)
                .is_some_and(|owner| match property {
                    Some(property) => readonly_property(
                        &owner,
                        property,
                        &self.type_definitions,
                        &mut HashSet::new(),
                        &mut budget,
                        self.private_marker(usage.scope),
                        0,
                    ),
                    None => contains_readonly_member(
                        &owner,
                        &self.type_definitions,
                        &mut HashSet::new(),
                        &mut budget,
                    )
                    .unwrap_or(false),
                });
        if budget.exhausted {
            return Some(self.mutation_limit_diagnostic(&usage.operator));
        }
        if !readonly {
            return None;
        }
        let own_constructor = matches!(receiver, [token] if token.is("this"))
            && self
                .constructor_fields
                .get(&self.scopes[usage.scope].execution)
                .is_some_and(|fields| property.is_some_and(|property| fields.contains(property)));
        if own_constructor {
            return None;
        }
        Some(Diagnostic::error(
            DiagnosticCode::TypeMismatch,
            token.span(&self.module.id),
            format!(
                "TS{}: cannot mutate readonly property `{}`",
                if private_slot.is_some_and(|slot| slot.method) {
                    2803
                } else {
                    2540
                },
                property.unwrap_or("computed")
            ),
        ))
    }

    fn member_key(&self, tokens: &[Token], scope: ScopeId) -> Option<String> {
        let (receiver, property, _) = targets::member(tokens)?;
        property.map(str::to_string).or_else(|| {
            let [open, name, close] = targets::strip(tokens).get(receiver.len()..)? else {
                return None;
            };
            if !open.is("[") || !close.is("]") {
                return None;
            }
            let (_, Some(binding)) = self.resolve(scope, &name.text, Meaning::Value)? else {
                return None;
            };
            let Type::Literal(value) = &binding.declared_type else {
                return None;
            };
            if (value.starts_with('\'') && value.ends_with('\''))
                || (value.starts_with('"') && value.ends_with('"'))
            {
                return value
                    .get(1..value.len() - 1)
                    .filter(|value| !value.contains('\\'))
                    .map(str::to_string);
            }
            value.parse::<usize>().ok().map(|index| index.to_string())
        })
    }

    fn namespace_binding(
        &self,
        tokens: &[Token],
        scope: ScopeId,
        depth: usize,
    ) -> Option<&Binding> {
        if depth > 128 {
            return None;
        }
        let tokens = targets::strip(tokens);
        if let [name] = tokens {
            return self
                .resolve(scope, &name.text, Meaning::Value)
                .and_then(|(_, binding)| binding);
        }
        let (receiver, property, _) = targets::member(tokens)?;
        let owner = self.namespace_binding(receiver, scope, depth + 1)?;
        self.namespace_members
            .get(owner.namespace.as_ref()?)?
            .values
            .get(property?)
    }

    fn indexed_type(
        &self,
        owner: &Type,
        key: Option<&str>,
        budget: &mut TypeExpansionBudget,
        depth: usize,
    ) -> Option<Type> {
        if depth > 128 {
            budget.exhausted = true;
            return None;
        }
        if !budget.consume() {
            return None;
        }
        match owner {
            Type::Array(element) => Some(*element.clone()),
            Type::Tuple(elements) => {
                let index = key.and_then(|key| key.parse::<usize>().ok());
                let rest = elements
                    .iter()
                    .position(|element| element.rest)
                    .unwrap_or(elements.len());
                if let Some(index) = index.filter(|index| *index < rest) {
                    return elements
                        .get(index)
                        .map(|element| element.annotation.clone());
                }
                let start = if index.is_some() { rest } else { 0 };
                let values: Vec<_> = elements[start..]
                    .iter()
                    .map(|element| match &element.annotation {
                        Type::Array(value) if element.rest => *value.clone(),
                        value => value.clone(),
                    })
                    .collect();
                match values.as_slice() {
                    [] => None,
                    [value] => Some(value.clone()),
                    _ => Some(Type::Union(values)),
                }
            }
            Type::Named { .. } => {
                let value = instantiate_named(
                    owner,
                    &self.type_definitions,
                    &mut HashSet::new(),
                    budget,
                    "mutation index",
                )?;
                self.indexed_type(&value, key, budget, depth + 1)
            }
            Type::Union(parts) | Type::Intersection(parts) => {
                let values: Vec<_> = parts
                    .iter()
                    .filter_map(|part| self.indexed_type(part, key, budget, depth + 1))
                    .collect();
                match values.as_slice() {
                    [] => None,
                    [value] => Some(value.clone()),
                    _ => Some(Type::Union(values)),
                }
            }
            _ => None,
        }
    }

    fn target_type(
        &self,
        tokens: &[Token],
        scope: ScopeId,
        visited: &mut BTreeSet<(ScopeId, String)>,
        budget: &mut TypeExpansionBudget,
        depth: usize,
    ) -> Option<Type> {
        if depth > 128 {
            budget.exhausted = true;
            return None;
        }
        if !budget.consume() {
            return None;
        }
        let tokens = targets::strip(tokens);
        if let [name] = tokens {
            let (bound_scope, binding) = self.resolve(scope, &name.text, Meaning::Query)?;
            let binding = binding?;
            if visited.len() >= 128 {
                budget.exhausted = true;
                return None;
            }
            if !visited.insert((bound_scope, name.text.clone())) {
                return None;
            }
            if binding.declared_type != Type::Unknown {
                return Some(binding.declared_type.clone());
            }
            let (start, end) = binding.initializer?;
            let initializer = self.tokens.partition_point(|token| token.start < start);
            let after = self.tokens.partition_point(|token| token.start < end);
            return self.target_type(
                &self.tokens[initializer..after],
                self.scope_at(start),
                visited,
                budget,
                depth + 1,
            );
        }
        if let Some((receiver, _, token)) = targets::member(tokens) {
            let property = self.member_key(tokens, scope);
            if let Some(property) = property.as_deref() {
                if token.kind == TokenKind::Identifier
                    && token.text.starts_with('#')
                    && self.private_slot(scope, property).is_some()
                {
                    return self.private_slot_type(scope, property, budget);
                }
            }
            let owner = self.target_type(receiver, scope, visited, budget, depth + 1)?;
            if let Some(property) = property.as_deref() {
                if let PropertyType::Found { value, .. } = property_type(
                    &owner,
                    property,
                    &self.type_definitions,
                    &mut HashSet::new(),
                    budget,
                ) {
                    return Some(value);
                }
                if let Some(value) = mutation_field_type(
                    &owner,
                    property,
                    &self.type_definitions,
                    &mut HashSet::new(),
                    budget,
                    self.private_marker(scope),
                    depth + 1,
                ) {
                    return Some(value);
                }
            }
            return self.indexed_type(&owner, property.as_deref(), budget, depth + 1);
        }
        if tokens.last().is_some_and(|token| token.is(")")) {
            let open = (0..tokens.len()).find(|index| {
                tokens[*index].is("(") && targets::close(tokens, *index) == Some(tokens.len() - 1)
            })?;
            if tokens.first().is_some_and(|token| token.is("new")) {
                return tokens.get(1).map(|name| Type::Named {
                    name: name.text.clone(),
                    arguments: Vec::new(),
                });
            }
            if let Type::Function { result, .. } =
                self.target_type(&tokens[..open], scope, visited, budget, depth + 1)?
            {
                return Some(*result);
            }
        }
        None
    }
}
