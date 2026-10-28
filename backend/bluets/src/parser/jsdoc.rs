// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

//! JavaScript comments contribute static metadata, never runtime imports.

use super::*;
mod tags;
use tags::{comments, Tag};

pub(crate) fn is_javascript(id: &str) -> bool {
    [".js", ".mjs", ".cjs"]
        .iter()
        .any(|suffix| id.ends_with(suffix))
}

pub(crate) fn checking_directive(source: &str) -> Option<bool> {
    let tokens = lex("<directive>", source).ok()?;
    let end = tokens.first()?.start;
    let mut result = None;
    for line in source[..end].lines() {
        let line = line
            .trim()
            .trim_start_matches('/')
            .trim_start_matches('*')
            .trim();
        if line.starts_with("@ts-check") {
            result = Some(true);
        }
        if line.starts_with("@ts-nocheck") {
            result = Some(false);
        }
    }
    result
}

fn typed(
    module: &mut Module,
    tag: &Tag,
    limits: &ParserLimits,
    used: &mut usize,
) -> Result<Option<Type>, Vec<Diagnostic>> {
    let Some(mut range) = tag.annotation.clone() else {
        return Ok(None);
    };
    let rest = module.source[range.clone()].starts_with("...");
    if rest {
        range.start += 3;
    }
    let mut tokens = lex_with_limits(
        &module.id,
        &module.source[range.clone()],
        limits.max_source_bytes,
        limits.max_tokens.saturating_sub(*used),
    )?;
    *used = used.saturating_add(tokens.len().saturating_sub(1));
    for token in &mut tokens {
        token.start += range.start;
        token.end += range.start;
    }
    let mut value = implementation::parse_jsdoc_type(module, tokens, limits.max_type_depth)?;
    if let Type::Record(fields) = &mut value {
        for field in fields {
            field.span.end = field.span.start + field.name.len();
        }
    }
    Ok(Some(if rest {
        Type::Array(Box::new(value))
    } else {
        value
    }))
}

