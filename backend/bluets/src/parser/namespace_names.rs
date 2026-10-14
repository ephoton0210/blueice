// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

//! Qualified namespace references as single tokens.
//!
//! A reference such as `N.f`, `Outer.Inner.x` or, inside a namespace, `Inner.z`
//! reads a member of a declared namespace. The checker keys those members by
//! their dotted name, so a second parse pass merges each such chain into one
//! identifier token spelled `N.f`. Its span still covers every source byte, so
//! emission, which copies source text, is unchanged. Only a chain whose first
//! name is a namespace in scope and whose every next name is an exported member
//! of the namespace before it is merged: `K.s` stays three tokens when `K` is a
//! class merged with a namespace that has no `s`, and the checker reads it as
//! the static member it is.

use super::*;
use std::collections::BTreeSet;

#[derive(Debug, Default)]
struct Members {
    /// Every exported member name.
    all: BTreeSet<String>,
    /// The exported members that are namespaces.
    namespaces: BTreeSet<String>,
}

/// What the first parse learned about the module's namespaces.
#[derive(Debug, Default)]
pub(super) struct NamespaceNames {
    /// Members by namespace path; `""` is the module itself, whose namespaces
    /// are all in scope whether or not they are exported.
    members: BTreeMap<String, Members>,
    /// Each namespace body's byte range and path, in declaration order.
    bodies: Vec<(usize, usize, String)>,
    /// Each namespace header's byte range, which holds names that declare.
    headers: Vec<(usize, usize)>,
}

fn join(prefix: &str, name: &str) -> String {
    if prefix.is_empty() {
        name.to_string()
    } else {
        format!("{prefix}.{name}")
    }
}

fn declared_name(declaration: &Declaration) -> Option<&str> {
    match declaration {
        Declaration::Variable(item) => Some(&item.name),
        Declaration::Function(item) => Some(&item.name),
        Declaration::Class(item) => Some(&item.name),
        Declaration::Enum(item) => Some(&item.name),
        Declaration::Interface(item) => Some(&item.name),
        Declaration::TypeAlias(item) => Some(&item.name),
        Declaration::Namespace(item) => Some(&item.name),
        _ => None,
    }
}

/// The members a namespace exports, as another module sees them, by name; the
/// inner namespaces nest.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct NamespaceTree {
    /// Every exported member name.
    pub members: BTreeSet<String>,
    /// The exported members that are namespaces, with theirs.
    pub namespaces: BTreeMap<String, NamespaceTree>,
}

/// The namespaces a module exports, by the name they are exported under: every
/// block of one namespace contributes to it.
pub fn exported_namespace_trees(module: &Module) -> BTreeMap<String, NamespaceTree> {
    fn tree_of(blocks: &[&NamespaceDeclaration]) -> NamespaceTree {
        let mut tree = NamespaceTree::default();
        for block in blocks {
            let every = block.exports_every_member();
            for inner in &block.body {
                let Some(name) = declared_name(inner) else {
                    continue;
                };
                let exported = every
                    || match inner {
                        Declaration::Variable(item) => item.exported,
                        Declaration::Function(item) => item.exported,
                        Declaration::Class(item) => item.exported,
                        Declaration::Enum(item) => item.exported,
                        Declaration::Interface(item) => item.exported,
                        Declaration::TypeAlias(item) => item.exported,
                        Declaration::Namespace(item) => item.exported,
                        _ => false,
                    };
                if exported {
                    tree.members.insert(name.to_string());
                }
            }
        }
        let mut inner_names: BTreeSet<&str> = BTreeSet::new();
        for block in blocks {
            for inner in &block.body {
                if let Declaration::Namespace(item) = inner {
                    if item.exported || block.exports_every_member() {
                        inner_names.insert(&item.name);
                    }
                }
            }
        }
        for name in inner_names {
            let inner_blocks: Vec<&NamespaceDeclaration> = blocks
                .iter()
                .flat_map(|block| block.body.iter())
                .filter_map(|declaration| match declaration {
                    Declaration::Namespace(item) if item.name == name => Some(item),
                    _ => None,
                })
                .collect();
            tree.namespaces
                .insert(name.to_string(), tree_of(&inner_blocks));
        }
        tree
    }
    let exported_by_name: BTreeMap<&str, &str> = module
        .declarations
        .iter()
        .filter_map(|declaration| match declaration {
            Declaration::ValueExport(export) => Some(export.bindings.iter()),
            _ => None,
        })
        .flatten()
        .map(|binding| (binding.local.as_str(), binding.exported.as_str()))
        .collect();
    let mut by_export: BTreeMap<String, Vec<&NamespaceDeclaration>> = BTreeMap::new();
    for declaration in &module.declarations {
        let Declaration::Namespace(namespace) = declaration else {
            continue;
        };
        if namespace.exported {
            by_export
                .entry(namespace.name.clone())
                .or_default()
                .push(namespace);
        } else if let Some(exported) = exported_by_name.get(namespace.name.as_str()) {
            by_export
                .entry((*exported).to_string())
                .or_default()
                .push(namespace);
        }
    }
    by_export
        .into_iter()
        .map(|(name, blocks)| (name, tree_of(&blocks)))
        .collect()
}

