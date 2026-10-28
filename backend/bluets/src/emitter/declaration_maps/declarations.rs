// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

//! Ordered declaration pairs preserve merges and retained private dependencies.
use super::*;

fn identity(item: &Declaration) -> Option<(u8, &str)> {
    Some(match item {
        Declaration::Variable(v) => (0, &v.name),
        Declaration::Function(f) => (1, &f.name),
        Declaration::Class(c) => (2, &c.name),
        Declaration::Interface(i) => (3, &i.name),
        Declaration::TypeAlias(a) => (4, &a.name),
        Declaration::Namespace(n) => (5, &n.name),
        Declaration::Import(i) => (6, &i.specifier),
        Declaration::DefaultExport(_) => (7, "default"),
        Declaration::ValueExport(_) => (8, "export"),
        Declaration::TypeExport(_) => (9, "type-export"),
        _ => return None,
    })
}
impl Mapper<'_> {
    pub(super) fn declarations(
        &mut self,
        s: &[Declaration],
        g: &[Declaration],
    ) -> Result<(), Diagnostic> {
        let mut used = BTreeSet::new();
        for generated in g {
            if self.exhausted {
                break;
            }
            let Some(key) = identity(generated) else {
                return Err(unsupported(generated.span()));
            };
            let Some((index, source)) = s
                .iter()
                .enumerate()
                .find(|(index, item)| !used.contains(index) && identity(item) == Some(key))
            else {
                continue;
            };
            used.insert(index);
            let gt = self.generated(generated.span());
            let st = self.source(source.span());
            self.comment(generated.span().start, source.span().start);
            match (generated, source) {
                (Declaration::Variable(g), Declaration::Variable(s)) => {
                    if st.first().is_some_and(|t| t.is("export"))
                        && st.get(1).is_some_and(|t| t.is("default"))
                    {
                        continue;
                    }
                    self.point(g.span.start, s.span.start);
                    for kind in ["const", "let", "var"] {
                        if let Some((g, s)) = gt
                            .iter()
                            .find(|t| t.is(kind))
                            .zip(st.iter().find(|t| t.is(kind)))
                        {
                            self.point(g.start, s.start);
                        }
                    }
                    self.named(&gt, &st, &g.name);
                    if s.annotation.is_some() {
                        self.ty(annotation(&gt), annotation(&st));
                    }
                    let ge = gt
                        .last()
                        .filter(|t| t.is(";"))
                        .map_or(g.span.end, |t| t.start);
                    let se = st
                        .last()
                        .filter(|t| t.is(";"))
                        .map_or(s.span.end, |t| t.start);
                    self.point(ge, se);
                    self.point(g.span.end, s.span.end);
                }
                (Declaration::Function(g), Declaration::Function(s)) => {
                    self.point(g.span.start, s.span.start);
                    self.named(&gt, &st, &g.name);
                    self.type_parameters(&g.type_parameters, &s.type_parameters);
                    self.parameters(&g.parameters, &s.parameters);
                    if s.return_type.is_some() {
                        self.return_type(&gt, &st);
                    }
                    self.point(g.span.end, s.span.end);
                }
                (Declaration::Class(g), Declaration::Class(s)) => self.class(g, s),
                (Declaration::Interface(g), Declaration::Interface(s)) => {
                    self.range(&g.span, &s.span);
                    self.modifiers(
                        &gt[..gt.iter().position(|t| t.is("interface")).unwrap_or(0)],
                        &st,
                    );
                    self.named(&gt, &st, &g.name);
                    self.type_parameters(&g.type_parameters, &s.type_parameters);
                    for (gf, sf) in g.fields.iter().zip(&s.fields) {
                        let g = self.field_tokens(&gf.span, true);
                        let s = self.field_tokens(&sf.span, false);
                        self.ty(&g, &s);
                    }
                }
                (Declaration::TypeAlias(g), Declaration::TypeAlias(s)) => {
                    self.range(&g.span, &s.span);
                    self.modifiers(
                        &gt[..gt.iter().position(|t| t.is("type")).unwrap_or(0)],
                        &st,
                    );
                    self.named(&gt, &st, &g.name);
                    self.type_parameters(&g.type_parameters, &s.type_parameters);
                    if let Some((ge, se)) = top(&gt, "=").zip(top(&st, "=")) {
                        let ge_end = gt.len() - usize::from(gt.last().is_some_and(|t| t.is(";")));
                        let se_end = st.len() - usize::from(st.last().is_some_and(|t| t.is(";")));
                        self.ty(&gt[ge + 1..ge_end], &st[se + 1..se_end]);
                    }
                }
                (Declaration::Namespace(g), Declaration::Namespace(s)) => {
                    self.range(&g.span, &s.span);
                    self.pair(
                        &Token {
                            kind: TokenKind::Identifier,
                            text: g.name.clone(),
                            start: g.name_span.start,
                            end: g.name_span.end,
                        },
                        &Token {
                            kind: TokenKind::Identifier,
                            text: s.name.clone(),
                            start: s.name_span.start,
                            end: s.name_span.end,
                        },
                    );
                    if let Some((g, s)) = gt
                        .iter()
                        .find(|t| t.is("{"))
                        .zip(st.iter().find(|t| t.is("{")))
                    {
                        self.point(g.start, s.start);
                    }
                    self.declarations(&s.body, &g.body)?;
                }
                (Declaration::DefaultExport(g), Declaration::DefaultExport(s)) => {
                    self.range(&g.span, &s.span);
                    if !s.expression {
                        self.named(&gt, &st, &g.name);
                    }
                }
                (Declaration::Import(g), Declaration::Import(s)) => {
                    self.range(&g.span, &s.span);
                    self.range(&g.specifier_span, &s.specifier_span);
                    if let Some((g, s)) = gt.get(1).zip(st.get(1)) {
                        self.point(g.start, s.start);
                    }
                    if let Some((g, s)) = gt
                        .iter()
                        .find(|t| t.is("{"))
                        .zip(st.iter().find(|t| t.is("{")))
                    {
                        self.point(g.start, s.start);
                    }
                    if let Some((g, s)) = gt
                        .iter()
                        .find(|t| t.is("}"))
                        .zip(st.iter().find(|t| t.is("}")))
                    {
                        self.point(g.end, s.end);
                    }
                    for binding in &g.bindings {
                        self.named(&gt, &st, &binding.imported);
                        if binding.local != binding.imported {
                            self.named(&gt, &st, &binding.local);
                        }
                    }
                }
                (Declaration::ValueExport(_), Declaration::ValueExport(_))
                | (Declaration::TypeExport(_), Declaration::TypeExport(_)) => {
                    self.module_tokens(&gt, &st)
                }
                _ => {}
            }
        }
        Ok(())
    }
    pub(super) fn return_type(&mut self, g: &[Token], s: &[Token]) {
        fn returned(tokens: &[Token]) -> &[Token] {
            let Some(close) = tokens.iter().rposition(|t| t.is(")")) else {
                return &[];
            };
            if !tokens.get(close + 1).is_some_and(|t| t.is(":")) {
                return &[];
            }
            let rest = &tokens[close + 2..];
            let end = top(rest, "{")
                .or_else(|| top(rest, ";"))
                .unwrap_or(rest.len());
            &rest[..end]
        }
        self.ty(returned(g), returned(s));
    }
    fn module_tokens(&mut self, g: &[Token], s: &[Token]) {
        if g.is_empty() || s.is_empty() {
            return;
        }
        self.point(g[0].start, s[0].start);
        let mut cursor = 0;
        for token in g {
            if let Some(index) = s[cursor..]
                .iter()
                .position(|source| source.text == token.text)
            {
                cursor += index;
                self.pair(token, &s[cursor]);
                cursor += 1;
            }
        }
        self.point(g.last().unwrap().end, s.last().unwrap().end);
    }
    fn field_tokens(&self, span: &SourceSpan, generated: bool) -> Vec<Token> {
        let tokens = if generated {
            &self.generated_tokens
        } else {
            &self.source_tokens
        };
        let mut span = span.clone();
        let next = tokens.partition_point(|token| token.start < span.end);
        if let Some(token) = tokens.get(next).filter(|token| token.is(";")) {
            span.end = token.end;
        }
        within(tokens, &span).to_vec()
    }
    fn comment(&mut self, g: usize, s: usize) {
        fn before(text: &str, end: usize) -> Option<(usize, usize)> {
            let prefix = &text[..end];
            let start = prefix.rfind("/**")?;
            let close = start + prefix[start..].find("*/")? + 2;
            prefix[close..].trim().is_empty().then_some((start, close))
        }
        if let Some(((gs, ge), (ss, se))) =
            before(&self.generated.source, g).zip(before(&self.source.source, s))
        {
            self.point(gs, ss);
            self.point(ge, se);
        }
    }
}
