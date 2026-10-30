// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

//! Prepared code owns native buffers through registry adoption.

use super::host_scheduling::{owned_code_bytes, SharedSchedulingCharge};
use super::*;

pub(super) struct PreparedModuleCode {
    pub(super) code: Bytecode,
    pub(super) key: String,
    pub(super) charge: Option<SharedSchedulingCharge>,
}
impl std::ops::Deref for PreparedModuleCode {
    type Target = Bytecode;
    fn deref(&self) -> &Bytecode {
        &self.code
    }
}
fn fail(scope: Option<&SchedulingScope>) -> RuntimeError {
    scope.map_or(RuntimeError::SchedulingLimit, SchedulingScope::fail)
}
impl PreparedModuleCode {
    pub(super) fn input(code: Bytecode) -> Self {
        Self {
            code,
            key: String::new(),
            charge: None,
        }
    }
    fn resize(&mut self, bytes: usize) -> Result<(), RuntimeError> {
        if let Some(charge) = &mut self.charge {
            charge.resize(bytes)?;
        }
        Ok(())
    }
    pub(super) fn synthetic(scope: Option<&SchedulingScope>) -> Result<Self, RuntimeError> {
        let mut pending = std::mem::size_of::<Binding>()
            + std::mem::size_of::<Vec<u32>>()
            + std::mem::size_of::<u32>()
            + std::mem::size_of::<ModuleExport>()
            + 2 * "default".len();
        let charge = scope
            .map(|scope| scope.shared_charge(pending))
            .transpose()?;
        let mut prepared = Self {
            code: Bytecode::empty(),
            key: String::new(),
            charge,
        };
        macro_rules! reserve {
            ($field:ident, $item:ty) => {{
                prepared
                    .code
                    .$field
                    .try_reserve_exact(1)
                    .map_err(|_| fail(scope))?;
                pending -= std::mem::size_of::<$item>();
                prepared.resize(prepared.bytes().saturating_add(pending))?;
            }};
        }
        prepared
            .code
            .bindings_mut()
            .try_reserve_exact(1)
            .map_err(|_| fail(scope))?;
        pending -= std::mem::size_of::<Binding>();
        prepared.resize(prepared.bytes().saturating_add(pending))?;
        reserve!(scopes, Vec<u32>);
        reserve!(module_exports, ModuleExport);
        let name = module_referrers::copy_referrer("default", scope)?;
        prepared.code.bindings_mut().push(Binding {
            name,
            mutable: false,
            strict_immutable: true,
            lexical: false,
            catch_parameter: false,
            eval_var: false,
        });
        pending -= "default".len();
        prepared.resize(prepared.bytes().saturating_add(pending))?;
        let mut slots = Vec::new();
        slots.try_reserve_exact(1).map_err(|_| fail(scope))?;
        slots.push(0);
        prepared.code.scopes.push(slots);
        pending -= std::mem::size_of::<u32>();
        prepared.resize(prepared.bytes().saturating_add(pending))?;
        let export_name = module_referrers::copy_referrer("default", scope)?;
        prepared.code.module_exports.push(ModuleExport::Local {
            export_name,
            local_slot: 0,
        });
        pending -= "default".len();
        debug_assert_eq!(pending, 0);
        prepared.resize(prepared.bytes())?;
        prepared.code.module = true;
        prepared.code.strict = true;
        // Synthetic records have no hoisted declaration prefix or instruction
        // stream; linking initializes their immutable default binding.
        prepared.code.module_evaluate_entry = Some(0);
        Ok(prepared)
    }
    pub(super) fn attach(&mut self, scope: Option<&SchedulingScope>) -> Result<(), RuntimeError> {
        let bytes = self.bytes();
        match (&mut self.charge, scope) {
            (Some(owner), Some(scope)) if !owner.belongs_to(scope) => return Err(scope.fail()),
            (Some(owner), Some(_)) => owner.resize(bytes)?,
            (Some(owner), None) => return Err(owner.fail()),
            (None, Some(scope)) => self.charge = Some(scope.shared_charge(bytes)?),
            (None, None) => {}
        }
        if let (Some(value), Some(scope)) = (&self.code.synthetic_default_export, scope) {
            value.attach(scope)?;
        }
        if let Some(scope) = scope {
            self.code
                .attach_runtime_sources(&scope.heap_native_budget())?;
        }
        Ok(())
    }
    pub(super) fn prepare_key(
        &mut self,
        key: &str,
        header: usize,
        scope: Option<&SchedulingScope>,
    ) -> Result<(), RuntimeError> {
        debug_assert!(self.key.is_empty());
        self.resize(
            self.bytes()
                .saturating_add(header)
                .saturating_add(key.len()),
        )?;
        self.key = module_referrers::copy_referrer(key, scope)?;
        self.resize(self.bytes().saturating_add(header))
    }
    pub(super) fn bytes(&self) -> usize {
        owned_code_bytes(&self.code).saturating_add(self.key.capacity())
    }
    // Host fixture construction has its own admission boundary. Runtime
    // producers transfer this owner directly into ModuleRegistryDraft.
    #[cfg(any(test, coverage))]
    pub(super) fn into_unleased(self) -> Bytecode {
        let Self { code, key, charge } = self;
        drop(key);
        drop(charge);
        code
    }
}
