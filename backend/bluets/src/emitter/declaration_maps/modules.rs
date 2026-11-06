// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

//! Module nodes map bindings and specifiers, not synthesized punctuation.
use super::*;
use crate::parser::{ImportDeclaration, ValueExportDeclaration};

impl Mapper<'_> {
    pub(super) fn import(
        &mut self,
        g: &ImportDeclaration,
        s: &ImportDeclaration,
        gt: &[Token],
        st: &[Token],
    ) {
        self.range(&g.span, &s.span);
        self.range(&g.specifier_span, &s.specifier_span);
        if g.equals_require {
            if g.exported {
                self.modifiers(
                    &gt[..gt.iter().position(|t| t.is("import")).unwrap_or(0)],
                    st,
                );
            }
            if let Some((g, s)) = gt
                .iter()
                .find(|t| t.is("require"))
                .zip(st.iter().find(|t| t.is("require")))
            {
                self.point(g.start, s.start);
            }
            if let Some((g, s)) = gt
                .iter()
                .rfind(|t| t.is(")"))
                .zip(st.iter().rfind(|t| t.is(")")))
            {
                self.point(g.end, s.end);
            }
        } else if let Some((g, s)) = gt.get(1).zip(st.get(1)) {
            self.point(g.start, s.start);
        }
        self.named_clause(gt, st);
        for binding in &g.bindings {
            if binding.type_only && !g.type_only {
                if let Some(original) = s
                    .bindings
                    .iter()
                    .find(|original| original.local == binding.local)
                {
                    self.point(binding.span.start, original.span.start);
                }
            }
            if binding.imported != "*" {
                self.named(gt, st, &binding.imported);
            }
            if binding.local != binding.imported {
                self.named(gt, st, &binding.local);
            }
        }
    }

    pub(super) fn value_export(
        &mut self,
        g: &ValueExportDeclaration,
        s: &ValueExportDeclaration,
        gt: &[Token],
        st: &[Token],
    ) {
        self.range(&g.span, &s.span);
        if let Some((g, s)) = g.specifier_span.as_ref().zip(s.specifier_span.as_ref()) {
            self.range(g, s);
        }
        self.named_clause(gt, st);
        for binding in &g.bindings {
            self.named(gt, st, &binding.local);
            if binding.exported != binding.local {
                self.named(gt, st, &binding.exported);
            }
            if let Some(original) = s.bindings.iter().find(|original| {
                original.local == binding.local && original.exported == binding.exported
            }) {
                let generated = gt
                    .iter()
                    .filter(|token| {
                        token.start >= binding.span.start && token.end <= binding.span.end
                    })
                    .collect::<Vec<_>>();
                let source = st
                    .iter()
                    .filter(|token| {
                        token.start >= original.span.start && token.end <= original.span.end
                    })
                    .collect::<Vec<_>>();
                for (g, s) in generated
                    .first()
                    .zip(source.first())
                    .into_iter()
                    .chain(generated.last().zip(source.last()))
                {
                    if g.kind == TokenKind::String && s.kind == TokenKind::String {
                        self.pair(g, s);
                    }
                }
            }
        }
        if let Some(namespace) = &g.namespace {
            self.named(gt, st, namespace);
        }
    }

    fn named_clause(&mut self, g: &[Token], s: &[Token]) {
        if let Some((g, s)) = g
            .iter()
            .find(|t| t.is("{"))
            .zip(s.iter().find(|t| t.is("{")))
        {
            self.point(g.start, s.start);
        }
        if let Some((g, s)) = g
            .iter()
            .find(|t| t.is("}"))
            .zip(s.iter().find(|t| t.is("}")))
        {
            self.point(g.end, s.end);
        }
    }
}
