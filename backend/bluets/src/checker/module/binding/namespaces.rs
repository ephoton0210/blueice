// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

//! Namespace binding and checking (J.3.5).
//!
//! A namespace body is checked as a module of its own: a second checker binds
//! and checks the body's declarations over a copy of the enclosing scope, so the
//! body sees bare names, its own declarations shadow outer ones, and a later
//! block of the same namespace sees what earlier blocks exported. What the body
//! exports is then published into the enclosing checker under qualified keys
//! (`N.f`, `N.I`, `N.Inner.z`), which is how a merged reference token such as
//! `N.f` is found. Every type an exported declaration mentions is renamed to its
//! qualified key on the way out, so it resolves anywhere.
//!
//! Variable types are inferred while a body is checked, not while it is bound,
//! so a namespace is published twice: once when bound, with declared types, and
//! again when its body has been checked, with inferred ones.

use super::*;
use crate::parser::NamespaceDeclaration;

mod type_traversal;
mod values;
use type_traversal::{named_types, Qualifier};
pub(crate) use values::ExportedValue;

/// What one namespace (all its blocks) exports.
#[derive(Debug, Clone, Default)]
pub(crate) struct NamespaceMembers {
    values: BTreeSet<String>,
    types: BTreeSet<String>,
    /// Types declared but not exported; they exist under qualified keys so an
    /// exported declaration may mention them, but no reference may name them.
    hidden_types: BTreeSet<String>,
    namespaces: BTreeSet<String>,
}

/// A namespace as another module imports it: every entry keyed by the name the
/// exporting module declares it under, and the exporting module's own types the
/// entries mention, kept under keys marked with that module.
#[derive(Debug, Clone)]
pub(crate) struct NamespaceExport {
    source_name: String,
    values: BTreeMap<String, Type>,
    functions: BTreeMap<String, Vec<FunctionSignature>>,
    class_constructors: BTreeMap<String, ClassConstructorBinding>,
    types: BTreeMap<String, TypeDefinition>,
    members: BTreeMap<String, NamespaceMembers>,
    pub(in crate::checker) has_values: bool,
}

impl std::fmt::Debug for ClassConstructorBinding {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter
            .debug_struct("ClassConstructorBinding")
            .field("signatures", &self.signatures)
            .finish_non_exhaustive()
    }
}

/// The entries one body publishes, all under qualified keys.
#[derive(Default)]
struct Published {
    values: BTreeMap<String, Type>,
    functions: BTreeMap<String, Vec<FunctionSignature>>,
    class_constructors: BTreeMap<String, ClassConstructorBinding>,
    types: BTreeMap<String, TypeDefinition>,
    members: NamespaceMembers,
    pub(in crate::checker) has_values: bool,
}

struct BodyRun {
    diagnostics: Vec<Diagnostic>,
    symbols: Vec<Symbol>,
    published: Published,
    inferred_returns: BTreeMap<usize, Type>,
    inferred_parameters: BTreeMap<usize, Type>,
}

/// What a body declares, and which of it the namespace exports.
#[derive(Default)]
struct Declared {
    values: Vec<(String, bool)>,
    types: Vec<(String, bool)>,
    namespaces: Vec<(String, bool)>,
}

fn join(prefix: &str, name: &str) -> String {
    if prefix.is_empty() {
        name.to_string()
    } else {
        format!("{prefix}.{name}")
    }
}

/// Whether a body declares anything that exists at run time.
pub(super) fn body_has_values(body: &[Declaration]) -> bool {
    body.iter().any(|declaration| match declaration {
        Declaration::Variable(_)
        | Declaration::Function(_)
        | Declaration::Class(_)
        | Declaration::Enum(_)
        | Declaration::Raw(_) => true,
        Declaration::Namespace(inner) => body_has_values(&inner.body),
        _ => false,
    })
}

