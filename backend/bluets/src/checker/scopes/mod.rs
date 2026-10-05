// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

//! Lexical identities shared by value, erased-type and initialization checks.
//! Type inference remains in the module checker; lookup never falls back to
//! an inferred `unknown`. All bindings are collected before resolving uses.

use super::*;
use crate::parser::{NestedFunctionBody, VariableDeclaration, VariableKind};

mod expressions;
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
    declared_type: Type,
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
}

impl<'a> ScopeModel<'a> {
    pub(super) fn new(
        project: &'a Project,
        module: &'a Module,
        values: &BTreeMap<String, Type>,
        types: &BTreeMap<String, TypeDefinition>,
        ambient: Option<&AmbientDeclarations>,
        max_type_expansions: usize,
    ) -> Self {
        let tokens = crate::syntax::lex_with_limits(
            &module.id,
            &module.source,
            module.source.len(),
            module.source.len().saturating_add(1),
        )
        .unwrap_or_default();
        let mut model = Self {
            module,
            project,
            scopes: Vec::new(),
            references: Vec::new(),
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
        };
        model.child(
            None,
            SourceSpan::new(&module.id, 0, module.source.len()),
            true,
            false,
        );
        // Preserve the already supported ECMAScript/console names until K.1.4
        // replaces this explicit compatibility list with versioned declarations.
        // These are static names; they confer no runtime or host authority.
        for name in [
            "undefined",
            "NaN",
            "Infinity",
            "globalThis",
            "console",
            "Object",
            "Function",
            "Boolean",
            "Number",
            "String",
            "Array",
            "Promise",
            "Symbol",
            "BigInt",
            "Math",
            "JSON",
            "Reflect",
            "Date",
            "RegExp",
            "Map",
            "Set",
            "WeakMap",
            "WeakSet",
            "Error",
            "EvalError",
            "RangeError",
            "ReferenceError",
            "SyntaxError",
            "TypeError",
            "URIError",
            "AggregateError",
            "ArrayBuffer",
            "SharedArrayBuffer",
            "DataView",
            "Int8Array",
            "Uint8Array",
            "Uint8ClampedArray",
            "Int16Array",
            "Uint16Array",
            "Int32Array",
            "Uint32Array",
            "Float32Array",
            "Float64Array",
            "BigInt64Array",
            "BigUint64Array",
            "Atomics",
            "Intl",
            "parseInt",
            "parseFloat",
            "isNaN",
            "isFinite",
            "decodeURI",
            "decodeURIComponent",
            "encodeURI",
            "encodeURIComponent",
            "eval",
        ] {
            model.value(0, name, 0, false, false, Type::Unknown);
        }
        // Bound imports and owner ambient declarations supply exact names.
        // Local AST declarations below replace these entries with lifetimes.
        for (name, value) in values {
            model.value(0, name, 0, false, false, value.clone());
        }
        model.scopes[0].types.extend(types.keys().cloned());
        if let Some(ambient) = ambient {
            for (name, value) in &ambient.values {
                model.value(0, name, 0, false, false, value.clone());
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
        self.scopes.push(Scope {
            parent,
            span,
            var_boundary,
            execution,
            namespace_path,
            namespace_identity,
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
                declared_type,
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
                Some((_, Some(binding))) if binding.type_only => Some(format!(
                    "`{root}` has no value binding (type-only declaration or import)"
                )),
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
                    diagnostics.push(Diagnostic::error(code, reference.span.clone(), message));
                }
            }
        }
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
            return (!binding.type_only).then(|| binding.declared_type.clone());
        }
        let mut parts = name.split('.');
        let root = parts.next()?;
        let (_, binding) = self.resolve(scope, root, Meaning::Query)?;
        let binding = binding?;
        if binding.type_only {
            return None;
        }
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

    fn reference(&mut self, scope: ScopeId, token: &Token) {
        self.references.push(Reference {
            scope,
            name: token.text.clone(),
            meaning: Meaning::Value,
            span: token.span(&self.module.id),
        });
    }
}
