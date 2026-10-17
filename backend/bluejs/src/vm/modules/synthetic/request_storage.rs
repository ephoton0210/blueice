// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

//! Static synthetic request staging owns native buffers through registration.

use super::*;
use crate::vm::module_walk_storage::Storage;

fn selected(code: &Bytecode) -> impl Iterator<Item = (&str, ModuleType, SyntheticModuleType)> {
    let imports = code.module_imports.iter().filter_map(|import| {
        if matches!(import.import_name, ModuleImportName::Source) {
            return None;
        }
        SyntheticModuleType::from_module_type(import.module_type)
            .map(|kind| (import.module_request.as_str(), import.module_type, kind))
    });
    let exports = code.module_exports.iter().filter_map(|export| {
        let (request, module_type) = match export {
            ModuleExport::Indirect {
                module_request,
                module_type,
                ..
            }
            | ModuleExport::Star {
                module_request,
                module_type,
            }
            | ModuleExport::Namespace {
                module_request,
                module_type,
                ..
            }
            | ModuleExport::DeferredNamespace {
                module_request,
                module_type,
                ..
            } => (module_request.as_str(), *module_type),
            ModuleExport::Local { .. } | ModuleExport::Source { .. } => return None,
        };
        SyntheticModuleType::from_module_type(module_type).map(|kind| (request, module_type, kind))
    });
    imports.chain(exports)
}

pub(super) struct Requests {
    targets: Storage<(ModuleName, SyntheticModuleType)>,
}
impl Requests {
    pub(super) fn collect(
        modules: &HashMap<String, Bytecode>,
        scope: Option<&SchedulingScope>,
    ) -> Result<Self, RuntimeError> {
        if let Some(scope) = scope {
            scope.shared_charge(0)?;
        }
        let mut names = Storage::default();
        names.reserve(modules.len(), scope)?;
        names.data.extend(modules.keys());
        names.data.sort_unstable();

        // Admit the complete target backing before resolving any request.
        // Imports precede exports within each canonical module, as before.
        let count = modules.values().try_fold(0usize, |count, code| {
            count
                .checked_add(selected(code).count())
                .ok_or_else(|| scope.map_or(RuntimeError::SchedulingLimit, SchedulingScope::fail))
        })?;
        let mut targets = Storage::default();
        targets.reserve(count, scope)?;
        for name in names.data {
            let code = &modules[name];
            for (request, module_type, kind) in selected(code) {
                let target = ModuleName::resolve(name, request, module_type, scope)?;
                targets.data.push((target, kind));
            }
        }
        Ok(Self { targets })
    }
    pub(super) fn drain(&mut self) -> impl Iterator<Item = (ModuleName, SyntheticModuleType)> + '_ {
        // The batch owns its backing until Drain has destroyed remaining items;
        // each yielded key keeps its own charge through host registration.
        self.targets.data.drain(..)
    }
    #[cfg(any(test, coverage))]
    pub(super) fn bytes(&self) -> usize {
        self.targets.bytes()
            + self
                .targets
                .data
                .iter()
                .map(|(name, _)| name.bytes())
                .sum::<usize>()
    }
    #[cfg(any(test, coverage))]
    pub(super) fn iter(&self) -> impl Iterator<Item = (&str, SyntheticModuleType)> {
        self.targets
            .data
            .iter()
            .map(|(name, kind)| (name.as_str(), *kind))
    }
}
