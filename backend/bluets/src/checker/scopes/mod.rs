// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

//! Lexical identities shared by value, erased-type and initialization checks.
//! Type inference remains in the module checker; lookup never falls back to
//! an inferred `unknown`. All bindings are collected before resolving uses.

use super::*;
use crate::parser::{NestedFunctionBody, VariableDeclaration, VariableKind};

mod checking_flags;
mod diagnostics;
pub(super) mod emission;
mod expressions;
pub(super) mod flow;
mod flow_bindings;
mod mutations;
mod private;
mod switches;
pub(super) mod targets;
mod walk;

type ScopeId = usize;

#[derive(Clone, Copy, PartialEq, Eq)]
enum Meaning {
    Value,
    Type,
    Query,
}

#[derive(Clone)]
struct Binding {
    ready: usize,
    class: bool,
    /// A type-only import blocks value lookup rather than exposing an outer value.
    type_only: bool,
    type_only_export: Option<SourceSpan>,
    declared_type: Type,
    kind: BindingKind,
    namespace: Option<String>,
    initializer: Option<(usize, usize)>,
    library: bool,
    annotated: bool,
}

#[derive(Clone, Copy, Default, PartialEq, Eq)]
pub(super) enum BindingKind {
    #[default]
    Mutable,
    Const,
    Import,
    NamespaceImport,
    Function,
    Class,
    Enum,
    Namespace,
}

struct Scope {
    parent: Option<ScopeId>,
    /// `var` stops at a function, module, namespace or static block.
    var_boundary: bool,
    /// Uses across a function boundary are delayed, so textual order alone
    /// does not diagnose a closure capturing a later outer declaration.
    execution: ScopeId,
    span: SourceSpan,
    namespace_path: String,
    namespace_identity: String,
    /// ECMAScript private identifiers retain their lexical declaring class.
    class_owner: Option<ScopeId>,
    ambient_exports: bool,
    values: BTreeMap<String, Binding>,
    types: BTreeSet<String>,
}

struct Reference {
    scope: ScopeId,
    name: String,
    meaning: Meaning,
    span: SourceSpan,
}

#[derive(Default)]
struct NamespaceBindings {
    values: BTreeMap<String, Binding>,
    types: BTreeSet<String>,
}

pub(super) struct ScopeModel<'a> {
    module: &'a Module,
    project: &'a Project,
    scopes: Vec<Scope>,
    references: Vec<Reference>,
    arguments_reads: BTreeMap<ScopeId, Vec<SourceSpan>>,
    /// Existing, bound qualified types (including imported namespace members).
    qualified_types: BTreeSet<String>,
    type_definitions: BTreeMap<String, TypeDefinition>,
    max_type_expansions: usize,
    tokens: Vec<Token>,
    visited_functions: BTreeSet<usize>,
    local_functions: BTreeMap<usize, FunctionDeclaration>,
    local_variables: BTreeMap<usize, VariableDeclaration>,
    namespace_members: BTreeMap<String, NamespaceBindings>,
    scope_ranges: Vec<(usize, ScopeId)>,
    erased_ranges: Vec<(usize, usize)>,
    declaration_names: BTreeSet<usize>,
    mutations: Vec<mutations::MutationUse>,
    mutation_error: Option<Diagnostic>,
    constructor_fields: BTreeMap<ScopeId, BTreeSet<String>>,
    private_classes: BTreeMap<ScopeId, private::PrivateClass>,
    parameters: BTreeMap<(ScopeId, String), SourceSpan>,
    catch_bindings: BTreeMap<(ScopeId, String), bool>,
    type_declarations: BTreeMap<(ScopeId, String), (SourceSpan, bool)>,
    flow_failure: std::cell::RefCell<Option<Diagnostic>>,
}

impl<'a> ScopeModel<'a> {
    /// Imports belong to the module scope; uses in nested scopes resolve the
    /// same binding, while local shadows retain their own value meaning.
    pub(super) fn bind_type_only_export(&mut self, name: &str, span: &SourceSpan) {
        if let Some(binding) = self.scopes[0].values.get_mut(name) {
            binding.type_only = true;
            binding.type_only_export = Some(span.clone());
        }
    }

