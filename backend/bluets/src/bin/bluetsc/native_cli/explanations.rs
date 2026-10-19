// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

//! File inclusion reasons use only the graph and configuration already authorized.

use super::*;

#[derive(Default)]
pub(super) struct Explanations {
    nodes: BTreeMap<String, Node>,
}

#[derive(Default)]
struct Node {
    display: String,
    dependencies: Vec<String>,
    reasons: Vec<String>,
}

impl Explanations {
    pub(super) fn collect(
        &mut self,
        graph: &Project,
        invocation: &Invocation,
        loader: &FileLoader,
    ) {
        let Some(config) = &invocation.project_config else {
            return;
        };
        for id in graph.modules.keys() {
            let Ok(path) = loader.source_path(id) else {
                continue;
            };
            if !path.starts_with(&invocation.root) {
                continue;
            }
            self.nodes.entry(id.clone()).or_default().display =
                tsconfig::relative_text(&config.directory, &path);
        }
        for (source, kind, specifier, target) in graph.resolved_source_references() {
            if !self.nodes.contains_key(source) || !self.nodes.contains_key(target) {
                continue;
            }
            let from = self.nodes[source].display.clone();
            let reason = if kind == "path" {
                format!("Referenced via '{specifier}' from file '{from}'")
            } else {
                format!("Type library referenced via '{specifier}' from file '{from}'")
            };
            self.edge(source, target, reason);
        }
        for (id, module) in &graph.modules {
            if !self.nodes.contains_key(id) {
                continue;
            }
            for declaration in &module.declarations {
                let edge = match declaration {
                    blueice_bluets::Declaration::Import(import) => Some((
                        import.specifier.as_str(),
                        import
                            .attributes
                            .as_ref()
                            .and_then(blueice_bluets::ImportAttributes::resolution_mode),
                    )),
                    blueice_bluets::Declaration::TypeExport(export) => {
                        export.specifier.as_deref().map(|specifier| {
                            (
                                specifier,
                                export
                                    .attributes
                                    .as_ref()
                                    .and_then(blueice_bluets::ImportAttributes::resolution_mode),
                            )
                        })
                    }
                    blueice_bluets::Declaration::ValueExport(export) => export
                        .specifier
                        .as_deref()
                        .map(|specifier| (specifier, None)),
                    _ => None,
                };
                let Some((specifier, mode)) = edge else {
                    continue;
                };
                let Some(target) = graph.resolved_module_with_mode(id, specifier, mode) else {
                    continue;
                };
                if self.nodes.contains_key(target) {
                    let from = &self.nodes[id].display;
                    self.edge(
                        id,
                        target,
                        format!("Imported via '{specifier}' from file '{from}'"),
                    );
                }
            }
        }
    }

    fn edge(&mut self, source: &str, target: &str, reason: String) {
        let dependencies = &mut self.nodes.get_mut(source).unwrap().dependencies;
        if !dependencies.iter().any(|dependency| dependency == target) {
            dependencies.push(target.into());
        }
        self.reason(target, reason);
    }

    fn reason(&mut self, id: &str, reason: String) {
        let Some(node) = self.nodes.get_mut(id) else {
            return;
        };
        if !node.reasons.contains(&reason) {
            node.reasons.push(reason);
        }
    }

    pub(super) fn render(mut self, invocation: &Invocation, loader: &FileLoader) -> String {
        let Some(config) = &invocation.project_config else {
            return String::new();
        };
        let mut roots = Vec::new();
        let config_name = tsconfig::relative_text(&config.directory, &config.config_path);
        for path in &config.file_order {
            let Ok(id) = loader.module_id(path) else {
                continue;
            };
            roots.push(id.clone());
            let reason = if config.explicit_files.contains(path) {
                format!("Part of 'files' list in {config_name}")
            } else {
                let Some(pattern) = config.inclusion_pattern(path) else {
                    continue;
                };
                format!("Matched by include pattern '{pattern}' in '{config_name}'")
            };
            self.reason(&id, reason);
        }
        for (path, name) in &config.type_libraries {
            let Ok(id) = loader.module_id(path) else {
                continue;
            };
            roots.push(id.clone());
            let reason = if config.options.contains_key("types") {
                format!("Entry point of type library '{name}' specified in compilerOptions")
            } else {
                format!("Entry point for implicit type library '{name}'")
            };
            self.reason(&id, reason);
        }
        let mut output = String::new();
        let mut seen = BTreeSet::new();
        for id in roots {
            self.visit(&id, &mut seen, &mut output);
        }
        output
    }

    fn visit(&self, id: &str, seen: &mut BTreeSet<String>, output: &mut String) {
        if !seen.insert(id.to_string()) {
            return;
        }
        let Some(node) = self.nodes.get(id) else {
            return;
        };
        for target in &node.dependencies {
            self.visit(target, seen, output);
        }
        output.push_str(&node.display);
        output.push('\n');
        for reason in &node.reasons {
            output.push_str("  ");
            output.push_str(reason);
            output.push('\n');
        }
    }
}