fn declared_in(namespace: &NamespaceDeclaration) -> Declared {
    let every = namespace.exports_every_member();
    let mut declared = Declared::default();
    for declaration in &namespace.body {
        match declaration {
            Declaration::Variable(item) => declared
                .values
                .push((item.name.clone(), item.exported || every)),
            Declaration::Function(item)
                if !declared.values.iter().any(|(name, _)| *name == item.name) =>
            {
                declared
                    .values
                    .push((item.name.clone(), item.exported || every));
            }
            Declaration::Class(item) => {
                declared
                    .values
                    .push((item.name.clone(), item.exported || every));
                declared
                    .types
                    .push((item.name.clone(), item.exported || every));
            }
            Declaration::Enum(item)
                if !declared.values.iter().any(|(name, _)| *name == item.name) =>
            {
                declared
                    .values
                    .push((item.name.clone(), item.exported || every));
                declared
                    .types
                    .push((item.name.clone(), item.exported || every));
            }
            Declaration::Interface(item) => declared
                .types
                .push((item.name.clone(), item.exported || every)),
            Declaration::TypeAlias(item) => declared
                .types
                .push((item.name.clone(), item.exported || every)),
            Declaration::Namespace(item) => declared
                .namespaces
                .push((item.name.clone(), item.exported || every)),
            _ => {}
        }
    }
    declared
}