    pub(super) fn new(
        project: &'a Project,
        module: &'a Module,
        values: &BTreeMap<String, Type>,
        types: &BTreeMap<String, TypeDefinition>,
        ambient: Option<&AmbientDeclarations>,
        library_target: Option<crate::compiler::EcmaTarget>,
        max_type_expansions: usize,
    ) -> Self {
        let tokens = crate::syntax::lex_with_limits(
            &module.id,
            &module.source,
            module.source.len(),
            module.source.len().saturating_add(1),
        )
        .unwrap_or_default();
        let tokens = targets::private_identifiers(tokens);
        let mut model = Self {
            module,
            project,
            scopes: Vec::new(),
            references: Vec::new(),
            arguments_reads: BTreeMap::new(),
            qualified_types: types.keys().cloned().collect(),
            type_definitions: types.clone(),
            max_type_expansions,
            tokens,
            visited_functions: BTreeSet::new(),
            local_functions: BTreeMap::new(),
            local_variables: BTreeMap::new(),
            namespace_members: BTreeMap::new(),
            scope_ranges: Vec::new(),
            erased_ranges: Vec::new(),
            declaration_names: BTreeSet::new(),
            mutations: Vec::new(),
            mutation_error: None,
            constructor_fields: BTreeMap::new(),
            private_classes: BTreeMap::new(),
            parameters: BTreeMap::new(),
            catch_bindings: BTreeMap::new(),
            type_declarations: BTreeMap::new(),
            flow_failure: std::cell::RefCell::new(None),
        };
        model.child(
            None,
            SourceSpan::new(&module.id, 0, module.source.len()),
            true,
            false,
        );
        // Library name recognition is static even when the page call policy
        // withholds its value/function bindings. Console remains the explicit
        // compatibility name; DOM and other host names require owner declarations.
        model.value(0, "console", 0, false, false, Type::Unknown);
        model.value(0, "undefined", 0, false, false, Type::Undefined);
        for module in library_target
            .into_iter()
            .flat_map(crate::standard_library::modules)
        {
            for declaration in &module.declarations {
                let name = match declaration {
                    Declaration::Variable(value) => &value.name,
                    Declaration::Function(value) => &value.name,
                    Declaration::Class(value) => &value.name,
                    Declaration::Namespace(value) => &value.name,
                    _ => continue,
                };
                model.value(0, name, 0, false, false, Type::Unknown);
                model.scopes[0].values.get_mut(name).unwrap().library = true;
            }
        }
        // Bound imports and owner ambient declarations supply exact names.
        // Local AST declarations below replace these entries with lifetimes.
        for (name, value) in values {
            let library = model.scopes[0]
                .values
                .get(name)
                .is_some_and(|binding| binding.library);
            model.value(0, name, 0, false, false, value.clone());
            model.scopes[0].values.get_mut(name).unwrap().library = library;
        }
        model.scopes[0].types.extend(types.keys().cloned());
        if let Some(ambient) = ambient {
            for (name, value) in &ambient.values {
                model.value(0, name, 0, false, false, value.clone());
                if let Some(kind) = ambient.binding_kinds.get(name) {
                    model.binding_kind(0, name, *kind);
                }
            }
            model.scopes[0].types.extend(ambient.types.keys().cloned());
        }
        for edit in module
            .edits
            .iter()
            .filter(|edit| edit.replacement.is_empty())
        {
            if let Some(last) = model
                .erased_ranges
                .last_mut()
                .filter(|last| edit.start <= last.1)
            {
                last.1 = last.1.max(edit.end);
            } else {
                model.erased_ranges.push((edit.start, edit.end));
            }
        }
        model.declarations(&module.declarations, 0);
        model.index_scopes();
        // The parser records type names even inside assertions that disappear
        // during emit. Scope spans attach them to the same lexical identities.
        for reference in &module.type_references {
            let scope = model.scope_at(reference.span.start);
            model.references.push(Reference {
                scope,
                name: reference.name.clone(),
                span: reference.span.clone(),
                meaning: if reference.value_query {
                    Meaning::Query
                } else {
                    Meaning::Type
                },
            });
        }
        model
    }

    fn child(
        &mut self,
        parent: Option<ScopeId>,
        span: SourceSpan,
        var_boundary: bool,
        delayed: bool,
    ) -> ScopeId {
        let id = self.scopes.len();
        let execution = if delayed {
            id
        } else {
            parent.map_or(id, |parent| self.scopes[parent].execution)
        };
        let namespace_path = parent.map_or_else(String::new, |parent| {
            self.scopes[parent].namespace_path.clone()
        });
        let namespace_identity = parent.map_or_else(String::new, |parent| {
            self.scopes[parent].namespace_identity.clone()
        });
        let class_owner = parent.and_then(|parent| self.scopes[parent].class_owner);
        self.scopes.push(Scope {
            parent,
            span,
            var_boundary,
            execution,
            namespace_path,
            namespace_identity,
            class_owner,
            ambient_exports: false,
            values: BTreeMap::new(),
            types: BTreeSet::new(),
        });
        id
    }

