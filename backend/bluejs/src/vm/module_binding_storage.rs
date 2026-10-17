// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

//! Module binding producers own admitted scratch through heap publication.

use super::host_scheduling::SharedSchedulingCharge;
use super::module_walk_storage::Storage;
use super::*;

fn fail(scope: Option<&SchedulingScope>) -> RuntimeError {
    scope.map_or(RuntimeError::SchedulingLimit, SchedulingScope::fail)
}
fn live(scope: Option<&SchedulingScope>) -> Result<(), RuntimeError> {
    if let Some(scope) = scope {
        scope.shared_charge(0)?;
    }
    Ok(())
}
fn import_count(code: &Bytecode) -> usize {
    code.module_imports
        .iter()
        .filter(|i| i.local_slot.is_some())
        .count()
}

pub(super) struct ImportedSlots {
    slots: Storage<usize>,
}
impl ImportedSlots {
    pub(super) fn prepare(
        code: &Bytecode,
        scope: Option<&SchedulingScope>,
    ) -> Result<Self, RuntimeError> {
        live(scope)?;
        let mut slots = Storage::default();
        slots.reserve(import_count(code), scope)?;
        slots.data.extend(
            code.module_imports
                .iter()
                .filter_map(|i| i.local_slot.map(|s| s as usize)),
        );
        slots.data.sort_unstable();
        slots.data.dedup();
        Ok(Self { slots })
    }
    pub(super) fn contains(&self, slot: usize) -> bool {
        self.slots.data.binary_search(&slot).is_ok()
    }
    #[cfg(any(test, coverage))]
    pub(super) fn bytes(&self) -> usize {
        self.slots.bytes()
    }
}

pub(super) struct Aliases {
    data: Storage<(usize, ObjectId)>,
    expected: usize,
}
impl Aliases {
    pub(super) fn prepare(
        code: &Bytecode,
        scope: Option<&SchedulingScope>,
    ) -> Result<Self, RuntimeError> {
        live(scope)?;
        let expected = import_count(code);
        let mut data = Storage::default();
        data.reserve(expected, scope)?;
        Ok(Self { data, expected })
    }
    pub(super) fn push(&mut self, slot: usize, cell: ObjectId) {
        assert!(self.data.data.len() < self.expected);
        assert!(self.data.data.len() < self.data.data.capacity());
        self.data.data.push((slot, cell));
    }
    pub(super) fn publish(mut self, cells: &mut HashMap<usize, ObjectId>) {
        assert_eq!(self.data.data.len(), self.expected);
        let needed = self
            .data
            .data
            .iter()
            .filter(|(slot, _)| !cells.contains_key(slot))
            .count();
        assert!(
            needed <= cells.capacity() - cells.len(),
            "linked cell backing precedes alias publication"
        );
        for (slot, cell) in self.data.data.drain(..) {
            cells.insert(slot, cell);
        }
    }
    #[cfg(any(test, coverage))]
    pub(super) fn bytes(&self) -> usize {
        self.data.bytes()
    }
}

