// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

//! Declaration provenance pairs retained syntax, leaving inferred nodes unmapped.
use super::*;
use crate::parser::{ClassDeclaration, ClassMemberShell, Parameter};
use crate::syntax::{Token, TokenKind};
use std::collections::BTreeSet;
mod declarations;
mod members;
mod types;
pub(crate) const VERSION: &str = "declaration-map-provenance-v2";

pub(super) fn build(
    id: &str,
    module: &Module,
    declaration: &str,
    options: &CompilerOptions,
) -> Result<SourceMap, Diagnostic> {
    let generated = crate::parser::parse_module_with_limits(
        format!("{id}.d.ts"),
        declaration,
        options.limits.parser.clone(),
    )
    .map_err(|errors| {
        errors
            .into_iter()
            .next()
            .expect("generated syntax diagnostic")
    })?;
    let mut mapper = Mapper {
        source: module,
        generated: &generated,
        source_tokens: lexical(id, &module.source)?,
        generated_tokens: lexical(id, declaration)?,
        points: BTreeMap::new(),
        limit: options.limits.max_source_map_segments,
        exhausted: false,
    };
    mapper.declarations(&module.declarations, &generated.declarations)?;
    if mapper.exhausted {
        return Err(Diagnostic::error(
            DiagnosticCode::ResourceLimit,
            SourceSpan::new(id, 0, 0),
            "declaration map exceeded the segment limit",
        ));
    }
    let segments = mapper
        .points
        .into_iter()
        .map(|(generated_offset, source_offset)| {
            let (generated_line, generated_column) = position(declaration, generated_offset);
            let (source_line, source_column) = position(&module.source, source_offset);
            ProvenanceSegment {
                generated_line,
                generated_column,
                source_line,
                source_column,
                mapped: true,
                name_index: None,
            }
        })
        .collect::<Vec<_>>();
    let file = output_file_name(id);
    let file = if let Some(stem) = file.strip_suffix(".mjs") {
        format!("{stem}.d.mts")
    } else if let Some(stem) = file.strip_suffix(".cjs") {
        format!("{stem}.d.cts")
    } else {
        format!("{}.d.ts", file.strip_suffix(".js").unwrap_or(&file))
    };
    Ok(SourceMap {
        file,
        sources: vec![id.into()],
        sources_content: Vec::new(),
        inline_sources: false,
        source_root: options.source_root.clone().unwrap_or_default(),
        names: Vec::new(),
        mappings: encode_mappings(&segments),
    })
}

struct Mapper<'a> {
    source: &'a Module,
    generated: &'a Module,
    source_tokens: Vec<Token>,
    generated_tokens: Vec<Token>,
    points: BTreeMap<usize, usize>,
    limit: usize,
    exhausted: bool,
}
impl Mapper<'_> {
    fn point(&mut self, generated: usize, source: usize) {
        if generated > self.generated.source.len() || source > self.source.source.len() {
            return;
        }
        if !self.points.contains_key(&generated) && self.points.len() >= self.limit {
            self.exhausted = true;
            return;
        }
        self.points.insert(generated, source);
    }
    fn pair(&mut self, generated: &Token, source: &Token) {
        self.point(generated.start, source.start);
        self.point(generated.end, source.end);
    }
    fn range(&mut self, generated: &SourceSpan, source: &SourceSpan) {
        self.point(generated.start, source.start);
        self.point(generated.end, source.end);
    }
    fn source(&self, span: &SourceSpan) -> Vec<Token> {
        within(&self.source_tokens, span).to_vec()
    }
    fn generated(&self, span: &SourceSpan) -> Vec<Token> {
        within(&self.generated_tokens, span).to_vec()
    }
    fn named(&mut self, g: &[Token], s: &[Token], name: &str) {
        if let Some((g, s)) = g
            .iter()
            .find(|t| t.text == name)
            .zip(s.iter().find(|t| t.text == name))
        {
            self.pair(g, s);
        }
    }
    fn modifiers(&mut self, g: &[Token], s: &[Token]) {
        for token in g.iter().filter(|t| {
            matches!(
                t.text.as_str(),
                "export"
                    | "default"
                    | "abstract"
                    | "public"
                    | "protected"
                    | "private"
                    | "readonly"
                    | "static"
                    | "declare"
            )
        }) {
            if let Some(source) = s.iter().find(|source| source.text == token.text) {
                self.pair(token, source);
            }
        }
    }
}

fn lexical(id: &str, text: &str) -> Result<Vec<Token>, Diagnostic> {
    let tokens = crate::syntax::lex(id, text)
        .map_err(|errors| errors.into_iter().next().expect("lexical diagnostic"))?;
    let mut result = Vec::new();
    for token in tokens.into_iter().filter(|t| t.kind != TokenKind::Eof) {
        // This token view is provenance-only: it never changes runtime parsing.
        if token.text.len() > 1
            && token.text.starts_with('>')
            && token.text.bytes().all(|b| b == b'>' || b == b'=')
        {
            for (index, byte) in token.text.bytes().enumerate() {
                result.push(Token {
                    kind: token.kind,
                    text: (byte as char).to_string(),
                    start: token.start + index,
                    end: token.start + index + 1,
                });
            }
        } else {
            result.push(token);
        }
    }
    Ok(result)
}
fn within<'a>(tokens: &'a [Token], span: &SourceSpan) -> &'a [Token] {
    let start = tokens.partition_point(|t| t.end <= span.start);
    let end = tokens.partition_point(|t| t.start < span.end);
    &tokens[start..end.max(start)]
}
fn top(tokens: &[Token], wanted: &str) -> Option<usize> {
    let mut depth = 0usize;
    for (index, token) in tokens.iter().enumerate() {
        if depth == 0 && token.is(wanted) {
            return Some(index);
        }
        if matches!(token.text.as_str(), "(" | "[" | "{" | "<") {
            depth += 1;
        } else if matches!(token.text.as_str(), ")" | "]" | "}" | ">") {
            depth = depth.saturating_sub(1);
        }
    }
    None
}
fn annotation(tokens: &[Token]) -> &[Token] {
    let Some(colon) = top(tokens, ":") else {
        return &[];
    };
    let rest = &tokens[colon + 1..];
    let end = top(rest, "=")
        .or_else(|| top(rest, ";"))
        .unwrap_or(rest.len());
    &rest[..end]
}
fn position(text: &str, offset: usize) -> (usize, usize) {
    let prefix = &text[..offset];
    (
        prefix.bytes().filter(|b| *b == b'\n').count(),
        prefix.rsplit('\n').next().unwrap().encode_utf16().count(),
    )
}
fn unsupported(span: &SourceSpan) -> Diagnostic {
    Diagnostic::error(
        DiagnosticCode::UnsupportedSyntax,
        span.clone(),
        "declaration-map provenance for this declaration shape is not implemented",
    )
}
