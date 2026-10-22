// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

//! Document-owned copies of immutable host-compiled child code.

use super::compiled_source_storage::{SourceOwner, WeakSource};
use super::host_scheduling::SharedSchedulingCharge;
use super::shared_code_storage::WeakCode;
use super::*;
use std::rc::Weak;
use std::sync::Arc;

struct CachedCode {
    // Preserve original allocation identity even after the host drops its code.
    _original: Weak<()>,
    compiled: WeakCode,
}
pub(crate) struct HostCodeCache {
    code: HashMap<usize, CachedCode>,
    sources: HashMap<usize, WeakSource>,
    // Slots of each table's real allocation, fixed when it is rebuilt. A
    // table's `capacity()` is not a size: deletions leave tombstones that lower
    // it and later insertions raise it again with no reallocation, so the
    // ledger must not follow it.
    code_slots: usize,
    source_slots: usize,
    charge: SharedSchedulingCharge,
}
impl Drop for HostCodeCache {
    fn drop(&mut self) {
        // One live cache per single-threaded document. Do not leave its dead
        // allocation retained by the account's weak discovery pointer.
        self.charge.clear_host_code_cache();
    }
}
fn header_bytes() -> usize {
    std::mem::size_of::<RefCell<HostCodeCache>>()
}
fn grow<V>(
    map: &mut HashMap<usize, V>,
    slots: &mut usize,
    owner: &mut SharedSchedulingCharge,
    key: usize,
    budget: &HeapNativeBudget,
) -> Result<(), RuntimeError> {
    if map.contains_key(&key) || map.len() < map.capacity() {
        return Ok(());
    }
    let slot = std::mem::size_of::<(usize, V)>();
    let count = map.len().checked_add(1).ok_or_else(|| budget.fail())?;
    let minimum = count.checked_mul(slot).ok_or_else(|| budget.fail())?;
    let mut charge = budget.reserve(minimum)?;
    let mut candidate = HashMap::new();
    candidate.try_reserve(count).map_err(|_| budget.fail())?;
    let candidate_slots = candidate.capacity();
    charge.resize(candidate_slots.saturating_mul(slot))?;
    let old = slots.saturating_mul(slot);
    candidate.extend(map.drain());
    *map = candidate;
    *slots = candidate_slots;
    owner.release_to(owner.bytes() - old);
    owner.absorb(charge)?;
    Ok(())
}
fn shrink<V>(map: &mut HashMap<usize, V>, slots: &mut usize, owner: &mut SharedSchedulingCharge) {
    let slot = std::mem::size_of::<(usize, V)>();
    let old_slots = slots.saturating_mul(slot);
    if map.is_empty() {
        *map = HashMap::new();
        *slots = 0;
        owner.release_to(owner.bytes() - old_slots);
        return;
    }
    if *slots <= map.len().saturating_mul(4) {
        return;
    }
    let old = owner.bytes();
    let minimum = map.len().saturating_mul(slot);
    if !owner.try_resize_for_reclamation(old.saturating_add(minimum)) {
        return;
    }
    let mut candidate = HashMap::new();
    if candidate.try_reserve(map.len()).is_err() {
        owner.release_to(old);
        return;
    }
    let candidate_slots = candidate.capacity();
    let admitted = old.saturating_add(candidate_slots.saturating_mul(slot));
    if !owner.try_resize_for_reclamation(admitted) {
        drop(candidate);
        owner.release_to(old);
        return;
    }
    candidate.extend(map.drain());
    *map = candidate;
    *slots = candidate_slots;
    owner.release_to(admitted - old_slots);
}
impl HostCodeCache {
    pub(crate) fn shared(budget: &HeapNativeBudget) -> Result<Rc<RefCell<Self>>, RuntimeError> {
        budget.reserve(0)?;
        if let Some(cache) = budget.host_code_cache() {
            if !budget.owns(&cache.borrow().charge) {
                return Err(budget.fail());
            }
            return Ok(cache);
        }
        let charge = budget.reserve(header_bytes())?;
        let cache = Rc::new(RefCell::new(Self {
            code: HashMap::new(),
            sources: HashMap::new(),
            code_slots: 0,
            source_slots: 0,
            charge,
        }));
        budget.install_host_code_cache(&cache);
        Ok(cache)
    }
    fn source(
        cache: &Rc<RefCell<Self>>,
        text: &Arc<str>,
        budget: &HeapNativeBudget,
    ) -> Result<Rc<SourceOwner>, RuntimeError> {
        let key = text.as_ptr() as usize;
        if let Some(owner) = cache
            .borrow()
            .sources
            .get(&key)
            .and_then(WeakSource::upgrade)
        {
            return Ok(owner);
        }
        let owner = SourceOwner::host(text, budget)?;
        let mut cache = cache.borrow_mut();
        let Self {
            sources,
            source_slots,
            charge,
            ..
        } = &mut *cache;
        grow(sources, source_slots, charge, key, budget)?;
        sources.insert(key, owner.weak());
        Ok(owner)
    }
    pub(crate) fn prepare(
        cache: &Rc<RefCell<Self>>,
        original: &SharedBytecode,
        budget: &HeapNativeBudget,
    ) -> Result<SharedBytecode, RuntimeError> {
        budget.reserve(0)?;
        if !budget.owns(&cache.borrow().charge) {
            return Err(budget.fail());
        }
        original.attach_runtime_sources(budget)?;
        if original.is_runtime() {
            return Ok(original.clone());
        }
        let key = original.host_identity();
        if let Some(code) = cache
            .borrow()
            .code
            .get(&key)
            .and_then(|entry| entry.compiled.upgrade())
        {
            code.attach_runtime_sources(budget)?;
            return Ok(code);
        }
        let code = original.copy_host(budget, |code| Self::prepare_root(cache, code, budget))?;
        let mut cache = cache.borrow_mut();
        let Self {
            code: entries,
            code_slots,
            charge,
            ..
        } = &mut *cache;
        grow(entries, code_slots, charge, key, budget)?;
        entries.insert(
            key,
            CachedCode {
                _original: original.weak_identity(),
                compiled: code.weak(),
            },
        );
        Ok(code)
    }
    pub(crate) fn prepare_root(
        cache: &Rc<RefCell<Self>>,
        code: &mut Bytecode,
        budget: &HeapNativeBudget,
    ) -> Result<(), RuntimeError> {
        budget.reserve(0)?;
        if !budget.owns(&cache.borrow().charge) {
            return Err(budget.fail());
        }
        code.attach_runtime_sources(budget)?;
        let installed = code.has_runtime_sources();
        let source = if installed {
            None
        } else {
            code.source_text
                .allocation()
                .map(|source| Self::source(cache, source, budget))
                .transpose()?
        };
        for child in &mut code.functions {
            *child = Self::prepare(cache, child, budget)?;
        }
        if !installed {
            code.install_host_source(source, budget);
        }
        Ok(())
    }
    pub(crate) fn prune(&mut self) {
        self.code.retain(|_, entry| entry.compiled.live());
        self.sources.retain(|_, source| source.live());
        shrink(&mut self.code, &mut self.code_slots, &mut self.charge);
        shrink(&mut self.sources, &mut self.source_slots, &mut self.charge);
        let bytes = header_bytes()
            .saturating_add(
                self.code_slots
                    .saturating_mul(std::mem::size_of::<(usize, CachedCode)>()),
            )
            .saturating_add(
                self.source_slots
                    .saturating_mul(std::mem::size_of::<(usize, WeakSource)>()),
            );
        self.charge.release_to(bytes);
    }
    pub(crate) fn is_empty(&self) -> bool {
        self.code.is_empty() && self.sources.is_empty()
    }
}

#[cfg(any(test, coverage))]
#[path = "../../tests/fixtures/host_code_storage.rs"]
mod tests;