pub(super) struct NamespaceExports {
    expected: usize,
    names: Vec<JsString>,
    exports: Vec<(JsString, ObjectId)>,
    name_bytes: usize,
    next: usize,
    charge: Option<SharedSchedulingCharge>,
}
impl NamespaceExports {
    pub(super) fn prepare(
        source: &[&str],
        scope: Option<&SchedulingScope>,
    ) -> Result<Self, RuntimeError> {
        let mut pending = source.iter().try_fold(0usize, |bytes, name| {
            name.encode_utf16()
                .count()
                .checked_mul(std::mem::size_of::<u16>())
                .and_then(|n| bytes.checked_add(n))
                .ok_or_else(|| fail(scope))
        })?;
        let output = source
            .len()
            .checked_mul(std::mem::size_of::<(JsString, ObjectId)>())
            .ok_or_else(|| fail(scope))?;
        let minimum = source
            .len()
            .checked_mul(std::mem::size_of::<JsString>())
            .and_then(|bytes| bytes.checked_add(output))
            .and_then(|bytes| bytes.checked_add(pending))
            .ok_or_else(|| fail(scope))?;
        let mut prepared = Self {
            expected: source.len(),
            names: Vec::new(),
            exports: Vec::new(),
            name_bytes: 0,
            next: 0,
            charge: scope
                .map(|scope| scope.shared_charge(minimum))
                .transpose()?,
        };
        prepared
            .names
            .try_reserve_exact(source.len())
            .map_err(|_| fail(scope))?;
        prepared.refresh(pending.saturating_add(output))?;
        prepared
            .exports
            .try_reserve_exact(source.len())
            .map_err(|_| fail(scope))?;
        prepared.refresh(pending)?;
        for name in source {
            let count = name.encode_utf16().count();
            let mut units = Vec::new();
            units.try_reserve_exact(count).map_err(|_| fail(scope))?;
            units.extend(name.encode_utf16());
            let name = JsString::from_code_units(units);
            prepared.name_bytes = prepared
                .name_bytes
                .saturating_add(name.owned_heap_payload_bytes().unwrap_or(usize::MAX));
            prepared.names.push(name);
            pending -= count * std::mem::size_of::<u16>();
            prepared.refresh(pending)?;
        }
        Ok(prepared)
    }
    fn current_bytes(&self) -> usize {
        host_scheduling::slots(&self.names)
            .saturating_add(host_scheduling::slots(&self.exports))
            .saturating_add(self.name_bytes)
    }
    fn refresh(&mut self, pending: usize) -> Result<(), RuntimeError> {
        let bytes = self.current_bytes().saturating_add(pending);
        if let Some(charge) = &mut self.charge {
            charge.resize(bytes)?;
        }
        Ok(())
    }
    fn next_name(&mut self) -> JsString {
        assert!(self.next < self.expected);
        let name = std::mem::take(&mut self.names[self.next]);
        self.next += 1;
        name
    }
    pub(super) fn keep_next(&mut self, cell: ObjectId) {
        assert!(self.exports.len() < self.exports.capacity());
        let name = self.next_name();
        self.exports.push((name, cell));
    }
    pub(super) fn skip_next(&mut self) {
        let name = self.next_name();
        let released = name.owned_heap_payload_bytes().unwrap_or(usize::MAX);
        drop(name);
        self.name_bytes -= released;
        let bytes = self.current_bytes();
        if let Some(charge) = &mut self.charge {
            charge.release_to(bytes);
        }
    }
    pub(super) fn publish(self, vm: &mut Vm, namespace: ObjectId) -> Result<(), RuntimeError> {
        assert_eq!(self.next, self.expected);
        let Self {
            names,
            exports,
            mut charge,
            name_bytes,
            ..
        } = self;
        drop(names);
        let bytes = crate::heap::namespace_export_bytes(&exports);
        assert_eq!(
            bytes,
            host_scheduling::slots(&exports).saturating_add(name_bytes)
        );
        if let Some(charge) = &mut charge {
            charge.release_to(bytes);
        }
        let mut ready = ReadyExports { exports, charge };
        ready.attach(vm.host_scheduling.as_ref())?;
        vm.with_roots(move |heap| ready.publish(heap, namespace))
    }
    #[cfg(any(test, coverage))]
    pub(super) fn bytes(&self) -> usize {
        self.current_bytes()
    }
    #[cfg(any(test, coverage))]
    pub(super) fn payload_bytes(&self) -> usize {
        crate::heap::namespace_export_bytes(&self.exports)
    }
}

struct ReadyExports {
    exports: Vec<(JsString, ObjectId)>,
    charge: Option<SharedSchedulingCharge>,
}
impl ReadyExports {
    fn attach(&mut self, scope: Option<&SchedulingScope>) -> Result<(), RuntimeError> {
        live(scope)?;
        match (self.charge.as_ref(), scope) {
            (Some(owner), Some(scope)) if !owner.belongs_to(scope) => Err(scope.fail()),
            (Some(owner), None) => Err(owner.fail()),
            (None, Some(scope)) => {
                self.charge =
                    Some(scope.shared_charge(crate::heap::namespace_export_bytes(&self.exports))?);
                Ok(())
            }
            _ => Ok(()),
        }
    }
    fn publish(self, heap: &mut Heap, namespace: ObjectId) -> Result<(), crate::heap::HeapError> {
        let Self { exports, charge } = self;
        let result = heap.initialize_module_namespace(namespace, exports);
        drop(charge);
        result
    }
}
