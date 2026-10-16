// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

//! Realm-global records retain native ownership across script and realm entry.

use super::host_scheduling::value_payload_bytes;
use super::*;

#[cfg(any(test, coverage))]
#[path = "../../tests/fixtures/global_binding_admission.rs"]
mod global_binding_tests;

enum Records<'a> {
    Code {
        global: ObjectId,
        code: &'a Bytecode,
        eval: bool,
    },
    Name(&'a str),
}

impl<'a> Records<'a> {
    fn visit(&self, vm: &Vm, mut visit: impl FnMut(Option<usize>, &'a str, bool)) {
        match *self {
            Self::Name(name) => visit(None, name, true),
            Self::Code { global, code, eval } => {
                for &slot in code.scopes.first().map(Vec::as_slice).unwrap_or_default() {
                    let slot = slot as usize;
                    let binding = &code.bindings[slot];
                    if eval && binding.lexical {
                        continue;
                    }
                    let new = !vm.global_bindings.contains_key(&binding.name);
                    if new
                        && vm.global_var_is_accessor(
                            global,
                            binding,
                            code.global_function_names.contains(&binding.name),
                        )
                    {
                        continue;
                    }
                    if new || vm.script_global_slots.get(&slot) != Some(&binding.name) {
                        visit(Some(slot), &binding.name, new);
                    }
                }
            }
        }
    }
}

struct GlobalRecord {
    slot: Option<usize>,
    key: Option<String>,
    alias: Option<String>,
    initial: Value,
    existed: bool,
    configurable: bool,
}

pub(super) struct PreparedGlobalValue {
    pub(super) initial: Value,
    pub(super) existed: bool,
    pub(super) configurable: bool,
    pub(super) charge: execution_payload::ExecutionPayloadCharge,
}

pub(super) struct PreparedGlobalRecords {
    rows: Vec<GlobalRecord>,
    globals: Option<SchedulingMapCapacity<String, GlobalBinding>>,
    aliases: Option<SchedulingMapCapacity<usize, String>>,
    charge: execution_payload::ExecutionPayloadCharge,
    payload: execution_payload::ExecutionPayloadCharge,
}

impl PreparedGlobalRecords {
    fn refresh(&mut self) {
        let bytes = self
            .rows
            .iter()
            .fold(host_scheduling::slots(&self.rows), |bytes, row| {
                bytes
                    .saturating_add(row.key.as_ref().map_or(0, String::capacity))
                    .saturating_add(row.alias.as_ref().map_or(0, String::capacity))
            })
            .saturating_add(
                self.globals
                    .as_ref()
                    .map_or(0, SchedulingMapCapacity::staging_bytes),
            )
            .saturating_add(
                self.aliases
                    .as_ref()
                    .map_or(0, SchedulingMapCapacity::staging_bytes),
            );
        self.charge
            .resize(if self.charge.enabled() { bytes } else { 0 });
        let payload = self.rows.iter().fold(0usize, |bytes, row| {
            bytes.saturating_add(value_payload_bytes(&row.initial))
        });
        self.payload
            .resize(if self.payload.enabled() { payload } else { 0 });
    }

    fn stage_value(
        &mut self,
        vm: &mut Vm,
        index: usize,
        global: ObjectId,
        binding: &Binding,
        function: bool,
    ) -> Result<(), RuntimeError> {
        let mut descriptor = vm
            .heap
            .get_own_property_descriptor(global, binding.name.as_str())?;
        let row = &mut self.rows[index];
        let old_payload = value_payload_bytes(&row.initial);
        row.existed = descriptor.is_some();
        row.configurable = descriptor
            .as_ref()
            .is_some_and(|descriptor| descriptor.configurable == Some(true));
        if !binding.lexical && !function {
            row.initial = descriptor
                .as_mut()
                .and_then(|descriptor| descriptor.value.take())
                .unwrap_or(Value::Undefined);
        }
        drop(descriptor);
        if self.payload.enabled() {
            self.payload
                .resize(self.payload.bytes() - old_payload + value_payload_bytes(&row.initial));
        }
        vm.synchronize_scheduling_usage()
    }

    fn take_value(&mut self, vm: &Vm, index: usize) -> PreparedGlobalValue {
        let row = &mut self.rows[index];
        let initial = std::mem::replace(&mut row.initial, Value::Undefined);
        let value = PreparedGlobalValue {
            existed: row.existed,
            configurable: row.configurable,
            charge: vm.charge_execution_payload(value_payload_bytes(&initial)),
            initial,
        };
        if self.payload.enabled() {
            self.payload
                .resize(self.payload.bytes() - value_payload_bytes(&value.initial));
        }
        value
    }

    fn release_names(&mut self, bytes: usize) {
        if self.charge.enabled() {
            self.charge.resize(self.charge.bytes() - bytes);
        }
    }

