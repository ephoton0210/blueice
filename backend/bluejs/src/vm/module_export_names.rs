// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

//! GetExportedNames borrows source names and owns admitted traversal storage.

use super::module_walk_storage::{Storage, Visited};
use super::*;

struct Frame<'a> {
    module: &'a str,
    next_export: usize,
}

pub(super) struct ExportNames<'a> {
    names: Storage<&'a str>,
}
impl<'a> std::ops::Deref for ExportNames<'a> {
    type Target = [&'a str];
    fn deref(&self) -> &Self::Target {
        &self.names.data
    }
}
impl<'a> ExportNames<'a> {
    pub(super) fn collect(
        modules: &'a HashMap<String, Bytecode>,
        module: &str,
        scope: Option<&SchedulingScope>,
    ) -> Result<Self, RuntimeError> {
        if let Some(scope) = scope {
            scope.shared_charge(0)?;
        }
        let root = modules
            .get_key_value(module)
            .map(|(key, _)| key.as_str())
            .ok_or_else(|| {
                RuntimeError::ModuleResolution(format!(
                    "module {module} was not supplied by the host"
                ))
            })?;
        let mut visited = Visited::default();
        let mut distinct_names = Visited::default();
        let mut frames = Storage::default();
        let mut names = Storage::default();
        visited.insert(root, scope)?;
        frames.push(
            Frame {
                module: root,
                next_export: 0,
            },
            scope,
        )?;
        while let Some(frame) = frames.data.last_mut() {
            let Some(export) = modules[frame.module].module_exports.get(frame.next_export) else {
                frames.data.pop();
                continue;
            };
            frame.next_export += 1;
            match export {
                ModuleExport::Local { export_name, .. }
                | ModuleExport::Indirect { export_name, .. }
                | ModuleExport::Namespace { export_name, .. }
                | ModuleExport::DeferredNamespace { export_name, .. }
                | ModuleExport::Source { export_name, .. } => {
                    let name = export_name.as_str();
                    if (frame.module == root || name != "default")
                        && distinct_names.insert(name, scope)?
                    {
                        names.push(name, scope)?;
                    }
                }
                ModuleExport::Star {
                    module_request,
                    module_type,
                } => {
                    let target =
                        ModuleName::resolve(frame.module, module_request, *module_type, scope)?;
                    let canonical = modules
                        .get_key_value(target.as_str())
                        .map(|(key, _)| key.as_str())
                        .ok_or_else(|| {
                            RuntimeError::ModuleResolution(format!(
                                "module {} was not supplied by the host",
                                target.as_str()
                            ))
                        })?;
                    drop(target);
                    if visited.insert(canonical, scope)? {
                        frames.push(
                            Frame {
                                module: canonical,
                                next_export: 0,
                            },
                            scope,
                        )?;
                    }
                }
            }
        }
        names.data.sort_unstable();
        Ok(Self { names })
    }
    pub(super) fn exclude(&mut self, name: &str) {
        self.names.data.retain(|value| *value != name);
    }
    #[cfg(any(test, coverage))]
    pub(super) fn bytes(&self) -> usize {
        self.names.bytes()
    }
}
