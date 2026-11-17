// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

//! Static helper requirements retain source syntax rather than generated text.

use super::*;
use crate::parser::{BindingPattern, ClassDeclaration, VariableDeclaration};
use crate::syntax::{Token, TokenKind};
mod declarations;
mod expressions;

struct Requirements<'a> {
    module: &'a Module,
    options: &'a CompilerOptions,
    tokens: Vec<Token>,
    requests: Vec<(&'static str, SourceSpan)>,
    binding_ranges: Vec<(usize, usize)>,
}

pub(super) fn validate(
    module: &Module,
    project: &Project,
    options: &CompilerOptions,
) -> Vec<Diagnostic> {
    if !options.import_helpers || !project.is_external_module(module) {
        return Vec::new();
    }
    let mut requirements = Requirements {
        module,
        options,
        tokens: crate::syntax::lex_with_limits(
            &module.id,
            &module.source,
            options.limits.parser.max_source_bytes,
            options.limits.parser.max_tokens,
        )
        .unwrap_or_default(),
        requests: Vec::new(),
        binding_ranges: Vec::new(),
    };
    requirements.declarations();
    requirements.expressions();
    requirements.requests.sort_by_key(|(_, span)| span.start);
    let mut checked = BTreeSet::new();
    let mut diagnostics = Vec::new();
    for (helper, span) in requirements.requests {
        if !checked.insert(helper) {
            continue;
        }
        if let Err(diagnostic) = super::validate_provider(module, project, helper, span) {
            let absent = diagnostic
                .typescript
                .as_ref()
                .is_some_and(|row| row.code == 2354);
            diagnostics.push(diagnostic);
            // A missing provider is diagnosed once, at its first requirement.
            if absent {
                break;
            }
        }
    }
    diagnostics
}

impl Requirements<'_> {
    fn add(&mut self, helper: &'static str, span: SourceSpan) {
        self.requests.push((helper, span));
    }

    fn function(&mut self, asynchronous: bool, generator: bool, span: SourceSpan) {
        if asynchronous && generator && self.options.target < crate::EcmaTarget::Es2018 {
            self.add("__asyncGenerator", span.clone());
            self.add("__await", span.clone());
        } else if asynchronous && self.options.target < crate::EcmaTarget::Es2017 {
            self.add("__awaiter", span.clone());
        }
        if self.options.target < crate::EcmaTarget::Es2015 && (generator || asynchronous) {
            self.add("__generator", span);
        }
    }

    fn variable(&mut self, variable: &VariableDeclaration) {
        if variable.declared {
            return;
        }
        let Some(pattern) = &variable.pattern else {
            return;
        };
        let end = variable
            .initializer
            .first()
            .map_or(variable.span.end, |token| token.start);
        self.binding_ranges.push((variable.span.start, end));
        if matches!(pattern.pattern, BindingPattern::Object(_))
            && self.options.target < crate::EcmaTarget::Es2018
        {
            if let Some(rest) = &pattern.rest {
                let span = self
                    .tokens
                    .iter()
                    .find(|token| {
                        token.start >= rest.span.start
                            && token.end <= rest.span.end
                            && token.text == rest.name
                    })
                    .map_or_else(|| rest.span.clone(), |token| token.span(&self.module.id));
                self.add("__rest", span);
            }
        }
        if let BindingPattern::Array(elements) = &pattern.pattern {
            if self.options.target < crate::EcmaTarget::Es2015 && self.options.downlevel_iteration {
                if let Some(element) = elements.iter().flatten().next() {
                    self.add("__read", element.span.clone());
                }
            }
        }
    }
}
