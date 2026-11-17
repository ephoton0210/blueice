// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

//! Cross-file declaration-space conflicts keep both original source owners.

use super::*;

#[derive(Clone, Copy, PartialEq, Eq)]
enum Kind {
    Lexical,
    Var,
    Function,
    Class,
    Interface,
    Alias,
    Namespace,
}

struct Binding<'a> {
    name: &'a str,
    kind: Kind,
    span: SourceSpan,
    runtime: bool,
}
impl Binding<'_> {
    fn value(&self) -> bool {
        matches!(
            self.kind,
            Kind::Lexical | Kind::Var | Kind::Function | Kind::Class
        ) || (self.kind == Kind::Namespace && self.runtime)
    }
    fn ty(&self) -> bool {
        matches!(self.kind, Kind::Class | Kind::Interface | Kind::Alias)
    }
}

pub(in crate::checker) fn diagnose(project: &Project) -> (BTreeSet<String>, Vec<Diagnostic>) {
    let mut groups = BTreeMap::<&str, Vec<Binding<'_>>>::new();
    for body in bodies(project) {
        for declaration in body {
            let (name, kind, runtime) = match declaration {
                Declaration::Variable(item) if item.pattern.is_none() => (
                    item.name.as_str(),
                    if item.kind == crate::VariableKind::Var {
                        Kind::Var
                    } else {
                        Kind::Lexical
                    },
                    true,
                ),
                Declaration::Function(item) => (
                    item.name.as_str(),
                    Kind::Function,
                    !item.declared && !item.overload,
                ),
                Declaration::Class(item) => (
                    item.name.as_str(),
                    Kind::Class,
                    !is_declaration_module(&item.span.module),
                ),
                Declaration::Interface(item) => (item.name.as_str(), Kind::Interface, false),
                Declaration::TypeAlias(item) => (item.name.as_str(), Kind::Alias, false),
                Declaration::Namespace(item) => (
                    item.name.as_str(),
                    Kind::Namespace,
                    !item.declared
                        && !is_declaration_module(&item.span.module)
                        && !ambient::namespace_is_type_only(&item.body),
                ),
                _ => continue,
            };
            if name.is_empty() {
                continue;
            }
            groups.entry(name).or_default().push(Binding {
                name,
                kind,
                runtime,
                span: name_span(project, declaration.span(), name),
            });
        }
    }
    let mut invalid = BTreeSet::new();
    let mut diagnostics = Vec::new();
    for (name, bindings) in groups {
        let values = bindings
            .iter()
            .filter(|row| row.value())
            .collect::<Vec<_>>();
        let types = bindings.iter().filter(|row| row.ty()).collect::<Vec<_>>();
        let lexical = values.iter().any(|row| row.kind == Kind::Lexical) && values.len() > 1;
        let aliases = types.iter().any(|row| row.kind == Kind::Alias) && types.len() > 1;
        let classes = types.iter().filter(|row| row.kind == Kind::Class).count();
        let class_values = classes > 0
            && values
                .iter()
                .any(|row| matches!(row.kind, Kind::Var | Kind::Function));
        let conflicts = if lexical {
            Some((2451, values))
        } else if aliases || classes > 1 {
            Some((2300, types))
        } else if class_values {
            Some((
                2300,
                values
                    .into_iter()
                    .filter(|row| row.kind != Kind::Namespace)
                    .collect(),
            ))
        } else {
            None
        };
        if let Some((code, conflicts)) = conflicts {
            invalid.insert(name.to_string());
            // One representative per source avoids a quadratic scan of repeated
            // declarations. Same-file diagnostics remain with their own checker.
            let mut representatives = BTreeMap::<&str, &Binding<'_>>::new();
            for row in &conflicts {
                representatives.entry(&row.span.module).or_insert(row);
            }
            for row in conflicts {
                if let Some(peer) = representatives
                    .iter()
                    .find(|(module, _)| **module != row.span.module)
                    .map(|(_, row)| *row)
                {
                    diagnostics.push(conflict(code, row, Some(peer)));
                }
            }
        }
        let merged = bindings
            .iter()
            .filter(|row| matches!(row.kind, Kind::Function | Kind::Class) && row.runtime)
            .map(|row| row.span.module.as_str())
            .collect::<BTreeSet<_>>();
        for namespace in bindings
            .iter()
            .filter(|row| row.kind == Kind::Namespace && row.runtime)
        {
            if merged.iter().any(|module| *module != namespace.span.module) {
                invalid.insert(name.to_string());
                diagnostics.push(conflict(2433, namespace, None));
            }
        }
    }
    (invalid, diagnostics)
}

fn conflict(code: u32, row: &Binding<'_>, peer: Option<&Binding<'_>>) -> Diagnostic {
    let arguments = if code == 2433 {
        Vec::new()
    } else {
        vec![row.name.to_string()]
    };
    let mut diagnostic = Diagnostic::error(
        DiagnosticCode::DuplicateDeclaration,
        row.span.clone(),
        format!(
            "global declaration of `{}` conflicts across source files",
            row.name
        ),
    )
    .with_typescript(code, arguments);
    if let Some(peer) = peer {
        diagnostic
            .typescript
            .as_mut()
            .expect("Native declaration collision template")
            .related_information
            .push(crate::TypeScriptRelatedInformation {
                code: 6203,
                message: format!("'{}' was also declared here.", row.name),
                span: peer.span.clone(),
                position: None,
            });
    }
    diagnostic
}

pub(in crate::checker) fn name_span(
    project: &Project,
    span: &SourceSpan,
    name: &str,
) -> SourceSpan {
    project
        .source(&span.module)
        .and_then(|source| source.get(span.start..span.end))
        .and_then(|source| crate::syntax::lex(&span.module, source).ok())
        .and_then(|tokens| tokens.into_iter().find(|token| token.is(name)))
        .map(|token| {
            SourceSpan::new(
                &span.module,
                span.start + token.start,
                span.start + token.end,
            )
        })
        .unwrap_or_else(|| span.clone())
}