pub(super) fn apply(
    module: &mut Module,
    limits: &ParserLimits,
    mut used: usize,
) -> Result<(), Vec<Diagnostic>> {
    let docs = comments(&module.id, &module.source)?;
    let external = module_declarations::has_module_syntax(&module.declarations);
    let mut aliases = Vec::new();
    let mut target = 0;
    for doc in docs {
        let tags = tags::parse(&module.source, doc.clone());
        if tags.is_empty() {
            continue;
        }
        while module
            .declarations
            .get(target)
            .is_some_and(|declaration| declaration.span().end <= doc.start)
        {
            target += 1;
        }
        if module.declarations.get(target).is_some_and(|declaration| {
            declaration.span().start < doc.start && doc.end <= declaration.span().end
        }) {
            return Err(vec![Diagnostic::error(DiagnosticCode::UnsupportedSyntax, SourceSpan::new(&module.id, doc.start, doc.end), "JSDoc annotations inside runtime bodies require a supported nested declaration context")]);
        }
        used = used.saturating_add(tags.len());
        if used > limits.max_tokens {
            return Err(vec![Diagnostic::error(
                DiagnosticCode::ResourceLimit,
                SourceSpan::new(&module.id, doc.start, doc.end),
                "JSDoc exceeds the shared token limit",
            )]);
        }
        for tag in &tags {
            let names = lex_with_limits(
                &module.id,
                &tag.name,
                limits.max_source_bytes,
                limits.max_tokens.saturating_sub(used),
            )?;
            used = used.saturating_add(names.len().saturating_sub(1));
        }
        if let Some(alias) = tags.iter().find(|tag| tag.kind == "typedef") {
            let value = if alias
                .annotation
                .as_ref()
                .is_some_and(|range| module.source[range.clone()].trim() == "Object")
            {
                let mut fields = Vec::new();
                for tag in tags.iter().filter(|tag| tag.kind == "property") {
                    if let Some(mut value) = typed(module, tag, limits, &mut used)? {
                        if tag.optional {
                            value = Type::Union(vec![value, Type::Undefined]);
                        }
                        fields.push(TypeField {
                            accessor_write_type: None,
                            method: false,
                            name: tag.name.clone(),
                            readonly: false,
                            optional: tag.optional,
                            value,
                            span: SourceSpan::new(&module.id, tag.start, tag.end),
                        });
                    }
                }
                Some(Type::Record(fields))
            } else {
                typed(module, alias, limits, &mut used)?
            };
            if let Some(value) = value {
                aliases.push(Declaration::TypeAlias(TypeAliasDeclaration {
                    name: alias.name.clone(),
                    type_parameters: Vec::new(),
                    value,
                    exported: external,
                    span: SourceSpan::new(&module.id, doc.start, doc.end),
                }));
            }
            continue;
        }
        while module
            .declarations
            .get(target)
            .is_some_and(|declaration| declaration.span().start < doc.end)
        {
            target += 1;
        }
        if target == module.declarations.len() {
            continue;
        }
        if let Declaration::Function(function) = &mut module.declarations[target] {
            for tag in tags.iter().filter(|tag| tag.kind == "template") {
                for name in tag
                    .name
                    .split(',')
                    .map(str::trim)
                    .filter(|name| !name.is_empty())
                {
                    function.type_parameters.push(TypeParameter {
                        is_const: false,
                        variance: None,
                        name: name.into(),
                        constraint: None,
                        default: None,
                        span: SourceSpan::new(
                            &module.id,
                            tag.name_start,
                            tag.name_start + name.len(),
                        ),
                    });
                }
            }
        }
        for tag in tags.into_iter().filter(|tag| tag.kind != "template") {
            let references = module.type_references.len();
            let Some(value) = typed(module, &tag, limits, &mut used)? else {
                continue;
            };
            if let Declaration::Function(function) = &module.declarations[target] {
                let parameters = function
                    .type_parameters
                    .iter()
                    .map(|parameter| &parameter.name)
                    .collect::<std::collections::BTreeSet<_>>();
                let mut added = module.type_references.split_off(references);
                added.retain(|reference| {
                    reference.value_query || !parameters.contains(&&reference.name)
                });
                module.type_references.extend(added);
            }
            match &mut module.declarations[target] {
                Declaration::Variable(variable) if tag.kind == "type" => {
                    variable.annotation = Some(value)
                }
                Declaration::Function(function)
                    if tag.kind == "returns" || tag.kind == "return" =>
                {
                    function.return_type = Some(value)
                }
                Declaration::Function(function) if tag.kind == "param" => {
                    if let Some(parameter) = function
                        .parameters
                        .iter_mut()
                        .find(|parameter| parameter.name == tag.name)
                    {
                        parameter.annotation = Some(value);
                        parameter.optional |= tag.optional;
                    }
                }
                _ => {}
            }
        }
    }
    module.declarations.extend(aliases);
    commonjs_exports(module);
    Ok(())
}

/// Recognize only a static assignment to an existing local function. Other
/// CommonJS operations retain the ordinary unsupported/global checks.
fn commonjs_exports(module: &mut Module) {
    if module
        .declarations
        .iter()
        .any(|declaration| match declaration {
            Declaration::Variable(value) => {
                value.name == "exports"
                    || value.pattern.as_ref().is_some_and(|pattern| {
                        pattern.names().iter().any(|(name, _)| *name == "exports")
                    })
            }
            Declaration::Function(value) => value.name == "exports",
            Declaration::Class(value) => value.name == "exports",
            Declaration::Import(value) => value
                .bindings
                .iter()
                .any(|binding| binding.local == "exports"),
            _ => false,
        })
    {
        return;
    }
    let functions = module
        .declarations
        .iter()
        .filter_map(|declaration| match declaration {
            Declaration::Function(function) => Some(function.name.clone()),
            _ => None,
        })
        .collect::<std::collections::BTreeSet<_>>();
    let mut exports = Vec::new();
    module.declarations.retain(|declaration| {
        let Declaration::Raw(raw) = declaration else {
            return true;
        };
        let tokens = &raw.tokens;
        let len = tokens.len() - usize::from(tokens.last().is_some_and(|token| token.is(";")));
        if len == 5
            && tokens[0].is("exports")
            && tokens[1].is(".")
            && tokens[3].is("=")
            && functions.contains(&tokens[4].text)
        {
            exports.push(Declaration::ValueExport(ValueExportDeclaration {
                export_assignment: false,
                bindings: vec![ValueExportBinding {
                    local: tokens[4].text.clone(),
                    exported: tokens[2].text.clone(),
                    type_only: false,
                    span: raw.span.clone(),
                }],
                specifier: None,
                specifier_span: None,
                attributes: None,
                star: false,
                namespace: None,
                span: raw.span.clone(),
            }));
            false
        } else {
            true
        }
    });
    module.declarations.extend(exports);
}
