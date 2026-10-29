// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

//! Helper policies admit only ABIs whose original lowering implements them.

use super::{CompilerOptions, Diagnostic, DiagnosticCode, EmittedJavaScript, Module, SourceSpan};
use crate::parser::TextEdit;
use crate::{Declaration, ModuleKind, Project};
use std::collections::{BTreeMap, BTreeSet};

pub(crate) const VERSION: &str = "bluets-helper-selection/2";
const PREFIX: &str = "__blueice_abi_";
mod fixed;

pub(super) fn external(options: &CompilerOptions) -> bool {
    options.import_helpers || options.no_emit_helpers
}

pub(super) fn reference(source: &str, helper: &str) -> String {
    (0usize..)
        .map(|index| format!("{PREFIX}{helper}_{index}"))
        .find(|name| !source.contains(name))
        .expect("finite source admits a fresh helper reference")
}

pub(super) fn lower(
    mut emitted: EmittedJavaScript,
    module: &Module,
    project: &Project,
    options: &CompilerOptions,
) -> Result<EmittedJavaScript, Diagnostic> {
    if !external(options) {
        return Ok(emitted);
    }
    emitted = fixed::lower(emitted, module, options)?;
    let tokens = crate::syntax::lex_with_limits(
        &module.id,
        &emitted.javascript,
        options.limits.parser.max_source_bytes,
        options.limits.parser.max_tokens,
    )
    .map_err(|mut diagnostics| diagnostics.remove(0))?;
    let references = tokens
        .iter()
        .filter_map(|token| {
            let helper = token.text.strip_prefix(PREFIX)?.rsplit_once('_')?.0;
            (!module.source.contains(&token.text)).then(|| (token.text.clone(), helper.to_string()))
        })
        .collect::<BTreeMap<_, _>>();
    let provider = (0usize..)
        .map(|index| format!("__blueice_tslib_{index}"))
        .find(|name| !emitted.javascript.contains(name))
        .expect("finite source admits a fresh provider binding");
    let mut edits = Vec::new();
    for token in &tokens {
        let Some(helper) = references.get(&token.text) else {
            continue;
        };
        edits.push(TextEdit {
            start: token.start,
            end: token.end,
            replacement: if options.import_helpers {
                format!("{provider}.{helper}")
            } else {
                helper.clone()
            },
        });
    }
    if options.import_helpers && !references.is_empty() {
        for helper in references.values().collect::<BTreeSet<_>>() {
            let span = helper_span(module, helper);
            let Some(target) = project.resolved_module(&module.id, "tslib") else {
                return Err(Diagnostic::error(
                    DiagnosticCode::ModuleNotFound,
                    span,
                    "imported helper provider cannot be resolved",
                )
                .with_typescript(2354, vec!["tslib".into()]));
            };
            if !exports_helper(project, target, helper) {
                return Err(Diagnostic::error(
                    DiagnosticCode::UnsupportedSyntax,
                    span,
                    format!("helper provider does not export {helper}"),
                )
                .with_typescript(2343, vec!["tslib".into(), helper.clone()]));
            }
        }
        let prefix = if options.module_kind == ModuleKind::CommonJs {
            format!("\nvar {provider} = require('tslib');\n")
        } else {
            format!("\nimport * as {provider} from 'tslib';\n")
        };
        let insertion = super::targets::directive_end(&tokens);
        edits.push(TextEdit {
            start: insertion,
            end: insertion,
            replacement: prefix,
        });
    }
    emitted = super::targets::mapped_edits(emitted, edits);
    validate(&emitted, module, options)?;
    Ok(emitted)
}