    fn value(
        &mut self,
        scope: ScopeId,
        name: &str,
        ready: usize,
        class: bool,
        type_only: bool,
        declared_type: Type,
    ) {
        self.scopes[scope].values.insert(
            name.to_string(),
            Binding {
                ready,
                class,
                type_only,
                type_only_export: None,
                declared_type,
                kind: BindingKind::Mutable,
                namespace: None,
                initializer: None,
                annotated: false,
                library: false,
            },
        );
    }

    fn index_scopes(&mut self) {
        let mut events: BTreeMap<usize, Vec<(bool, ScopeId)>> = BTreeMap::new();
        for (id, scope) in self.scopes.iter().enumerate() {
            if scope.span.start < scope.span.end {
                events.entry(scope.span.start).or_default().push((true, id));
                events.entry(scope.span.end).or_default().push((false, id));
            }
        }
        let mut active = BTreeSet::new();
        for (offset, events) in events {
            for (enter, id) in events {
                let key = (
                    self.scopes[id].span.end - self.scopes[id].span.start,
                    std::cmp::Reverse(id),
                );
                if enter {
                    active.insert(key);
                } else {
                    active.remove(&key);
                }
            }
            self.scope_ranges
                .push((offset, active.first().map_or(0, |(_, id)| id.0)));
        }
    }

    fn scope_at(&self, offset: usize) -> ScopeId {
        let end = self
            .scope_ranges
            .partition_point(|(start, _)| *start <= offset);
        end.checked_sub(1)
            .map_or(0, |index| self.scope_ranges[index].1)
    }

    fn erased(&self, token: &Token) -> bool {
        let end = self
            .erased_ranges
            .partition_point(|(start, _)| *start <= token.start);
        end.checked_sub(1)
            .is_some_and(|index| token.end <= self.erased_ranges[index].1)
    }

    fn resolve(
        &self,
        mut scope: ScopeId,
        name: &str,
        meaning: Meaning,
    ) -> Option<(ScopeId, Option<&Binding>)> {
        loop {
            let here = &self.scopes[scope];
            let merged = self.merged_members(scope);
            if meaning == Meaning::Type {
                if here.types.contains(name)
                    || merged.is_some_and(|members| members.types.contains(name))
                {
                    return Some((scope, None));
                }
            } else if let Some(binding) = here
                .values
                .get(name)
                .or_else(|| merged.and_then(|members| members.values.get(name)))
            {
                return Some((scope, Some(binding)));
            }
            scope = here.parent?;
        }
    }

    /// Only namespace body scopes consult the shared exports. Function and
    /// block scopes must reach them through their parent, preserving shadowing
    /// and the execution identity of the declaration.
    fn merged_members(&self, scope: ScopeId) -> Option<&NamespaceBindings> {
        let here = &self.scopes[scope];
        if here.namespace_identity.is_empty()
            || here.parent.is_some_and(|parent| {
                self.scopes[parent].namespace_identity == here.namespace_identity
            })
        {
            return None;
        }
        self.namespace_members.get(&here.namespace_identity)
    }

    fn qualified_type_exists(&self, mut scope: ScopeId, name: &str) -> bool {
        loop {
            let path = &self.scopes[scope].namespace_path;
            if self.scopes[scope].types.contains(name)
                || self.qualified_types.contains(name)
                || (!path.is_empty() && self.qualified_types.contains(&format!("{path}.{name}")))
            {
                return true;
            }
            let Some(parent) = self.scopes[scope].parent else {
                return false;
            };
            scope = parent;
        }
    }