impl ModuleChecker<'_> {
    fn policy(&self) -> CheckerPolicy {
        CheckerPolicy {
            checking: self.explicit_checking.then_some(self.checking),
            target: self.target,
            enforce_types: self.enforce_types,
            require_declared_global_calls: self.require_declared_global_calls,
            define_class_fields: self.define_class_fields,
            isolated_modules: self.isolated_modules,
            module_kind: self.module_kind,
            es_module_interop: self.es_module_interop,
            jsx: self.jsx_mode,
            experimental_decorators: self.experimental_decorators,
            jsx_factory: self.jsx_factory.clone(),
            jsx_fragment_factory: self.jsx_fragment_factory.clone(),
        }
    }

    pub(super) fn bind_namespace(&mut self, namespace: &NamespaceDeclaration) {
        if !self.namespace_name_is_free(namespace) {
            return;
        }
        let path = join(&self.namespace_path, &namespace.name);
        let run = self.run_namespace_body(namespace, &path, false);
        self.return_inference
            .results
            .borrow_mut()
            .extend(run.inferred_returns);
        self.return_inference
            .parameters
            .borrow_mut()
            .extend(run.inferred_parameters);
        self.diagnostics.extend(run.diagnostics);
        for mut symbol in run.symbols {
            symbol.name = join(&path, &symbol.name);
            symbol.exported = false;
            self.symbols.push(symbol);
        }
        self.publish_namespace(namespace, &path, run.published);
    }

    pub(super) fn check_namespace(&mut self, namespace: &NamespaceDeclaration) {
        let path = join(&self.namespace_path, &namespace.name);
        let run = self.run_namespace_body(namespace, &path, true);
        self.return_inference
            .results
            .borrow_mut()
            .extend(run.inferred_returns);
        self.return_inference
            .parameters
            .borrow_mut()
            .extend(run.inferred_parameters);
        self.diagnostics.extend(run.diagnostics);
        self.publish_namespace(namespace, &path, run.published);
    }

    /// A namespace merges with another namespace, a function, a class or an
    /// enum; any other declaration of its name in the same scope is a
    /// redeclaration.
    fn namespace_name_is_free(&mut self, namespace: &NamespaceDeclaration) -> bool {
        let name = &namespace.name;
        let path = join(&self.namespace_path, name);
        if !body_has_values(&namespace.body) {
            return true;
        }
        let Some(existing) = self.values.get(name) else {
            return true;
        };
        let mergeable = self.functions.contains_key(name)
            || self.class_constructors.contains_key(name)
            || self.enum_members.contains_key(name)
            || matches!(existing, Type::Named { name: object, .. } if *object == format!("typeof {path}"));
        if !mergeable {
            self.duplicate(name, namespace.name_span.clone());
        }
        mergeable
    }

    fn run_namespace_body(
        &self,
        namespace: &NamespaceDeclaration,
        path: &str,
        check: bool,
    ) -> BodyRun {
        let body_module = Module {
            id: self.module.id.clone(),
            source: self.module.source.clone(),
            declarations: namespace.body.clone(),
            edits: self.module.edits.clone(),
            generic_call_type_arguments: self.module.generic_call_type_arguments.clone(),
            nested_functions: self.module.nested_functions.clone(),
            type_references: self.module.type_references.clone(),
            expression_variable_types: self.module.expression_variable_types.clone(),
            type_assertions: self.module.type_assertions.clone(),
        };
        let declared = declared_in(namespace);
        let mut sub = ModuleChecker::new(
            self.project,
            &body_module,
            self.exports,
            self.ambient,
            self.namespace_exports,
            self.policy(),
            self.max_type_expansions,
        );
        sub.namespace_path = path.to_string();
        sub.pending_imports = self.pending_imports.clone();
        sub.module_namespace_imports = self.module_namespace_imports.clone();
        sub.namespaces = self.namespaces.clone();
        sub.type_only_namespaces = self.type_only_namespaces.clone();
        sub.types = self.types.clone();
        sub.values = self.values.clone();
        sub.library_values = self.library_values.clone();
        sub.functions = self.functions.clone();
        sub.class_constructors = self.class_constructors.clone();
        sub.type_only_classes = self.type_only_classes.clone();
        sub.function_implementations = self.function_implementations.clone();
        sub.enum_members = self.enum_members.clone();
        sub.const_enums = self.const_enums.clone();
        sub.type_only_enums = self.type_only_enums.clone();
        sub.ambient_const_enums = self.ambient_const_enums.clone();
        // What the body declares shadows the enclosing scope's names.
        for (name, _) in declared.values.iter().chain(&declared.types) {
            sub.forget_name(name);
        }
        for (name, _) in &declared.namespaces {
            sub.forget_name(name);
        }
        sub.expose_earlier_blocks(self, path);
        sub.bind_declarations();
        sub.infer_module_return_signatures();
        sub.validate_function_overloads();
        let skip = sub.diagnostics.len();
        if check {
            sub.check_types();
        }
        let mut diagnostics = std::mem::take(&mut sub.diagnostics);
        let symbols = if check {
            diagnostics = diagnostics.split_off(skip);
            Vec::new()
        } else {
            std::mem::take(&mut sub.symbols)
        };
        let published = sub.published_from(namespace, path, &declared);
        BodyRun {
            diagnostics,
            symbols,
            published,
            inferred_returns: sub.inferred_return_types(),
            inferred_parameters: sub.inferred_parameter_types(),
        }
    }

    /// Removes every trace of `name` so a declaration of it in this scope is
    /// not a redeclaration of the enclosing scope's.
    fn forget_name(&mut self, name: &str) {
        self.values.remove(name);
        self.library_values.remove(name);
        self.functions.remove(name);
        self.class_constructors.remove(name);
        self.type_only_classes.remove(name);
        self.function_implementations.remove(name);
        self.enum_members.remove(name);
        self.const_enums.remove(name);
        self.type_only_enums.remove(name);
        self.ambient_const_enums.remove(name);
        let nested = format!("{name}.");
        let object = format!("typeof {name}");
        let object_nested = format!("typeof {name}.");
        self.types.retain(|key, _| {
            key != name
                && key != &object
                && !key.starts_with(&nested)
                && !key.starts_with(&object_nested)
        });
        self.values.retain(|key, _| !key.starts_with(&nested));
        self.functions.retain(|key, _| !key.starts_with(&nested));
    }

    /// Makes what earlier blocks of the namespace exported visible by its bare
    /// name, as it is inside a later block.
    fn expose_earlier_blocks(&mut self, parent: &ModuleChecker<'_>, path: &str) {
        let Some(members) = parent.namespaces.get(path) else {
            return;
        };
        let visible: BTreeSet<&String> = members
            .values
            .iter()
            .chain(&members.types)
            .chain(&members.namespaces)
            .collect();
        let prefix = format!("{path}.");
        let object_prefix = format!("typeof {path}.");
        let root = |bare: &str| bare.split('.').next().unwrap_or(bare).to_string();
        for (key, value) in &parent.values {
            if let Some(bare) = key.strip_prefix(&prefix) {
                if visible.contains(&root(bare)) {
                    self.values.insert(bare.to_string(), value.clone());
                }
            }
        }
        for (key, signatures) in &parent.functions {
            if let Some(bare) = key.strip_prefix(&prefix) {
                if visible.contains(&root(bare)) {
                    self.functions.insert(bare.to_string(), signatures.clone());
                }
            }
        }
        for (key, binding) in &parent.class_constructors {
            if let Some(bare) = key.strip_prefix(&prefix) {
                if visible.contains(&root(bare)) {
                    self.class_constructors
                        .insert(bare.to_string(), binding.clone());
                }
            }
        }
        for (key, definition) in &parent.types {
            let bare = key.strip_prefix(&prefix).map(str::to_string).or_else(|| {
                key.strip_prefix(&object_prefix)
                    .map(|rest| format!("typeof {rest}"))
            });
            if let Some(bare) = bare {
                let first = bare.strip_prefix("typeof ").unwrap_or(&bare);
                if visible.contains(&root(first)) {
                    self.types.insert(bare, definition.clone());
                }
            }
        }
    }

    /// The entries a checked or bound body exports, under qualified keys.
    fn published_from(
        &self,
        namespace: &NamespaceDeclaration,
        path: &str,
        declared: &Declared,
    ) -> Published {
        let mut rename: BTreeMap<String, String> = BTreeMap::new();
        let prefix = format!("{path}.");
        for (name, _) in &declared.types {
            let object = format!("typeof {name}");
            let nested = format!("{name}.");
            for key in self.types.keys() {
                if key == name || key.starts_with(&nested) {
                    rename.insert(key.clone(), format!("{path}.{key}"));
                } else if *key == object {
                    rename.insert(key.clone(), format!("typeof {path}.{name}"));
                }
            }
        }
        // A type named through an inner namespace, `Inner.T`, is `path.Inner.T`.
        for key in self.types.keys() {
            if let Some(relative) = key.strip_prefix(&prefix) {
                rename.insert(relative.to_string(), key.clone());
            }
        }
        let qualify = Qualifier { rename: &rename };
        let mut published = Published {
            has_values: body_has_values(&namespace.body),
            ..Published::default()
        };
        for (name, exported) in &declared.values {
            if !*exported {
                continue;
            }
            published.members.values.insert(name.clone());
            let key = format!("{path}.{name}");
            if let Some(value) = self.values.get(name) {
                published.values.insert(key.clone(), qualify.ty(value));
            }
            if let Some(signatures) = self.functions.get(name) {
                published
                    .functions
                    .insert(key.clone(), qualify.signatures(signatures));
            }
            if let Some(binding) = self.class_constructors.get(name) {
                published.class_constructors.insert(
                    key,
                    ClassConstructorBinding {
                        signatures: qualify.signatures(&binding.signatures),
                        ..binding.clone()
                    },
                );
            }
        }
        for (name, exported) in &declared.types {
            if *exported {
                published.members.types.insert(name.clone());
            } else {
                published.members.hidden_types.insert(name.clone());
            }
            let object = format!("typeof {name}");
            let nested = format!("{name}.");
            for (key, definition) in &self.types {
                if key == name || key.starts_with(&nested) || *key == object {
                    let published_key = rename.get(key).cloned().unwrap_or_else(|| key.clone());
                    published
                        .types
                        .insert(published_key, qualify.definition(definition));
                }
            }
        }
        // What inner namespaces published is already keyed from the root.
        for (name, exported) in &declared.namespaces {
            if !*exported {
                continue;
            }
            published.members.namespaces.insert(name.clone());
            let inner = format!("{path}.{name}");
            let inner_nested = format!("{inner}.");
            let inner_object = format!("typeof {inner}");
            let inner_object_nested = format!("typeof {inner}.");
            let belongs = |key: &str| {
                key == inner
                    || key.starts_with(&inner_nested)
                    || key == inner_object
                    || key.starts_with(&inner_object_nested)
            };
            // A type the inner namespace named from this one's scope is bare
            // there and qualified here.
            for (key, value) in &self.values {
                if belongs(key) {
                    published.values.insert(key.clone(), qualify.ty(value));
                }
            }
            for (key, signatures) in &self.functions {
                if belongs(key) {
                    published
                        .functions
                        .insert(key.clone(), qualify.signatures(signatures));
                }
            }
            for (key, binding) in &self.class_constructors {
                if belongs(key) {
                    published.class_constructors.insert(
                        key.clone(),
                        ClassConstructorBinding {
                            signatures: qualify.signatures(&binding.signatures),
                            ..binding.clone()
                        },
                    );
                }
            }
            for (key, definition) in &self.types {
                if belongs(key) {
                    published
                        .types
                        .insert(key.clone(), qualify.definition(definition));
                }
            }
        }
        published
    }

    /// Adds a published body to this scope, under full keys and, inside another
    /// namespace, under the keys relative to it as well.
    fn publish_namespace(
        &mut self,
        namespace: &NamespaceDeclaration,
        path: &str,
        published: Published,
    ) {
        let scope = if self.namespace_path.is_empty() {
            String::new()
        } else {
            format!("{}.", self.namespace_path)
        };
        let relative = |key: &str| -> Option<String> {
            key.strip_prefix(&scope)
                .map(str::to_string)
                .filter(|relative| !scope.is_empty() && relative.as_str() != key)
        };
        let relative_object = |key: &str| -> Option<String> {
            let rest = key.strip_prefix("typeof ")?;
            let inner = rest.strip_prefix(&scope)?;
            (!scope.is_empty()).then(|| format!("typeof {inner}"))
        };
        for (key, value) in published.values {
            if let Some(relative) = relative(&key) {
                self.values.insert(relative, value.clone());
            }
            self.values.insert(key, value);
        }
        for (key, signatures) in published.functions {
            if let Some(relative) = relative(&key) {
                self.functions.insert(relative, signatures.clone());
            }
            self.functions.insert(key, signatures);
        }
        for (key, binding) in published.class_constructors {
            if let Some(relative) = relative(&key) {
                self.class_constructors.insert(relative, binding.clone());
            }
            self.class_constructors.insert(key, binding);
        }
        for (key, definition) in published.types {
            if let Some(relative) = relative(&key).or_else(|| relative_object(&key)) {
                self.types.insert(relative, definition.clone());
            }
            self.types.insert(key, definition);
        }
        let members = self.namespaces.entry(path.to_string()).or_default();
        members.values.extend(published.members.values);
        members.types.extend(published.members.types);
        members.hidden_types.extend(published.members.hidden_types);
        members.namespaces.extend(published.members.namespaces);
        if published.has_values {
            self.register_namespace_object(namespace, path);
        }
    }

    /// Binds the value `N` of a namespace that has run-time members, unless the
    /// name is already a function, class or enum the namespace merged into.
    fn register_namespace_object(&mut self, namespace: &NamespaceDeclaration, path: &str) {
        let local = &namespace.name;
        let object = format!("typeof {path}");
        // A name that is already an enum, function or class keeps its own value;
        // only a namespace object this namespace bound before is replaced.
        let own_object = self
            .types
            .get(&object)
            .is_some_and(|definition| definition.kind == TypeDefinitionKind::NamespaceObject);
        if self.values.contains_key(local) && !own_object {
            return;
        }
        let members = self.namespaces.get(path).cloned().unwrap_or_default();
        let mut fields = Vec::new();
        for name in &members.values {
            let key = format!("{path}.{name}");
            let value = match (self.functions.get(&key), self.values.get(&key)) {
                (Some(signatures), _) if !signatures.is_empty() => Type::Function {
                    parameters: signatures[0].parameters.clone(),
                    result: Box::new(signatures[0].return_type.clone()),
                },
                (_, Some(value)) => value.clone(),
                _ => Type::Unknown,
            };
            fields.push(TypeField {
                name: name.clone(),
                readonly: false,
                optional: false,
                value,
                span: namespace.name_span.clone(),
            });
        }
        for name in &members.namespaces {
            let inner = format!("{path}.{name}");
            if self.values.contains_key(&inner) {
                fields.push(TypeField {
                    name: name.clone(),
                    readonly: true,
                    optional: false,
                    value: Type::Named {
                        name: format!("typeof {inner}"),
                        arguments: Vec::new(),
                    },
                    span: namespace.name_span.clone(),
                });
            }
        }
        let value = Type::Named {
            name: object.clone(),
            arguments: Vec::new(),
        };
        self.types.insert(
            object,
            TypeDefinition {
                kind: TypeDefinitionKind::NamespaceObject,
                parameters: Vec::new(),
                value: Type::Record(fields),
            },
        );
        self.values.insert(path.to_string(), value.clone());
        if local != path {
            self.values.insert(local.clone(), value);
        }
    }

    /// The namespace path and member a qualified name refers to, looking for the
    /// longest namespace that the name extends.
    fn namespace_member_of(&self, name: &str) -> Option<(&NamespaceMembers, String, String)> {
        let mut best: Option<(&String, &NamespaceMembers)> = None;
        for (path, members) in &self.namespaces {
            if name.len() > path.len()
                && name.starts_with(path.as_str())
                && name.as_bytes()[path.len()] == b'.'
                && best.is_none_or(|(known, _)| path.len() > known.len())
            {
                best = Some((path, members));
            }
        }
        let (path, members) = best?;
        let rest = &name[path.len() + 1..];
        let root = rest.split('.').next().unwrap_or(rest).to_string();
        Some((members, path.clone(), root))
    }

    /// A type named through a namespace must be one the namespace exports.
    pub(super) fn refuse_hidden_namespace_type(&mut self, name: &str, span: &SourceSpan) -> bool {
        let Some((members, path, root)) = self.namespace_member_of(name) else {
            return false;
        };
        let hidden = members.hidden_types.contains(&root)
            && !members.types.contains(&root)
            && !members.values.contains(&root);
        if hidden {
            let suggestion = members
                .types
                .iter()
                .chain(members.values.iter())
                .find(|name| name.eq_ignore_ascii_case(&root))
                .cloned();
            let (code, arguments) = if let Some(suggestion) = suggestion {
                (2724, vec![path.clone(), root.clone(), suggestion])
            } else {
                (2694, vec![path.clone(), root.clone()])
            };
            self.typescript_type_error(
                span,
                format!("namespace `{path}` has no exported member `{root}`"),
                DiagnosticCode::UnknownType,
                code,
                arguments,
            );
        }
        hidden
    }

    /// An expression may not begin with a name that only has a type meaning: a
    /// type exported by a namespace, or a namespace with no run-time members.
    pub(super) fn check_namespace_value_use(&mut self, tokens: &[Token], span: &SourceSpan) {
        let Some(first) = tokens.first() else {
            return;
        };
        if first.kind != TokenKind::Identifier || self.values.contains_key(&first.text) {
            return;
        }
        let head = first.text.split('.').next().unwrap_or(&first.text);
        if self.type_only_namespaces.contains(head) {
            self.type_error(
                span,
                format!("namespace `{head}` was imported with `import type` and cannot be used as a value"),
                DiagnosticCode::TypeMismatch,
            );
            return;
        }
        if first.text.contains('.') {
            if let Some((members, path, root)) = self.namespace_member_of(&first.text) {
                let type_only = (members.types.contains(&root)
                    || members.hidden_types.contains(&root))
                    && !members.values.contains(&root)
                    && !members.namespaces.contains(&root);
                if type_only && first.text.len() == path.len() + 1 + root.len() {
                    self.type_error(
                        span,
                        format!(
                            "`{}` only refers to a type, but is being used as a value",
                            first.text
                        ),
                        DiagnosticCode::TypeMismatch,
                    );
                }
            }
            return;
        }
        let is_namespace = self.namespaces.contains_key(&first.text)
            || self
                .namespaces
                .contains_key(&join(&self.namespace_path, &first.text));
        if is_namespace {
            self.type_error(
                span,
                format!(
                    "namespace `{}` has no run-time members and cannot be used as a value",
                    first.text
                ),
                DiagnosticCode::TypeMismatch,
            );
        }
    }

    /// The namespaces this module exports, as the modules that import it bind them.
    pub(in crate::checker) fn exported_namespaces(&self) -> BTreeMap<String, NamespaceExport> {
        let mut sources: BTreeMap<String, String> = BTreeMap::new();
        for declaration in &self.module.declarations {
            match declaration {
                Declaration::Namespace(namespace) if namespace.exported => {
                    sources.insert(namespace.name.clone(), namespace.name.clone());
                }
                Declaration::ValueExport(export) => {
                    for binding in &export.bindings {
                        let is_namespace = self.module.declarations.iter().any(|candidate| {
                            matches!(candidate, Declaration::Namespace(namespace)
                                if namespace.name == binding.local)
                        });
                        if is_namespace {
                            sources.insert(binding.exported.clone(), binding.local.clone());
                        }
                    }
                }
                _ => {}
            }
        }
        sources
            .into_iter()
            .map(|(exported, source)| (exported, self.namespace_export_of(&source)))
            .collect()
    }

    pub(super) fn namespace_export_of(&self, source: &str) -> NamespaceExport {
        let nested = format!("{source}.");
        let object = format!("typeof {source}");
        let object_nested = format!("typeof {source}.");
        let belongs = |key: &str| {
            key == source
                || key.starts_with(&nested)
                || key == object
                || key.starts_with(&object_nested)
        };
        let mut export = NamespaceExport {
            source_name: source.to_string(),
            values: self
                .values
                .iter()
                .filter(|(key, _)| belongs(key))
                .map(|(key, value)| (key.clone(), value.clone()))
                .collect(),
            functions: self
                .functions
                .iter()
                .filter(|(key, _)| belongs(key))
                .map(|(key, value)| (key.clone(), value.clone()))
                .collect(),
            class_constructors: self
                .class_constructors
                .iter()
                .filter(|(key, _)| belongs(key))
                .map(|(key, value)| (key.clone(), value.clone()))
                .collect(),
            types: self
                .types
                .iter()
                .filter(|(key, _)| belongs(key))
                .map(|(key, value)| (key.clone(), value.clone()))
                .collect(),
            members: self
                .namespaces
                .iter()
                .filter(|(path, _)| path.as_str() == source || path.starts_with(&nested))
                .map(|(path, members)| (path.clone(), members.clone()))
                .collect(),
            has_values: self.module.declarations.iter().any(|declaration| {
                matches!(declaration, Declaration::Namespace(namespace)
                    if namespace.name == source
                        && !namespace.declared
                        && body_has_values(&namespace.body))
            }),
        };
        // The module's own types the entries mention travel with them, under
        // keys that say which module they belong to.
        let local_roots: BTreeSet<String> = self
            .module
            .declarations
            .iter()
            .filter_map(|declaration| match declaration {
                Declaration::Interface(item) => Some(item.name.clone()),
                Declaration::TypeAlias(item) => Some(item.name.clone()),
                Declaration::Class(item) => Some(item.name.clone()),
                Declaration::Enum(item) => Some(item.name.clone()),
                _ => None,
            })
            .collect();
        let mut dependencies: BTreeSet<String> = BTreeSet::new();
        let mut pending: Vec<Type> = Vec::new();
        let scan = |export: &NamespaceExport, pending: &mut Vec<Type>| {
            pending.extend(export.values.values().cloned());
            for signatures in export.functions.values() {
                for signature in signatures {
                    pending.push(Type::Function {
                        parameters: signature.parameters.clone(),
                        result: Box::new(signature.return_type.clone()),
                    });
                }
            }
            for binding in export.class_constructors.values() {
                for signature in &binding.signatures {
                    pending.push(Type::Function {
                        parameters: signature.parameters.clone(),
                        result: Box::new(signature.return_type.clone()),
                    });
                }
            }
            pending.extend(
                export
                    .types
                    .values()
                    .map(|definition| definition.value.clone()),
            );
        };
        scan(&export, &mut pending);
        while let Some(value) = pending.pop() {
            let mut names = BTreeSet::new();
            named_types(&value, &mut names);
            for name in names {
                let bare = name.strip_prefix("typeof ").unwrap_or(&name);
                let root = bare.split('.').next().unwrap_or(bare);
                if !local_roots.contains(root) || belongs(bare) {
                    continue;
                }
                if dependencies.insert(root.to_string()) {
                    let root_nested = format!("{root}.");
                    let root_object = format!("typeof {root}");
                    for (key, definition) in &self.types {
                        if key == root
                            || key.starts_with(&root_nested)
                            || *key == root_object
                            || key.starts_with(&format!("typeof {root}."))
                        {
                            pending.push(definition.value.clone());
                        }
                    }
                }
            }
        }
        if dependencies.is_empty() {
            return export;
        }
        let mut rename: BTreeMap<String, String> = BTreeMap::new();
        for root in &dependencies {
            let root_nested = format!("{root}.");
            let root_object = format!("typeof {root}");
            let root_object_nested = format!("typeof {root}.");
            for key in self.types.keys() {
                if key == root
                    || key.starts_with(&root_nested)
                    || *key == root_object
                    || key.starts_with(&root_object_nested)
                {
                    rename.insert(key.clone(), format!("{key}@{}", self.module.id));
                }
            }
        }
        let qualify = Qualifier { rename: &rename };
        export.values = export
            .values
            .iter()
            .map(|(key, value)| (key.clone(), qualify.ty(value)))
            .collect();
        export.functions = export
            .functions
            .iter()
            .map(|(key, value)| (key.clone(), qualify.signatures(value)))
            .collect();
        export.class_constructors = export
            .class_constructors
            .iter()
            .map(|(key, binding)| {
                (
                    key.clone(),
                    ClassConstructorBinding {
                        signatures: qualify.signatures(&binding.signatures),
                        ..binding.clone()
                    },
                )
            })
            .collect();
        export.types = export
            .types
            .iter()
            .map(|(key, definition)| (key.clone(), qualify.definition(definition)))
            .collect();
        for (old, new) in &rename {
            if let Some(definition) = self.types.get(old) {
                export
                    .types
                    .insert(new.clone(), qualify.definition(definition));
            }
        }
        export
    }

    /// Binds a namespace imported from another module under `local`.
    pub(super) fn bind_imported_namespace(
        &mut self,
        local: &str,
        export: &NamespaceExport,
        span: &SourceSpan,
        with_value: bool,
        merged: bool,
    ) {
        self.bind_imported_surface(local, export, span, with_value, merged, true);
    }

    fn bind_imported_surface(
        &mut self,
        local: &str,
        export: &NamespaceExport,
        span: &SourceSpan,
        with_value: bool,
        merged: bool,
        namespace_object: bool,
    ) {
        let source = export.source_name.as_str();
        let mut with_value = with_value;
        if namespace_object && with_value && !export.has_values && !merged {
            self.diagnostics.push(Diagnostic::error(
                DiagnosticCode::UnsupportedSyntax,
                span.clone(),
                format!(
                    "namespace `{source}` has no run-time members; import it with `import type`"
                ),
            ));
            // Bind it as a type-only import, so this is the only diagnostic.
            with_value = false;
        }
        if !merged
            && ((with_value && self.values.contains_key(local))
                || self.types.contains_key(&format!("typeof {local}")))
        {
            self.duplicate(local, span.clone());
            return;
        }
        let rename_key = |key: &str| -> Option<String> {
            if let Some(rest) = key.strip_prefix("typeof ") {
                let tail = rest.strip_prefix(source)?;
                return (tail.is_empty() || tail.starts_with('.'))
                    .then(|| format!("typeof {local}{tail}"));
            }
            let tail = key.strip_prefix(source)?;
            (tail.is_empty() || tail.starts_with('.')).then(|| format!("{local}{tail}"))
        };
        let mut rename: BTreeMap<String, String> = BTreeMap::new();
        for key in export.types.keys() {
            if let Some(new) = rename_key(key) {
                rename.insert(key.clone(), new);
            }
        }
        let qualify = Qualifier { rename: &rename };
        if with_value {
            for (key, value) in &export.values {
                if let Some(new) = rename_key(key) {
                    if !(merged && new == local) {
                        self.values.insert(new, qualify.ty(value));
                    }
                }
            }
            for (key, signatures) in &export.functions {
                if let Some(new) = rename_key(key) {
                    if !(merged && new == local) {
                        self.functions.insert(new, qualify.signatures(signatures));
                    }
                }
            }
            for (key, binding) in &export.class_constructors {
                if let Some(new) = rename_key(key) {
                    if merged && new == local {
                        continue;
                    }
                    self.class_constructors.insert(
                        new,
                        ClassConstructorBinding {
                            signatures: qualify.signatures(&binding.signatures),
                            ..binding.clone()
                        },
                    );
                }
            }
        }
        for (key, definition) in &export.types {
            let new = rename_key(key).unwrap_or_else(|| key.clone());
            // What the merged class, enum or function already bound stays.
            if merged && self.types.contains_key(&new) {
                continue;
            }
            self.types.insert(new, qualify.definition(definition));
        }
        for (path, members) in &export.members {
            if let Some(new) = rename_key(path) {
                self.namespaces.insert(new, members.clone());
            }
        }
        if merged {
            return;
        }
        if with_value && namespace_object {
            self.values.insert(
                local.to_string(),
                Type::Named {
                    name: format!("typeof {local}"),
                    arguments: Vec::new(),
                },
            );
        } else if !with_value {
            self.type_only_namespaces.insert(local.to_string());
        }
    }
}
