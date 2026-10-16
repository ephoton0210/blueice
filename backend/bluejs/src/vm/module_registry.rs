// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

//! Compiled module snapshots retain admitted root buffers.

use super::host_scheduling::{
    cloned_code_bytes, map_slots, owned_code_bytes, SharedSchedulingCharge,
};
use super::*;

#[cfg(any(test, coverage))]
#[path = "../../tests/fixtures/module_registry_ownership.rs"]
mod ownership_tests;

struct RegistryOwner {
    data: HashMap<String, Bytecode>,
    bytes: usize,
    charge: RefCell<Option<SharedSchedulingCharge>>,
}
enum RegistryState {
    Empty(HashMap<String, Bytecode>),
    Shared(Rc<RegistryOwner>),
}
pub(super) struct ModuleRegistry(RegistryState);
impl Default for ModuleRegistry {
    fn default() -> Self {
        Self(RegistryState::Empty(HashMap::new()))
    }
}
impl Clone for ModuleRegistry {
    fn clone(&self) -> Self {
        match &self.0 {
            RegistryState::Empty(_) => Self::default(),
            RegistryState::Shared(owner) => Self(RegistryState::Shared(owner.clone())),
        }
    }
}
impl std::ops::Deref for ModuleRegistry {
    type Target = HashMap<String, Bytecode>;
    fn deref(&self) -> &Self::Target {
        match &self.0 {
            RegistryState::Empty(data) => data,
            RegistryState::Shared(owner) => &owner.data,
        }
    }
}
fn payload_bytes(data: &HashMap<String, Bytecode>) -> usize {
    data.iter().fold(0usize, |bytes, (name, code)| {
        bytes
            .saturating_add(name.capacity())
            .saturating_add(owned_code_bytes(code))
    })
}
fn header_bytes() -> usize {
    std::mem::size_of::<RegistryOwner>()
}
fn retained_bytes(data: &HashMap<String, Bytecode>, payload: usize) -> usize {
    if data.capacity() == 0 {
        0
    } else {
        header_bytes()
            .saturating_add(map_slots(data))
            .saturating_add(payload)
    }
}
fn fail(scope: Option<&SchedulingScope>) -> RuntimeError {
    scope.map_or(RuntimeError::SchedulingLimit, SchedulingScope::fail)
}
fn resize(charge: &mut Option<SharedSchedulingCharge>, bytes: usize) -> Result<(), RuntimeError> {
    if let Some(charge) = charge {
        charge.resize(bytes)?;
    }
    Ok(())
}
fn merge(
    owner: &mut Option<SharedSchedulingCharge>,
    other: Option<SharedSchedulingCharge>,
) -> Result<(), RuntimeError> {
    if let Some(other) = other {
        if let Some(owner) = owner {
            owner.absorb(other)?;
        } else {
            *owner = Some(other);
        }
    }
    Ok(())
}
impl ModuleRegistry {
    pub(super) fn new(
        data: HashMap<String, Bytecode>,
        scope: Option<&SchedulingScope>,
    ) -> Result<Self, RuntimeError> {
        if let Some(scope) = scope {
            Vm::attach_synthetic_values(&data, scope)?;
        }
        if data.capacity() == 0 {
            return Ok(Self::default());
        }
        let bytes = retained_bytes(&data, payload_bytes(&data));
        let charge = scope.map(|scope| scope.shared_charge(bytes)).transpose()?;
        Ok(Self(RegistryState::Shared(Rc::new(RegistryOwner {
            data,
            bytes,
            charge: RefCell::new(charge),
        }))))
    }
    pub(super) fn retained_bytes(&self) -> usize {
        match &self.0 {
            RegistryState::Empty(_) => 0,
            RegistryState::Shared(owner) => owner.bytes,
        }
    }
    pub(super) fn attach(&self, scope: &SchedulingScope) -> Result<(), RuntimeError> {
        if let RegistryState::Shared(owner) = &self.0 {
            let mut charge = owner.charge.borrow_mut();
            if let Some(charge) = charge.as_ref() {
                if !charge.belongs_to(scope) {
                    scope.fail();
                    return Err(RuntimeError::TypeError(
                        "compiled registry already has a document owner".into(),
                    ));
                }
            } else {
                *charge = Some(scope.shared_charge(self.retained_bytes())?);
            }
        }
        Vm::attach_synthetic_values(self, scope)
    }
    pub(super) fn copy_host(
        data: &HashMap<String, Bytecode>,
        scope: Option<&SchedulingScope>,
    ) -> Result<Self, RuntimeError> {
        if let Some(scope) = scope {
            Vm::attach_synthetic_values(data, scope)?;
        }
        Ok(RegistryBuilder::copy(data, scope, false)?.finish())
    }
    pub(super) fn for_shadow_realm(
        &self,
        scope: Option<&SchedulingScope>,
    ) -> Result<Self, RuntimeError> {
        if let Some(scope) = scope {
            self.attach(scope)?;
        }
        if !self.values().any(|code| {
            code.synthetic_default_export
                .as_ref()
                .is_some_and(|value| matches!(value.value(), Value::Object(_)))
        }) {
            return Ok(self.clone());
        }
        Ok(RegistryBuilder::copy(self, scope, true)?.finish())
    }
}
struct RegistryBuilder {
    data: HashMap<String, Bytecode>,
    payload: usize,
    charge: Option<SharedSchedulingCharge>,
}
impl RegistryBuilder {
    fn copy(
        data: &HashMap<String, Bytecode>,
        scope: Option<&SchedulingScope>,
        filter_objects: bool,
    ) -> Result<Self, RuntimeError> {
        let include = |code: &Bytecode| {
            !filter_objects
                || !code
                    .synthetic_default_export
                    .as_ref()
                    .is_some_and(|value| matches!(value.value(), Value::Object(_)))
        };
        let count = data.values().filter(|code| include(code)).count();
        if count == 0 {
            return Ok(Self {
                data: HashMap::new(),
                payload: 0,
                charge: None,
            });
        }
        let hint =
            data.iter()
                .filter(|(_, code)| include(code))
                .fold(0usize, |bytes, (name, code)| {
                    bytes
                        .saturating_add(name.len())
                        .saturating_add(cloned_code_bytes(code))
                });
        let minimum = header_bytes()
            .saturating_add(hint)
            .saturating_add(count.saturating_mul(std::mem::size_of::<(String, Bytecode)>()));
        let mut result = Self {
            data: HashMap::new(),
            payload: 0,
            charge: scope
                .map(|scope| scope.shared_charge(minimum))
                .transpose()?,
        };
        result.data.try_reserve(count).map_err(|_| fail(scope))?;
        let table = map_slots(&result.data);
        resize(
            &mut result.charge,
            header_bytes().saturating_add(table).saturating_add(hint),
        )?;
        let mut remaining = hint;
        for (name, code) in data.iter().filter(|(_, code)| include(code)) {
            remaining -= name.len().saturating_add(cloned_code_bytes(code));
            let key = module_referrers::copy_referrer(name, scope)?;
            resize(
                &mut result.charge,
                header_bytes()
                    .saturating_add(table)
                    .saturating_add(result.payload)
                    .saturating_add(remaining)
                    .saturating_add(key.capacity())
                    .saturating_add(cloned_code_bytes(code)),
            )?;
            let code = code.clone();
            result.payload = result
                .payload
                .saturating_add(key.capacity())
                .saturating_add(owned_code_bytes(&code));
            resize(
                &mut result.charge,
                header_bytes()
                    .saturating_add(table)
                    .saturating_add(result.payload)
                    .saturating_add(remaining),
            )?;
            result.data.insert(key, code);
        }
        Ok(result)
    }
    fn insert(
        &mut self,
        key: String,
        code: Bytecode,
        scope: Option<&SchedulingScope>,
        incoming: Option<SharedSchedulingCharge>,
    ) -> Result<(), RuntimeError> {
        let code_bytes = owned_code_bytes(&code);
        let key_bytes = key.capacity();
        let old = self.data.get(&key).map(owned_code_bytes);
        let required = self.data.len().saturating_add(usize::from(old.is_none()));
        let mut replacement_charge = None;
        if required > self.data.capacity() {
            let minimum = required.saturating_mul(std::mem::size_of::<(String, Bytecode)>());
            replacement_charge = scope
                .map(|scope| scope.shared_charge(minimum))
                .transpose()?;
            let mut table = HashMap::new();
            table.try_reserve(required).map_err(|_| fail(scope))?;
            resize(&mut replacement_charge, map_slots(&table))?;
            table.extend(self.data.drain());
            self.data = table;
        }
        self.payload = self
            .payload
            .saturating_sub(old.unwrap_or(0))
            .saturating_add(code_bytes)
            .saturating_add(if old.is_none() { key_bytes } else { 0 });
        drop(self.data.insert(key, code));
        merge(&mut self.charge, incoming)?;
        merge(&mut self.charge, replacement_charge)?;
        resize(&mut self.charge, retained_bytes(&self.data, self.payload))
    }
    fn finish(self) -> ModuleRegistry {
        if self.data.capacity() == 0 {
            return ModuleRegistry::default();
        }
        let bytes = retained_bytes(&self.data, self.payload);
        ModuleRegistry(RegistryState::Shared(Rc::new(RegistryOwner {
            data: self.data,
            bytes,
            charge: RefCell::new(self.charge),
        })))
    }
}
pub(super) struct ModuleRegistryDraft {
    source: ModuleRegistry,
    builder: Option<RegistryBuilder>,
}
impl std::ops::Deref for ModuleRegistryDraft {
    type Target = HashMap<String, Bytecode>;
    fn deref(&self) -> &Self::Target {
        self.builder
            .as_ref()
            .map_or(&self.source, |builder| &builder.data)
    }
}
impl ModuleRegistryDraft {
    pub(super) fn new(source: ModuleRegistry) -> Self {
        Self {
            source,
            builder: None,
        }
    }
    pub(super) fn insert(
        &mut self,
        key: &str,
        code: Bytecode,
        scope: Option<&SchedulingScope>,
    ) -> Result<(), RuntimeError> {
        if let Some(scope) = scope {
            self.source.attach(scope)?;
            if let Some(builder) = &mut self.builder {
                if let Some(charge) = &builder.charge {
                    if !charge.belongs_to(scope) {
                        return Err(scope.fail());
                    }
                } else if builder.data.capacity() != 0 {
                    builder.charge =
                        Some(scope.shared_charge(retained_bytes(&builder.data, builder.payload))?);
                }
            }
        } else if let Some(charge) = self
            .builder
            .as_ref()
            .and_then(|builder| builder.charge.as_ref())
        {
            return Err(charge.fail());
        }
        let has_header = self
            .builder
            .as_ref()
            .map_or(!self.source.is_empty(), |builder| {
                builder.data.capacity() != 0
            });
        let overhead =
            owned_code_bytes(&code).saturating_add(if has_header { 0 } else { header_bytes() });
        let mut incoming = scope
            .map(|scope| scope.shared_charge(key.len().saturating_add(overhead)))
            .transpose()?;
        let key = module_referrers::copy_referrer(key, scope)?;
        resize(&mut incoming, key.capacity().saturating_add(overhead))?;
        if self.builder.is_none() {
            self.builder = Some(RegistryBuilder::copy(&self.source, scope, false)?);
        }
        self.builder
            .as_mut()
            .unwrap()
            .insert(key, code, scope, incoming)
    }
    pub(super) fn finish(self) -> ModuleRegistry {
        self.builder.map_or(self.source, RegistryBuilder::finish)
    }
}
impl Vm {
    pub(super) fn install_compiled_module_context(
        &mut self,
        referrer: String,
        data: HashMap<String, Bytecode>,
    ) -> Result<(), RuntimeError> {
        self.reconcile_scheduling_ownership();
        if data.capacity() == 0 && referrer.capacity() == 0 {
            self.module_registry = ModuleRegistry::default();
            self.active_module_name = Some(referrer);
            self.reconcile_scheduling_ownership();
            return Ok(());
        }
        let name_charge = self
            .host_scheduling
            .as_ref()
            .map(|scope| scope.shared_charge(referrer.capacity()))
            .transpose()?;
        let registry = ModuleRegistry::new(data, self.host_scheduling.as_ref())?;
        self.module_registry = registry;
        self.active_module_name = Some(referrer);
        drop(name_charge);
        self.reconcile_scheduling_ownership();
        Ok(())
    }
}