fn exports_helper(project: &Project, id: &str, helper: &str) -> bool {
    let mut pending = vec![(id.to_string(), helper.to_string())];
    let mut visited = BTreeSet::new();
    while let Some((id, helper)) = pending.pop() {
        if !visited.insert((id.clone(), helper.clone())) {
            continue;
        }
        let Some(module) = project.modules.get(&id) else {
            continue;
        };
        for declaration in &module.declarations {
            match declaration {
                Declaration::Function(function) if function.exported && function.name == helper => {
                    return true
                }
                Declaration::ValueExport(export) if !export.is_type_only() => {
                    let Some(target) = export
                        .specifier
                        .as_ref()
                        .and_then(|specifier| project.resolved_module(&id, specifier))
                    else {
                        continue;
                    };
                    if export.star && export.namespace.is_none() {
                        pending.push((target.to_string(), helper.clone()));
                    }
                    for binding in export
                        .bindings
                        .iter()
                        .filter(|binding| !binding.type_only && binding.exported == helper)
                    {
                        pending.push((target.to_string(), binding.local.clone()));
                    }
                }
                _ => {}
            }
        }
    }
    false
}

fn helper_span(module: &Module, helper: &str) -> SourceSpan {
    let tokens = crate::lex(&module.id, &module.source).unwrap_or_default();
    if helper == "__extends" {
        if let Some(class) = module
            .declarations
            .iter()
            .find_map(|declaration| match declaration {
                Declaration::Class(class) if class.extends_span.is_some() => Some(class),
                _ => None,
            })
        {
            let base = class.extends_span.as_ref().expect("selected heritage");
            let start = tokens
                .iter()
                .rev()
                .find(|token| {
                    token.is("extends")
                        && token.start >= class.name_span.end
                        && token.end <= base.start
                })
                .map_or(base.start, |token| token.start);
            return SourceSpan::new(&module.id, start, base.end);
        }
    }
    if matches!(
        helper,
        "__awaiter" | "__generator" | "__asyncGenerator" | "__await"
    ) {
        if let Some(function) = module
            .declarations
            .iter()
            .find_map(|declaration| match declaration {
                Declaration::Function(function)
                    if function.async_function || function.generator =>
                {
                    Some(function)
                }
                _ => None,
            })
        {
            if let Some(token) = tokens.iter().find(|token| {
                token.text == function.name
                    && token.start >= function.span.start
                    && token.end <= function.body_open.unwrap_or(function.span.end)
            }) {
                return token.span(&module.id);
            }
        }
    }
    SourceSpan::new(&module.id, 0, 0)
}

pub(super) fn validate(
    emitted: &EmittedJavaScript,
    module: &Module,
    options: &CompilerOptions,
) -> Result<(), Diagnostic> {
    if !options.import_helpers && !options.no_emit_helpers {
        return Ok(());
    }
    let original = crate::syntax::lex_with_limits(
        &module.id,
        &module.source,
        options.limits.parser.max_source_bytes,
        options.limits.parser.max_tokens,
    )
    .map_err(|mut diagnostics| diagnostics.remove(0))?;
    let source_functions = original
        .windows(2)
        .filter(|pair| pair[0].is("function"))
        .map(|pair| pair[1].text.as_str())
        .collect::<std::collections::BTreeSet<_>>();
    let tokens = crate::syntax::lex_with_limits(
        &module.id,
        &emitted.javascript,
        options.limits.parser.max_source_bytes,
        options.limits.parser.max_tokens,
    )
    .map_err(|mut diagnostics| diagnostics.remove(0))?;
    for pair in tokens.windows(2) {
        let name = &pair[1].text;
        let target_helper = pair[0].is("function")
            && name.starts_with("__blueice_target_")
            && !name.starts_with("__blueice_target_async_callable_")
            && !source_functions.contains(name.as_str());
        let fixed_helper = matches!(pair[0].text.as_str(), "var" | "let" | "const")
            && matches!(
                name.as_str(),
                "__importDefault"
                    | "__importStar"
                    | "__bluetsClassPrivateGet"
                    | "__bluetsClassPrivateSet"
                    | "__bluetsClassPrivateIn"
                    | "__bluetsRunInitializers"
                    | "__bluetsEsDecorate"
                    | "__bluetsDecorate"
                    | "__bluetsParam"
                    | "__bluetsMetadata"
            );
        if target_helper || fixed_helper {
            return Err(Diagnostic::error(
                DiagnosticCode::UnsupportedSyntax,
                SourceSpan::new(&module.id, 0, 0),
                format!("helper selection does not support generated ABI `{name}`"),
            ));
        }
    }
    Ok(())
}
