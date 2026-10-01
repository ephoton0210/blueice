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

/// What one namespace (all its blocks) exports.
#[derive(Debug, Clone, Default)]
pub(in crate::checker::module) struct NamespaceMembers {
    values: BTreeSet<String>,
    types: BTreeSet<String>,
    /// Types declared but not exported; they exist under qualified keys so an
    /// exported declaration may mention them, but no reference may name them.
    hidden_types: BTreeSet<String>,
    namespaces: BTreeSet<String>,
}

/// The entries one body publishes, all under qualified keys.
#[derive(Default)]
struct Published {
    values: BTreeMap<String, Type>,
    functions: BTreeMap<String, Vec<FunctionSignature>>,
    class_constructors: BTreeMap<String, ClassConstructorBinding>,
    types: BTreeMap<String, TypeDefinition>,
    members: NamespaceMembers,
    has_values: bool,
}

struct BodyRun {
    diagnostics: Vec<Diagnostic>,
    symbols: Vec<Symbol>,
    published: Published,
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
fn body_has_values(body: &[Declaration]) -> bool {
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

/// Renames type keys to their qualified form inside a type.
struct Qualifier<'r> {
    rename: &'r BTreeMap<String, String>,
}

impl Qualifier<'_> {
    fn name(&self, name: &str) -> String {
        self.rename
            .get(name)
            .cloned()
            .unwrap_or_else(|| name.to_string())
    }

    fn ty(&self, value: &Type) -> Type {
        match value {
            Type::Named { name, arguments } => Type::Named {
                name: self.name(name),
                arguments: arguments.iter().map(|argument| self.ty(argument)).collect(),
            },
            Type::Literal(text) => Type::Literal(self.name(text)),
            Type::Array(element) => Type::Array(Box::new(self.ty(element))),
            Type::Tuple(elements) => Type::Tuple(
                elements
                    .iter()
                    .map(|element| TupleTypeElement {
                        annotation: self.ty(&element.annotation),
                        ..element.clone()
                    })
                    .collect(),
            ),
            Type::Record(fields) => Type::Record(
                fields
                    .iter()
                    .map(|field| TypeField {
                        value: self.ty(&field.value),
                        ..field.clone()
                    })
                    .collect(),
            ),
            Type::Function { parameters, result } => Type::Function {
                parameters: self.parameters(parameters),
                result: Box::new(self.ty(result)),
            },
            Type::Union(options) => {
                Type::Union(options.iter().map(|option| self.ty(option)).collect())
            }
            Type::Intersection(options) => {
                Type::Intersection(options.iter().map(|option| self.ty(option)).collect())
            }
            other => other.clone(),
        }
    }

    fn parameters(&self, parameters: &[Parameter]) -> Vec<Parameter> {
        parameters
            .iter()
            .map(|parameter| Parameter {
                annotation: parameter.annotation.as_ref().map(|value| self.ty(value)),
                ..parameter.clone()
            })
            .collect()
    }

    fn type_parameters(&self, parameters: &[TypeParameter]) -> Vec<TypeParameter> {
        parameters
            .iter()
            .map(|parameter| TypeParameter {
                constraint: parameter.constraint.as_ref().map(|value| self.ty(value)),
                default: parameter.default.as_ref().map(|value| self.ty(value)),
                ..parameter.clone()
            })
            .collect()
    }

    fn signatures(&self, signatures: &[FunctionSignature]) -> Vec<FunctionSignature> {
        signatures
            .iter()
            .map(|signature| FunctionSignature {
                parameters: self.parameters(&signature.parameters),
                type_parameters: self.type_parameters(&signature.type_parameters),
                return_type: self.ty(&signature.return_type),
            })
            .collect()
    }

    fn definition(&self, definition: &TypeDefinition) -> TypeDefinition {
        TypeDefinition {
            kind: definition.kind,
            parameters: self.type_parameters(&definition.parameters),
            value: self.ty(&definition.value),
        }
    }
}

impl ModuleChecker<'_> {
    fn policy(&self) -> CheckerPolicy {
        CheckerPolicy {
            enforce_types: self.enforce_types,
            require_declared_global_calls: self.require_declared_global_calls,
            define_class_fields: self.define_class_fields,
            isolated_modules: self.isolated_modules,
        }
    }

    pub(super) fn bind_namespace(&mut self, namespace: &NamespaceDeclaration) {
        if !self.namespace_name_is_free(namespace) {
            return;
        }
        let path = join(&self.namespace_path, &namespace.name);
        let run = self.run_namespace_body(namespace, &path, false);
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
        };
        let declared = declared_in(namespace);
        let mut sub = ModuleChecker::new(
            self.project,
            &body_module,
            self.exports,
            self.ambient,
            self.policy(),
            self.max_type_expansions,
        );
        sub.namespace_path = path.to_string();
        sub.namespaces = self.namespaces.clone();
        sub.types = self.types.clone();
        sub.values = self.values.clone();
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
        }
    }

    /// Removes every trace of `name` so a declaration of it in this scope is
    /// not a redeclaration of the enclosing scope's.
    fn forget_name(&mut self, name: &str) {
        self.values.remove(name);
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
            self.type_error(
                span,
                format!("namespace `{path}` has no exported member `{root}`"),
                DiagnosticCode::UnknownType,
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
}
