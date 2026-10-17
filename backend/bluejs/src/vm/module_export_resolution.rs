// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

//! ResolveExport owns iterative query storage and borrows canonical bindings.

use super::module_walk_storage::Storage;
use super::*;

pub(super) enum ResolvedModule<'a> {
    Known(&'a str),
    Owned(ModuleName),
}
impl<'a> ResolvedModule<'a> {
    pub(super) fn resolve(
        modules: &'a HashMap<String, Bytecode>,
        referrer: &str,
        request: &str,
        module_type: ModuleType,
        scope: Option<&SchedulingScope>,
    ) -> Result<Self, RuntimeError> {
        let target = ModuleName::resolve(referrer, request, module_type, scope)?;
        Ok(match modules.get_key_value(target.as_str()) {
            Some((key, _)) => Self::Known(key.as_str()),
            None => Self::Owned(target),
        })
    }
    pub(super) fn as_str(&self) -> &str {
        match self {
            Self::Known(module) => module,
            Self::Owned(module) => module.as_str(),
        }
    }
    fn supplied(self) -> Result<&'a str, RuntimeError> {
        match self {
            Self::Known(module) => Ok(module),
            Self::Owned(module) => Err(RuntimeError::ModuleResolution(format!(
                "module {} was not supplied by the host",
                module.as_str()
            ))),
        }
    }
}
impl std::ops::Deref for ResolvedModule<'_> {
    type Target = str;
    fn deref(&self) -> &str {
        self.as_str()
    }
}
impl std::fmt::Display for ResolvedModule<'_> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(self.as_str())
    }
}
impl std::fmt::Debug for ResolvedModule<'_> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        std::fmt::Debug::fmt(self.as_str(), f)
    }
}
impl PartialEq for ResolvedModule<'_> {
    fn eq(&self, other: &Self) -> bool {
        self.as_str() == other.as_str()
    }
}
impl Eq for ResolvedModule<'_> {}

#[derive(Debug, PartialEq, Eq)]
pub(super) enum ExportResolution<'a> {
    Binding {
        module: ResolvedModule<'a>,
        slot: usize,
    },
    Missing,
    Ambiguous,
    Namespace {
        module: ResolvedModule<'a>,
    },
    DeferredNamespace {
        module: ResolvedModule<'a>,
    },
    Source {
        module: ResolvedModule<'a>,
    },
}

enum Step<'a> {
    Done(ExportResolution<'a>),
    Indirect { module: &'a str, name: &'a str },
    Stars,
}
fn explicit<'a>(
    modules: &'a HashMap<String, Bytecode>,
    module: &'a str,
    name: &str,
    scope: Option<&SchedulingScope>,
) -> Result<Step<'a>, RuntimeError> {
    let exports = &modules[module].module_exports;
    for export in exports {
        let target =
            |request: &str, kind| ResolvedModule::resolve(modules, module, request, kind, scope);
        match export {
            ModuleExport::Local {
                export_name,
                local_slot,
            } if export_name == name => {
                return Ok(Step::Done(ExportResolution::Binding {
                    module: ResolvedModule::Known(module),
                    slot: *local_slot as usize,
                }));
            }
            ModuleExport::Indirect {
                export_name,
                module_request,
                import_name,
                module_type,
            } if export_name == name => {
                return Ok(Step::Indirect {
                    module: target(module_request, *module_type)?.supplied()?,
                    name: import_name,
                });
            }
            ModuleExport::Namespace {
                export_name,
                module_request,
                module_type,
            } if export_name == name => {
                return Ok(Step::Done(ExportResolution::Namespace {
                    module: target(module_request, *module_type)?,
                }));
            }
            ModuleExport::DeferredNamespace {
                export_name,
                module_request,
                module_type,
            } if export_name == name => {
                return Ok(Step::Done(ExportResolution::DeferredNamespace {
                    module: target(module_request, *module_type)?,
                }));
            }
            ModuleExport::Source {
                export_name,
                module_request,
            } if export_name == name => {
                return Ok(Step::Done(ExportResolution::Source {
                    module: target(module_request, ModuleType::JavaScript)?,
                }));
            }
            _ => {}
        }
    }
    Ok(
        if name == "default"
            || !exports
                .iter()
                .any(|e| matches!(e, ModuleExport::Star { .. }))
        {
            Step::Done(ExportResolution::Missing)
        } else {
            Step::Stars
        },
    )
}

struct Frame<'module, 'name> {
    module: &'module str,
    name: &'name str,
    indirect: bool,
    next_star: usize,
    candidate: ExportResolution<'module>,
}

pub(super) fn resolve<'a>(
    modules: &'a HashMap<String, Bytecode>,
    module: &str,
    name: &str,
    scope: Option<&SchedulingScope>,
) -> Result<ExportResolution<'a>, RuntimeError> {
    if let Some(scope) = scope {
        scope.shared_charge(0)?;
    }
    let root = modules
        .get_key_value(module)
        .map(|(key, _)| key.as_str())
        .ok_or_else(|| {
            RuntimeError::ModuleResolution(format!("module {module} was not supplied by the host"))
        })?;
    let mut visited = Storage::default();
    let mut frames: Storage<Frame<'a, '_>> = Storage::default();
    let mut pending = Some((root, name));
    let mut returned = None;
    loop {
        if let Some((module, name)) = pending.take() {
            if visited.data.contains(&(module, name)) {
                returned = Some(ExportResolution::Missing);
                continue;
            }
            let step = explicit(modules, module, name, scope)?;
            let indirect = match step {
                Step::Done(result) => {
                    returned = Some(result);
                    continue;
                }
                Step::Indirect { module, name } => {
                    pending = Some((module, name));
                    true
                }
                Step::Stars => false,
            };
            visited.push((module, name), scope)?;
            frames.push(
                Frame {
                    module,
                    name,
                    indirect,
                    next_star: 0,
                    candidate: ExportResolution::Missing,
                },
                scope,
            )?;
            continue;
        }
        if let Some(result) = returned.take() {
            let Some(parent) = frames.data.last_mut() else {
                return Ok(result);
            };
            if parent.indirect {
                frames.data.pop();
                returned = Some(result);
            } else {
                match result {
                    ExportResolution::Missing => {}
                    ExportResolution::Ambiguous => {
                        frames.data.pop();
                        returned = Some(ExportResolution::Ambiguous);
                    }
                    found => {
                        if parent.candidate == ExportResolution::Missing {
                            parent.candidate = found;
                        } else if parent.candidate != found {
                            frames.data.pop();
                            returned = Some(ExportResolution::Ambiguous);
                        }
                    }
                }
            }
            continue;
        }
        let frame = frames
            .data
            .last_mut()
            .expect("unfinished star query owns a frame");
        debug_assert!(!frame.indirect);
        let exports = &modules[frame.module].module_exports;
        let mut request = None;
        while let Some(export) = exports.get(frame.next_star) {
            frame.next_star += 1;
            if let ModuleExport::Star {
                module_request,
                module_type,
            } = export
            {
                request = Some((module_request, *module_type));
                break;
            }
        }
        if let Some((request, kind)) = request {
            let target =
                ResolvedModule::resolve(modules, frame.module, request, kind, scope)?.supplied()?;
            pending = Some((target, frame.name));
        } else {
            let frame = frames.data.pop().unwrap();
            returned = Some(frame.candidate);
        }
    }
}
