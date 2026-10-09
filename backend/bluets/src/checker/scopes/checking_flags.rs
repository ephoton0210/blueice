// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

//! Optional diagnostics resolve uses to lexical binding identities.

use super::*;

impl ScopeModel<'_> {
    pub(in crate::checker) fn checking_diagnostics(
        &self,
        options: crate::CheckingOptions,
    ) -> Vec<Diagnostic> {
        let mut diagnostics = Vec::new();
        let used = self
            .references
            .iter()
            .filter_map(|reference| {
                self.resolve(reference.scope, &reference.name, reference.meaning)
                    .map(|(scope, _)| (scope, reference.name.clone()))
            })
            .collect::<BTreeSet<_>>();
        let external = self.module.declarations.iter().any(|d| {
            matches!(
                d,
                Declaration::Import(_)
                    | Declaration::TypeExport(_)
                    | Declaration::DefaultExport(_)
                    | Declaration::ValueExport(_)
            ) || match d {
                Declaration::Variable(d) => d.exported,
                Declaration::Function(d) => d.exported,
                Declaration::Class(d) => d.exported,
                Declaration::Interface(d) => d.exported,
                Declaration::TypeAlias(d) => d.exported,
                Declaration::Enum(d) => d.exported,
                Declaration::Namespace(d) => d.exported,
                _ => false,
            }
        });
        for ((scope, name), span) in &self.parameters {
            if options.no_unused_parameters
                && !name.starts_with('_')
                && name != "this"
                && !used.contains(&(*scope, name.clone()))
            {
                diagnostics.push(Diagnostic::error(
                    DiagnosticCode::TypeMismatch,
                    span.clone(),
                    format!("parameter `{name}` is declared but never read"),
                ));
            }
            if (options.always_strict || external || self.scopes[*scope].class_owner.is_some())
                && matches!(name.as_str(), "eval" | "arguments")
            {
                diagnostics.push(Diagnostic::error(
                    DiagnosticCode::TypeMismatch,
                    span.clone(),
                    format!("invalid strict-mode binding `{name}`"),
                ));
            }
        }
        if options.no_unused_locals {
            for ((scope, name), (span, exported)) in &self.type_declarations {
                if (*scope != 0 || external) && !exported && !used.contains(&(*scope, name.clone()))
                {
                    diagnostics.push(Diagnostic::error(
                        DiagnosticCode::TypeMismatch,
                        span.clone(),
                        format!("type `{name}` is declared but never used"),
                    ));
                }
            }
        }
        for token in self
            .tokens
            .iter()
            .filter(|t| self.declaration_names.contains(&t.start))
        {
            let scope = self.scope_at(token.start);
            let Some((owner, Some(binding))) = self.resolve(scope, &token.text, Meaning::Value)
            else {
                continue;
            };
            if self.parameters.contains_key(&(owner, token.text.clone()))
                || self
                    .catch_bindings
                    .contains_key(&(owner, token.text.clone()))
            {
                continue;
            }
            if (options.always_strict || external || self.scopes[scope].class_owner.is_some())
                && matches!(token.text.as_str(), "eval" | "arguments")
            {
                diagnostics.push(Diagnostic::error(
                    DiagnosticCode::TypeMismatch,
                    SourceSpan::new(&self.module.id, token.start, token.end),
                    format!("invalid strict-mode binding `{}`", token.text),
                ));
            }
            if options.no_unused_locals
                && !binding.library
                && (owner != 0 || external)
                && !used.contains(&(owner, token.text.clone()))
                && !self.exported(&token.text, owner)
            {
                diagnostics.push(Diagnostic::error(
                    DiagnosticCode::TypeMismatch,
                    SourceSpan::new(&self.module.id, token.start, token.end),
                    format!("`{}` is declared but never read", token.text),
                ));
            }
        }
        self.catch_diagnostics(options, &mut diagnostics);
        if options.no_fallthrough_cases_in_switch {
            self.fallthrough_diagnostics(&mut diagnostics);
        }
        diagnostics
    }

    fn exported(&self, name: &str, scope: ScopeId) -> bool {
        if scope != 0 {
            return self.scopes[scope].ambient_exports;
        }
        self.module
            .declarations
            .iter()
            .any(|declaration| match declaration {
                Declaration::Variable(d) => d.exported && d.name == name,
                Declaration::Function(d) => d.exported && d.name == name,
                Declaration::Class(d) => d.exported && d.name == name,
                Declaration::Enum(d) => d.exported && d.name == name,
                Declaration::Namespace(d) => d.exported && d.name == name,
                Declaration::Interface(d) => d.exported && d.name == name,
                Declaration::TypeAlias(d) => d.exported && d.name == name,
                Declaration::ValueExport(d) => d.bindings.iter().any(|b| b.local == name),
                Declaration::DefaultExport(d) => d.name == name,
                Declaration::TypeExport(d) => {
                    d.specifier.is_none() && d.bindings.iter().any(|b| b.local == name)
                }
                _ => false,
            })
    }

    fn catch_diagnostics(
        &self,
        options: crate::CheckingOptions,
        diagnostics: &mut Vec<Diagnostic>,
    ) {
        for (scope, bindings) in self.scopes.iter().enumerate() {
            for binding in bindings.values.values() {
                let Some((start, end)) = binding.initializer else {
                    continue;
                };
                let tokens = self
                    .tokens
                    .iter()
                    .filter(|t| t.start >= start && t.end <= end && !t.is(";"))
                    .collect::<Vec<_>>();
                let [token] = tokens.as_slice() else { continue };
                let Some((owner, _)) = self.resolve(scope, &token.text, Meaning::Value) else {
                    continue;
                };
                if self
                    .catch_bindings
                    .get(&(owner, token.text.clone()))
                    .is_some_and(|explicit| *explicit || options.use_unknown_in_catch_variables)
                    && !matches!(binding.declared_type, Type::Unknown | Type::Any)
                {
                    diagnostics.push(Diagnostic::error(
                        DiagnosticCode::TypeMismatch,
                        SourceSpan::new(&self.module.id, token.start, token.end),
                        "catch variable of type `unknown` is not assignable to the declared type",
                    ).with_typescript(2322,vec!["unknown".into(),crate::diagnostic::type_text::render(&binding.declared_type)]));
                }
            }
        }
    }

    fn fallthrough_diagnostics(&self, diagnostics: &mut Vec<Diagnostic>) {
        let tokens = &self.tokens;
        for (index, token) in tokens.iter().enumerate().filter(|(_, t)| t.is("switch")) {
            let Some(open) = (index + 1..tokens.len()).find(|i| tokens[*i].is("{")) else {
                continue;
            };
            let Some(end) = expressions::matching_end(tokens, open, "{", "}") else {
                continue;
            };
            let mut depth = 0usize;
            let mut labels = Vec::new();
            for (offset, t) in tokens[open + 1..end].iter().enumerate() {
                if depth == 0 && (t.is("case") || t.is("default")) {
                    labels.push(open + 1 + offset);
                }
                match t.text.as_str() {
                    "{" | "(" | "[" => depth += 1,
                    "}" | ")" | "]" => depth = depth.saturating_sub(1),
                    _ => {}
                }
            }
            for pair in labels.windows(2) {
                let Some(colon) = (pair[0] + 1..pair[1]).find(|i| tokens[*i].is(":")) else {
                    continue;
                };
                let body = &tokens[colon + 1..pair[1]];
                if body.is_empty() {
                    continue;
                }
                let terminates = switches::terminates(body, self.max_type_expansions);
                if !terminates {
                    diagnostics.push(Diagnostic::error(
                        DiagnosticCode::TypeMismatch,
                        SourceSpan::new(
                            &self.module.id,
                            tokens[pair[0]].start,
                            tokens[pair[0]].end,
                        ),
                        "fallthrough case in switch",
                    ));
                }
            }
            let _ = token;
        }
    }
}
