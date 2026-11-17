// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

//! Inclusion reasons retain root order, original spans and already-read sources.

use super::*;
use blueice_bluets::{Declaration, TypeScriptRelatedInformation};

pub(super) struct Reason<'a> {
    pub(super) text: String,
    pub(super) primary: bool,
    pub(super) span: SourceSpan,
    pub(super) source: &'a str,
    code: u32,
}

impl Reason<'_> {
    pub(super) fn related(&self) -> Option<TypeScriptRelatedInformation> {
        if self.code == 0 {
            return None;
        }
        let counterpart = Diagnostic::error(
            DiagnosticCode::ModuleNotFound,
            self.span.clone(),
            &self.text,
        )
        .with_typescript(self.code, Vec::new())
        .with_source_position(self.source)
        .typescript?;
        Some(TypeScriptRelatedInformation {
            code: counterpart.code,
            message: counterpart.message,
            span: counterpart.span,
            position: counterpart.position,
        })
    }
}

pub(super) fn collect<'a>(
    graph: &'a Project,
    config: &'a tsconfig::ProjectConfig,
    loader: &FileLoader,
) -> BTreeMap<String, Vec<Reason<'a>>> {
    let mut reasons = BTreeMap::<String, Vec<Reason>>::new();
    let mut seen = BTreeSet::new();
    for root in &graph.program_roots {
        reasons
            .entry(root.clone())
            .or_default()
            .push(root_reason(root, config, loader));
        visit(root, graph, loader, &mut seen, &mut reasons);
    }
    reasons
}

fn visit<'a>(
    id: &str,
    graph: &'a Project,
    loader: &FileLoader,
    seen: &mut BTreeSet<String>,
    reasons: &mut BTreeMap<String, Vec<Reason<'a>>>,
) {
    if !seen.insert(id.to_owned()) {
        return;
    }
    let Some(module) = graph.modules.get(id) else {
        return;
    };
    for declaration in &module.declarations {
        let edge = match declaration {
            Declaration::Import(import) => Some((
                import.specifier.as_str(),
                &import.specifier_span,
                if import.equals_require {
                    Some(ImportMode::Require)
                } else {
                    import
                        .attributes
                        .as_ref()
                        .and_then(blueice_bluets::ImportAttributes::resolution_mode)
                },
            )),
            Declaration::TypeExport(export) => export
                .specifier
                .as_deref()
                .zip(export.specifier_span.as_ref())
                .map(|(specifier, span)| {
                    (
                        specifier,
                        span,
                        export
                            .attributes
                            .as_ref()
                            .and_then(blueice_bluets::ImportAttributes::resolution_mode),
                    )
                }),
            Declaration::ValueExport(export) => export
                .specifier
                .as_deref()
                .zip(export.specifier_span.as_ref())
                .map(|(specifier, span)| (specifier, span, None)),
            _ => None,
        };
        let Some((specifier, span, mode)) = edge else {
            continue;
        };
        let Some(target) = graph.resolved_module_with_mode(id, specifier, mode) else {
            continue;
        };
        let Ok(path) = loader.source_path(id) else {
            continue;
        };
        reasons.entry(target.into()).or_default().push(Reason {
            text: format!(
                "Imported via {} from file '{}'",
                module.source.get(span.start..span.end).unwrap_or(specifier),
                path.to_string_lossy().replace('\\', "/")
            ),
            primary: true,
            span: span.clone(),
            source: &module.source,
            code: 1399,
        });
        visit(target, graph, loader, seen, reasons);
    }
}

fn root_reason<'a>(
    id: &str,
    config: &'a tsconfig::ProjectConfig,
    loader: &FileLoader,
) -> Reason<'a> {
    let path = loader.source_path(id).ok();
    // These bytes were admitted and fingerprinted by Reader; diagnostic work
    // performs no additional filesystem read or path discovery.
    for (config_path, source) in &config.config_sources {
        let Ok(tokens) = blueice_bluets::lex("tsconfig.json", source) else {
            continue;
        };
        let Some(start) = tokens
            .windows(3)
            .position(|tokens| tokens[0].is("\"files\"") && tokens[1].is(":") && tokens[2].is("["))
        else {
            continue;
        };
        for token in tokens[start + 3..]
            .iter()
            .take_while(|token| !token.is("]"))
        {
            let Ok(file) = serde_json::from_str::<String>(&token.text) else {
                continue;
            };
            if path.as_ref()
                != Some(&tsconfig::clean_path(
                    &config_path.parent().unwrap().join(file),
                ))
            {
                continue;
            }
            let name = tsconfig::relative_text(&config.directory, config_path);
            return Reason {
                text: format!("Part of 'files' list in {name}"),
                primary: false,
                span: token.span(&name),
                source,
                code: 1410,
            };
        }
    }
    Reason {
        text: "Root file specified for compilation".into(),
        primary: false,
        span: SourceSpan::new("", 0, 0),
        source: "",
        code: 0,
    }
}
