// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

//! Leading reference comments add bounded, owner-resolved static inputs.

use super::*;
mod library_names;

struct Reference<'a> {
    kind: &'a str,
    name: &'a str,
    span: SourceSpan,
}

fn references(module: &Module) -> Vec<Reference<'_>> {
    let mut result = Vec::new();
    let mut rest = module.source.as_str();
    loop {
        rest = rest.trim_start_matches(|ch: char| ch.is_whitespace() || ch == '\u{feff}');
        if rest.starts_with("/*") {
            let Some(end) = rest.find("*/") else { break };
            rest = &rest[end + 2..];
            continue;
        }
        if !rest.starts_with("//") {
            break;
        }
        let end = rest
            .find(['\r', '\n', '\u{2028}', '\u{2029}'])
            .unwrap_or(rest.len());
        if let Some(comment) = rest[..end].strip_prefix("///") {
            if let Some(attributes) = comment.trim().strip_prefix("<reference") {
                let mut attributes = attributes.trim_start();
                while let Some((key, tail)) = attributes.split_once('=') {
                    let key = key.trim();
                    let value = tail.trim_start();
                    let Some(quote) = value.chars().next().filter(|ch| matches!(ch, '\'' | '"'))
                    else {
                        break;
                    };
                    let value = &value[1..];
                    let Some(end) = value.find(quote) else { break };
                    let name = &value[..end];
                    if matches!(key, "path" | "types" | "lib") {
                        let start = name.as_ptr() as usize - module.source.as_ptr() as usize;
                        result.push(Reference {
                            kind: key,
                            name,
                            span: SourceSpan::new(&module.id, start, start + name.len()),
                        });
                    }
                    attributes = value[end + 1..].trim_start();
                }
            }
        }
        rest = &rest[end..];
    }
    result
}

impl ProjectBuilder<'_> {
    pub(super) fn visit_references(&mut self, module: &Module, depth: usize) {
        for reference in references(module) {
            if reference.kind == "lib" {
                let name = reference.name.to_ascii_lowercase();
                if matches!(name.as_str(), "es2020" | "es2022") {
                    self.project.referenced_libraries.insert(name);
                } else if library_names::is_known(&name) {
                    self.diagnostics.push(Diagnostic::error(
                        DiagnosticCode::UnsupportedSyntax,
                        reference.span,
                        format!("library reference `{name}` is not supported by the owned declaration profiles"),
                    ));
                } else {
                    self.diagnostics.push(
                        Diagnostic::error(
                            DiagnosticCode::ModuleNotFound,
                            reference.span,
                            format!("cannot find library definition `{name}`"),
                        )
                        .with_typescript(2726, vec![name]),
                    );
                }
                continue;
            }
            let (target, code, message) = if reference.kind == "path" {
                (
                    self.loader
                        .resolve_reference_path(&module.id, reference.name),
                    6053,
                    "cannot resolve referenced source",
                )
            } else {
                (
                    self.loader
                        .resolve_reference_types(&module.id, reference.name),
                    2688,
                    "cannot resolve referenced type definition",
                )
            };
            let target = match target {
                Ok(target) => target,
                Err(error) => {
                    let name = if reference.kind == "path" {
                        reference.name.strip_prefix("./").unwrap_or(reference.name)
                    } else {
                        reference.name
                    };
                    self.diagnostics.push(
                        Diagnostic::error(
                            DiagnosticCode::ModuleNotFound,
                            reference.span,
                            format!("{message}: {error}"),
                        )
                        .with_typescript(code, vec![name.to_string()]),
                    );
                    continue;
                }
            };
            let key = (
                module.id.clone(),
                reference.kind.to_string(),
                reference.name.to_string(),
            );
            if !self.project.reference_resolutions.contains_key(&key)
                && self.project.resolutions.len()
                    + self.project.mode_resolutions.len()
                    + self.project.augmentation_resolutions.len()
                    + self.project.reference_resolutions.len()
                    >= self.limits.max_module_edges
            {
                self.diagnostics.push(Diagnostic::error(
                    DiagnosticCode::ResourceLimit,
                    reference.span,
                    format!(
                        "module graph exceeds the {} import-edge limit",
                        self.limits.max_module_edges
                    ),
                ));
                continue;
            }
            self.project
                .reference_resolutions
                .insert(key, target.clone());
            self.project
                .referenced_declaration_modules
                .insert(target.clone());
            self.visit(&target, depth.saturating_add(1));
        }
    }
}
