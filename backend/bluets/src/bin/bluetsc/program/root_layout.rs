// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

//! Layout validation uses the complete admitted graph, including static checks.

use super::*;
use blueice_bluets::{Diagnostic, DiagnosticCode, Project};
use serde_json::Value;
#[path = "root_layout/reasons.rs"]
mod reasons;

pub(crate) fn diagnostics(
    invocation: &Invocation,
    graph: &Project,
    loader: &FileLoader,
) -> Vec<Diagnostic> {
    let Some(config) = invocation.project_config.as_ref().filter(|config| {
        config
            .options
            .get("rootDir")
            .and_then(Value::as_str)
            .is_some()
    }) else {
        return Vec::new();
    };
    let outside = graph
        .modules
        .keys()
        .filter(|id| {
            !is_declaration_path(Path::new(id)) && !blueice_bluets::is_external_library_module(id)
        })
        .filter_map(|id| {
            let path = loader.source_path(id).ok()?;
            (!path.starts_with(&config.emit_root)).then_some((id, path))
        })
        .collect::<Vec<_>>();
    if outside.is_empty() {
        return Vec::new();
    }
    let reasons = reasons::collect(graph, config, loader);
    outside
        .into_iter()
        .filter_map(|(id, path)| {
            let reasons = reasons.get(id).map(Vec::as_slice).unwrap_or_default();
            let primary = reasons.first().filter(|reason| reason.primary);
            let span = primary
                .map(|reason| reason.span.clone())
                .unwrap_or_else(|| SourceSpan::new("", 0, 0));
            let mut diagnostic = Diagnostic::error(
                DiagnosticCode::ModuleNotFound,
                span,
                format!("source `{id}` is outside configured rootDir"),
            )
            .with_typescript(
                6059,
                vec![
                    path.to_string_lossy().replace('\\', "/"),
                    config.emit_root.to_string_lossy().replace('\\', "/"),
                ],
            );
            if let Some(primary) = primary {
                diagnostic = diagnostic.with_source_position(primary.source);
            }
            let counterpart = diagnostic.typescript.as_mut()?;
            if reasons.len() > 1 || primary.is_none() && !reasons.is_empty() {
                counterpart
                    .message
                    .push_str("\n  The file is in the program because:");
                for reason in reasons {
                    counterpart.message.push_str("\n    ");
                    counterpart.message.push_str(&reason.text);
                }
            }
            for reason in reasons.iter().skip(usize::from(primary.is_some())) {
                if let Some(related) = reason.related() {
                    counterpart.related_information.push(related);
                }
            }
            Some(diagnostic)
        })
        .collect()
}