    fn install(&mut self, vm: &mut Vm) -> Result<(), RuntimeError> {
        vm.synchronize_scheduling_usage()?;
        let tables = self.globals.as_ref().unwrap().staging_bytes()
            + self.aliases.as_ref().unwrap().staging_bytes();
        self.globals.take().unwrap().commit(&mut vm.global_bindings);
        self.aliases
            .take()
            .unwrap()
            .commit(&mut vm.script_global_slots);
        self.release_names(tables);
        vm.synchronize_scheduling_usage()
    }
}

pub(super) struct SuspendedGlobalBindings {
    bindings: HashMap<String, GlobalBinding>,
    charge: execution_payload::ExecutionPayloadCharge,
}

impl SuspendedGlobalBindings {
    pub(super) fn restore(self, vm: &mut Vm) {
        vm.reserved_global_binding_slots -= self.bindings.len();
        debug_assert!(
            vm.global_bindings.len() + self.bindings.len() <= vm.global_bindings.capacity()
        );
        for (name, binding) in self.bindings {
            vm.publish_global_record(name, binding);
        }
        drop(self.charge);
        vm.reconcile_scheduling_ownership();
    }
}

fn storage_bytes(bindings: &HashMap<String, GlobalBinding>) -> usize {
    bindings
        .keys()
        .fold(host_scheduling::map_slots(bindings), |bytes, name| {
            bytes.saturating_add(name.capacity())
        })
}

fn minimum_table_bytes<K, V>(table: &HashMap<K, V>, added: usize) -> usize {
    let required = table.len().saturating_add(added);
    if required > table.capacity() {
        required.saturating_mul(std::mem::size_of::<(K, V)>())
    } else {
        0
    }
}

impl Vm {
    pub(super) fn global_binding_storage_bytes(&self) -> usize {
        storage_bytes(&self.global_bindings)
    }

    fn prepare_global_records(
        &mut self,
        records: Records<'_>,
    ) -> Result<PreparedGlobalRecords, RuntimeError> {
        let result = self.prepare_global_record_storage(records);
        if result.is_err() {
            self.reconcile_scheduling_ownership();
        }
        result
    }

    fn prepare_global_record_storage(
        &mut self,
        records: Records<'_>,
    ) -> Result<PreparedGlobalRecords, RuntimeError> {
        let mut count = 0usize;
        let mut globals = self.reserved_global_binding_slots;
        let mut aliases = 0usize;
        let mut names = 0usize;
        records.visit(self, |slot, name, key| {
            count += 1;
            globals += usize::from(key && !self.global_bindings.contains_key(name));
            let alias = slot.is_some_and(|slot| {
                self.script_global_slots.get(&slot).map(String::as_str) != Some(name)
            });
            aliases += usize::from(alias && !self.script_global_slots.contains_key(&slot.unwrap()));
            names = names.saturating_add(
                name.len()
                    .saturating_mul(usize::from(key) + usize::from(alias)),
            );
        });
        self.admit_scheduling_work(HostSchedulingUsage {
            payload_bytes: names
                .saturating_add(count.saturating_mul(std::mem::size_of::<GlobalRecord>()))
                .saturating_add(minimum_table_bytes(&self.global_bindings, globals))
                .saturating_add(minimum_table_bytes(&self.script_global_slots, aliases)),
            ..Default::default()
        })?;
        let globals = SchedulingMapCapacity::for_additional(
            &self.global_bindings,
            globals,
            self.host_scheduling.as_ref(),
        )?;
        let aliases = SchedulingMapCapacity::for_additional(
            &self.script_global_slots,
            aliases,
            self.host_scheduling.as_ref(),
        )?;
        let mut rows = Vec::with_capacity(count);
        records.visit(self, |slot, name, key| {
            rows.push(GlobalRecord {
                slot,
                key: key.then(|| name.to_owned()),
                alias: slot
                    .filter(|slot| {
                        self.script_global_slots.get(slot).map(String::as_str) != Some(name)
                    })
                    .map(|_| name.to_owned()),
                initial: Value::Undefined,
                existed: false,
                configurable: false,
            });
        });
        let mut prepared = PreparedGlobalRecords {
            rows,
            globals: Some(globals),
            aliases: Some(aliases),
            charge: self.charge_execution_control(0),
            payload: self.charge_execution_payload(0),
        };
        prepared.refresh();
        self.synchronize_scheduling_usage()?;
        Ok(prepared)
    }

    pub(super) fn prepare_global_declaration_records(
        &mut self,
        global: ObjectId,
        code: &Bytecode,
        eval: bool,
    ) -> Result<PreparedGlobalRecords, RuntimeError> {
        self.prepare_global_records(Records::Code { global, code, eval })
    }