    pub(super) fn diagnostics(&self) -> Vec<Diagnostic> {
        let mut diagnostics = Vec::new();
        let mut seen = BTreeSet::new();
        for reference in &self.references {
            let root = reference.name.split('.').next().unwrap_or(&reference.name);
            let resolved = self.resolve(reference.scope, root, reference.meaning);
            let code = if reference.meaning == Meaning::Type {
                DiagnosticCode::UnknownType
            } else {
                DiagnosticCode::UnknownName
            };
            let message = match resolved {
                _ if reference.meaning == Meaning::Type
                    && reference.name.contains('.')
                    && !self.qualified_type_exists(reference.scope, &reference.name) =>
                {
                    Some(format!("cannot find type `{}`", reference.name))
                }
                Some((_, Some(binding)))
                    if binding.type_only
                        && (reference.meaning == Meaning::Value
                            || reference.meaning == Meaning::Query
                                && binding.kind == BindingKind::Namespace) =>
                {
                    Some(format!(
                        "`{root}` has no value binding (type-only declaration or import)"
                    ))
                }
                Some((scope, Some(binding)))
                    if reference.meaning == Meaning::Value
                        && reference.span.start < binding.ready
                        && self.scopes[scope].execution
                            == self.scopes[reference.scope].execution =>
                {
                    let ts = if binding.class { "TS2449" } else { "TS2448" };
                    Some(format!("{ts}: `{root}` is used before its declaration"))
                }
                Some(_) => None,
                None if reference.meaning == Meaning::Type
                    && (self.qualified_types.contains(&reference.name)
                        || (reference.name.contains('.')
                            && self.qualified_type_exists(reference.scope, &reference.name)
                            && self
                                .resolve(reference.scope, root, Meaning::Query)
                                .is_some())) =>
                {
                    None
                }
                None => Some(format!(
                    "cannot find {} `{root}`",
                    if reference.meaning == Meaning::Type {
                        "type"
                    } else {
                        "name"
                    }
                )),
            };
            if let Some(message) = message {
                if seen.insert((reference.span.start, message.clone())) {
                    let code = if message.starts_with("TS244") {
                        DiagnosticCode::UsedBeforeDeclaration
                    } else {
                        code
                    };
                    diagnostics.push(self.diagnostic_counterpart(
                        reference,
                        Diagnostic::error(code, reference.span.clone(), message),
                    ));
                }
            }
        }
        diagnostics.extend(self.mutation_diagnostics());
        diagnostics
    }

    pub(super) fn query_types(&self) -> BTreeMap<String, Type> {
        self.module
            .type_references
            .iter()
            .filter(|reference| reference.value_query)
            .filter_map(|reference| {
                self.query_type(&reference.name, reference.span.start)
                    .map(|value| {
                        (
                            format!("typeof {}@{}", reference.name, reference.span.start),
                            value,
                        )
                    })
            })
            .collect()
    }

    pub(super) fn query_type(&self, name: &str, offset: usize) -> Option<Type> {
        let scope = self.scope_at(offset);
        if let Some((_, Some(binding))) = self.resolve(scope, name, Meaning::Query) {
            return Some(binding.declared_type.clone());
        }
        let mut parts = name.split('.');
        let root = parts.next()?;
        let (_, binding) = self.resolve(scope, root, Meaning::Query)?;
        let binding = binding?;
        let mut value = binding.declared_type.clone();
        let mut budget = TypeExpansionBudget::new(self.max_type_expansions);
        for part in parts {
            match property_type(
                &value,
                part,
                &self.type_definitions,
                &mut HashSet::new(),
                &mut budget,
            ) {
                PropertyType::Found { value: next, .. } => value = next,
                _ => return None,
            }
        }
        Some(value)
    }

    /// Identifies the lexical declaration owning a type name at a reference.
    pub(super) fn type_name_is_bound_in(
        &self,
        name: &str,
        offset: usize,
        origin: &SourceSpan,
    ) -> bool {
        self.resolve(self.scope_at(offset), name, Meaning::Type)
            .is_some_and(|(scope, _)| self.scopes[scope].span == *origin)
    }

    /// An implicit library default is irrelevant to opaque writes until source
    /// code actually references it. Resolve identities to preserve local/owner
    /// shadows, and retain references inside alias initializers and closures.
    pub(super) fn unused_library_value(&self, name: &str, offset: usize) -> bool {
        let Some((_, Some(binding))) = self.resolve(self.scope_at(offset), name, Meaning::Value)
        else {
            return false;
        };
        binding.library
            && !self.references.iter().any(|reference| {
                reference.name == name
                    && reference.meaning == Meaning::Value
                    && self
                        .resolve(reference.scope, name, Meaning::Value)
                        .is_some_and(|(_, binding)| binding.is_some_and(|binding| binding.library))
            })
    }

    fn reference(&mut self, scope: ScopeId, token: &Token) {
        let span = token.span(&self.module.id);
        if token.is("arguments") {
            self.arguments_reads
                .entry(self.scopes[scope].execution)
                .or_default()
                .push(span.clone());
        }
        self.references.push(Reference {
            scope,
            name: token.text.clone(),
            meaning: Meaning::Value,
            span,
        });
    }
}