impl NamespaceNames {
    /// `None` when the module declares no namespace and imports none.
    pub(super) fn collect(
        declarations: &[Declaration],
        imported: &[(String, NamespaceTree)],
    ) -> Option<Self> {
        let mut names = Self::default();
        names.visit("", declarations, true);
        for (local, tree) in imported {
            names.add_imported(local, tree);
        }
        (!names.bodies.is_empty() || !imported.is_empty()).then_some(names)
    }

    /// Adds a namespace imported under `local`, in scope everywhere.
    fn add_imported(&mut self, local: &str, tree: &NamespaceTree) {
        let root = self.members.entry(String::new()).or_default();
        root.all.insert(local.to_string());
        root.namespaces.insert(local.to_string());
        self.add_tree(local, tree);
    }

    fn add_tree(&mut self, path: &str, tree: &NamespaceTree) {
        let members = self.members.entry(path.to_string()).or_default();
        members.all.extend(tree.members.iter().cloned());
        members.all.extend(tree.namespaces.keys().cloned());
        members.namespaces.extend(tree.namespaces.keys().cloned());
        for (name, inner) in &tree.namespaces {
            self.add_tree(&join(path, name), inner);
        }
    }

    fn visit(&mut self, scope: &str, declarations: &[Declaration], whole_scope: bool) {
        for declaration in declarations {
            let Declaration::Namespace(namespace) = declaration else {
                continue;
            };
            let path = join(scope, &namespace.name);
            if whole_scope || namespace.exported {
                let members = self.members.entry(scope.to_string()).or_default();
                members.all.insert(namespace.name.clone());
                members.namespaces.insert(namespace.name.clone());
            }
            self.members.entry(path.clone()).or_default();
            if !namespace.implicit {
                self.bodies.push((
                    namespace.header_span.end,
                    namespace.closing_span.start,
                    path.clone(),
                ));
                self.headers
                    .push((namespace.header_span.start, namespace.header_span.end));
            } else {
                // A dotted segment's body is its child's, inside the same
                // braces as its parent's.
                self.bodies.push((
                    namespace.header_span.end,
                    namespace.closing_span.start,
                    path.clone(),
                ));
            }
            let every = namespace.exports_every_member();
            for inner in &namespace.body {
                let Some(name) = declared_name(inner) else {
                    continue;
                };
                let exported = every
                    || match inner {
                        Declaration::Variable(item) => item.exported,
                        Declaration::Function(item) => item.exported,
                        Declaration::Class(item) => item.exported,
                        Declaration::Enum(item) => item.exported,
                        Declaration::Interface(item) => item.exported,
                        Declaration::TypeAlias(item) => item.exported,
                        Declaration::Namespace(item) => item.exported,
                        _ => false,
                    };
                if exported && !matches!(inner, Declaration::Namespace(_)) {
                    self.members
                        .entry(path.clone())
                        .or_default()
                        .all
                        .insert(name.to_string());
                }
            }
            self.visit(&path, &namespace.body, false);
        }
    }

    /// The namespace paths whose bodies contain `offset`, innermost first,
    /// then the module itself.
    fn scopes_at(&self, offset: usize) -> Vec<&str> {
        let mut enclosing: Vec<&(usize, usize, String)> = self
            .bodies
            .iter()
            .filter(|(start, end, _)| *start <= offset && offset < *end)
            .collect();
        // A longer path is nested deeper.
        enclosing.sort_by_key(|(_, _, path)| std::cmp::Reverse(path.matches('.').count()));
        let mut scopes: Vec<&str> = enclosing.iter().map(|(_, _, path)| path.as_str()).collect();
        scopes.push("");
        scopes
    }

    fn in_header(&self, offset: usize) -> bool {
        self.headers
            .iter()
            .any(|(start, end)| *start <= offset && offset < *end)
    }

    /// Merges every qualified reference chain in `tokens` into one token.
    pub(super) fn merge(&self, tokens: Vec<Token>) -> Vec<Token> {
        let mut merged: Vec<Token> = Vec::with_capacity(tokens.len());
        let mut index = 0;
        while index < tokens.len() {
            let token = &tokens[index];
            let after_dot = merged.last().is_some_and(|before| before.is("."));
            if token.kind != TokenKind::Identifier
                || after_dot
                || self.in_header(token.start)
                || !tokens.get(index + 1).is_some_and(|dot| dot.is("."))
            {
                merged.push(token.clone());
                index += 1;
                continue;
            }
            let scope = self.scopes_at(token.start).into_iter().find(|scope| {
                self.members
                    .get(*scope)
                    .is_some_and(|members| members.namespaces.contains(&token.text))
            });
            let Some(scope) = scope else {
                merged.push(token.clone());
                index += 1;
                continue;
            };
            let mut text = token.text.clone();
            let mut path = join(scope, &token.text);
            let mut next = index + 1;
            while tokens.get(next).is_some_and(|dot| dot.is("."))
                && self.members.contains_key(&path)
            {
                let Some(member) = tokens.get(next + 1) else {
                    break;
                };
                if !matches!(member.kind, TokenKind::Identifier | TokenKind::Keyword)
                    || !self.members[&path].all.contains(&member.text)
                {
                    break;
                }
                text.push('.');
                text.push_str(&member.text);
                path = join(&path, &member.text);
                next += 2;
            }
            if next == index + 1 {
                merged.push(token.clone());
                index += 1;
                continue;
            }
            merged.push(Token {
                kind: TokenKind::Identifier,
                text,
                start: token.start,
                end: tokens[next - 1].end,
            });
            index = next;
        }
        merged
    }
}