    pub(super) fn publish_global_declaration_records(
        &mut self,
        global: ObjectId,
        code: &Bytecode,
        eval: bool,
        mut prepared: PreparedGlobalRecords,
    ) -> Result<(), RuntimeError> {
        // Resolve lazy globals during validation first, then own all initial
        // property values before table installation or the first new cell.
        for index in 0..prepared.rows.len() {
            if prepared.rows[index].key.is_some() {
                let binding = &code.bindings[prepared.rows[index].slot.unwrap()];
                prepared.stage_value(
                    self,
                    index,
                    global,
                    binding,
                    code.global_function_names.contains(&binding.name),
                )?;
            }
        }
        prepared.install(self)?;
        for index in 0..prepared.rows.len() {
            let slot = prepared.rows[index].slot.unwrap();
            let names = prepared.rows[index]
                .key
                .as_ref()
                .map_or(0, String::capacity)
                + prepared.rows[index]
                    .alias
                    .as_ref()
                    .map_or(0, String::capacity);
            if prepared.rows[index].key.is_some() {
                let initial = prepared.take_value(self, index);
                let key = prepared.rows[index].key.take().unwrap();
                let binding = &code.bindings[slot];
                let value = self.initialize_global_binding(
                    global,
                    binding,
                    code.global_function_names.contains(&binding.name),
                    eval,
                    initial,
                )?;
                self.publish_global_record(key, value);
            }
            if let Some(alias) = prepared.rows[index].alias.take() {
                self.script_global_slots.insert(slot, alias);
            }
            prepared.release_names(names);
        }
        Ok(())
    }

    fn publish_global_record(&mut self, name: String, binding: GlobalBinding) {
        if let Some(previous) = self.global_bindings.insert(name, binding) {
            self.release_root(previous._root);
        }
    }

    #[cfg(any(test, coverage))]
    pub(super) fn create_global_binding(
        &mut self,
        global: ObjectId,
        binding: &Binding,
        function: bool,
        configurable: bool,
    ) -> Result<(), RuntimeError> {
        let result = (|| {
            let mut prepared = self.prepare_global_records(Records::Name(&binding.name))?;
            prepared.stage_value(self, 0, global, binding, function)?;
            prepared.install(self)?;
            let initial = prepared.take_value(self, 0);
            let names = prepared.rows[0].key.as_ref().unwrap().capacity();
            let value =
                self.initialize_global_binding(global, binding, function, configurable, initial)?;
            self.publish_global_record(prepared.rows[0].key.take().unwrap(), value);
            prepared.release_names(names);
            Ok(())
        })();
        self.reconcile_scheduling_ownership();
        result
    }

    pub(super) fn recreate_global_eval_binding(
        &mut self,
        slot: usize,
        name: &str,
        cell: ObjectId,
        value: Value,
    ) -> Result<(), RuntimeError> {
        debug_assert!(self.script_global_slots.contains_key(&slot));
        let charge = self.charge_execution_payload(value_payload_bytes(&value));
        let result = (|| {
            let mut prepared = self.prepare_global_records(Records::Name(name))?;
            prepared.install(self)?;
            let names = prepared.rows[0].key.as_ref().unwrap().capacity();
            let global = self.globals["globalThis"];
            let root = self.heap.root(cell)?;
            // The cell is private until the ordinary global property succeeds.
            // All failure paths can delete its tentative value without allocation.
            if let Err(error) = self.with_roots(|heap| heap.set(cell, "value", value.clone())) {
                self.release_root(root);
                return Err(error);
            }
            let defined = self.with_roots(|heap| {
                let result = heap.define_own_property(
                    global,
                    name,
                    PropertyDescriptor::data(value, true, true, true),
                );
                drop(charge);
                result
            });
            match defined {
                Ok(true) => {
                    self.publish_global_record(
                        prepared.rows[0].key.take().unwrap(),
                        GlobalBinding {
                            cell,
                            mutable: true,
                            property: true,
                            _root: root,
                        },
                    );
                    prepared.release_names(names);
                    Ok(())
                }
                refused => {
                    self.heap
                        .delete(cell, "value")
                        .expect("tentative eval cell deletion cannot allocate");
                    self.release_root(root);
                    refused.map(|_| ())
                }
            }
        })();
        self.reconcile_scheduling_ownership();
        result
    }

    pub(super) fn suspend_lexical_global_bindings(
        &mut self,
    ) -> Result<SuspendedGlobalBindings, RuntimeError> {
        let result = (|| {
            let count = self
                .global_bindings
                .values()
                .filter(|binding| !binding.property)
                .count();
            self.admit_scheduling_work(HostSchedulingUsage {
                payload_bytes: count.saturating_mul(std::mem::size_of::<(String, GlobalBinding)>()),
                ..Default::default()
            })?;
            let mut bindings = HashMap::new();
            if bindings.try_reserve(count).is_err() {
                return Err(self
                    .host_scheduling
                    .as_ref()
                    .map_or(RuntimeError::SchedulingLimit, SchedulingScope::fail));
            }
            let mut charge = self.charge_execution_control(storage_bytes(&bindings));
            self.synchronize_scheduling_usage()?;
            bindings.extend(
                self.global_bindings
                    .extract_if(|_, binding| !binding.property),
            );
            self.reserved_global_binding_slots += count;
            charge.resize(if charge.enabled() {
                storage_bytes(&bindings)
            } else {
                0
            });
            let saved = SuspendedGlobalBindings { bindings, charge };
            if let Err(error) = self.synchronize_scheduling_usage() {
                saved.restore(self);
                return Err(error);
            }
            Ok(saved)
        })();
        if result.is_err() {
            self.reconcile_scheduling_ownership();
        }
        result
    }
}
