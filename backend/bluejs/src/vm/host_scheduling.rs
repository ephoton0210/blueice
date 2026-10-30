// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

//! Page-owned scheduling admission shared by a realm and its ShadowRealms.
//! Standalone ECMAScript VMs retain their unbounded host-neutral job policy.

use super::*;
use std::cell::{Cell, RefCell};

#[cfg(any(test, coverage))]
#[path = "../../tests/fixtures/task_payload_handoff.rs"]
mod task_payload_handoff;

#[cfg(any(test, coverage))]
#[path = "../../tests/fixtures/document_timer_counts.rs"]
mod document_timer_counts;

#[cfg(any(test, coverage))]
#[path = "../../tests/fixtures/scheduling_capacity.rs"]
mod capacity_tests;

#[cfg(any(test, coverage))]
#[path = "../../tests/fixtures/browser_task_capacity.rs"]
mod browser_task_capacity;

#[cfg(any(test, coverage))]
#[path = "../../tests/fixtures/promise_table_capacity.rs"]
mod promise_table_capacity;

#[cfg(any(test, coverage))]
#[path = "../../tests/fixtures/import_waiter_capacity.rs"]
mod import_waiter_capacity;

pub const MAX_PENDING_HOST_MICROTASKS: usize = 1024;
pub const MAX_PENDING_HOST_REACTIONS: usize = 1024;
pub const MAX_PENDING_HOST_TIMERS: usize = 1024;
pub const MAX_PENDING_BROWSER_TASKS: usize = 64;
pub const MAX_HOST_SCHEDULING_BYTES: usize = 8 * 1024 * 1024;

/// Planned slot storage for one append. Explicit reservation makes the
/// capacity admitted before publication match the requested allocation.
#[derive(Clone, Copy)]
pub(super) struct SchedulingCapacity {
    capacity: usize,
    additional_bytes: usize,
}

impl SchedulingCapacity {
    fn for_push<T>(len: usize, capacity: usize) -> Self {
        let required = len.saturating_add(1);
        let next = if required <= capacity {
            capacity
        } else {
            required.max(capacity.saturating_mul(2)).max(4)
        };
        Self {
            capacity: next,
            additional_bytes: next
                .saturating_sub(capacity)
                .saturating_mul(std::mem::size_of::<T>()),
        }
    }

    pub(super) fn for_vec_push<T>(values: &Vec<T>) -> Self {
        Self::for_push::<T>(values.len(), values.capacity())
    }

    /// True requires another admission check before publishing the entry:
    /// reserve_exact permits an allocator to report additional spare slots.
    pub(super) fn reserve_vec<T>(self, values: &mut Vec<T>) -> bool {
        if values.capacity() < self.capacity {
            values.reserve_exact(self.capacity.saturating_sub(values.len()));
        }
        values.capacity() > self.capacity
    }
}

/// Prepare actual hash-table capacity without changing live owners. The
/// replacement receives existing entries by move only after joint admission.
pub(super) struct SchedulingMapCapacity<K, V, S = std::collections::hash_map::RandomState> {
    replacement: Option<HashMap<K, V, S>>,
    additional_bytes: usize,
}

impl<K: Eq + std::hash::Hash, V, S: std::hash::BuildHasher + Default>
    SchedulingMapCapacity<K, V, S>
{
    pub(super) fn for_additional(
        values: &HashMap<K, V, S>,
        additional: usize,
        scope: Option<&SchedulingScope>,
    ) -> Result<Self, RuntimeError> {
        let required = values.len().saturating_add(additional);
        let replacement = if required > values.capacity() {
            let mut table = HashMap::with_hasher(S::default());
            if table.try_reserve(required).is_err() {
                return Err(scope.map_or(RuntimeError::SchedulingLimit, SchedulingScope::fail));
            }
            Some(table)
        } else {
            None
        };
        let capacity = replacement
            .as_ref()
            .map_or(values.capacity(), HashMap::capacity);
        Ok(Self {
            replacement,
            additional_bytes: capacity
                .saturating_sub(values.capacity())
                .saturating_mul(std::mem::size_of::<(K, V)>()),
        })
    }

    pub(super) fn commit(self, values: &mut HashMap<K, V, S>) {
        if let Some(mut table) = self.replacement {
            table.extend(values.drain());
            *values = table;
        }
    }

    /// A detached replacement coexists with the live table until commit.
    pub(super) fn staging_bytes(&self) -> usize {
        self.replacement.as_ref().map_or(0, map_slots)
    }
}

/// Await publishes a continuation and its only resumption owner together.
pub(super) enum AwaitRegistration {
    Reaction {
        promise: ObjectId,
        reaction: PromiseReaction,
    },
    Job(PromiseJob),
}

/// Owned scheduling slot capacities and native payloads (including the
/// bounded console buffer, execution payloads and shared agent registries), separate from managed heap
/// and shared installed bytecode. Allocator/control-table overhead, spare
/// BigInt limb capacity and process RSS are not represented by this ledger.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct HostSchedulingUsage {
    /// Browser and cleanup tasks; page snapshots include their ShadowRealms.
    pub pending_browser_tasks: usize,
    /// Callback timer registrations, including same-document ShadowRealms.
    /// A running interval keeps its registration; a firing one-shot does not.
    pub pending_timers: usize,
    pub pending_microtasks: usize,
    pub pending_reactions: usize,
    pub payload_bytes: usize,
}

impl HostSchedulingUsage {
    pub(super) fn with_map_growth<K, V, S>(
        mut self,
        growth: &SchedulingMapCapacity<K, V, S>,
    ) -> Self {
        self.payload_bytes = self.payload_bytes.saturating_add(growth.additional_bytes);
        self
    }

    pub(super) fn with_growth(mut self, growth: SchedulingCapacity) -> Self {
        self.payload_bytes = self.payload_bytes.saturating_add(growth.additional_bytes);
        self
    }

    fn add(self, other: Self) -> Self {
        Self {
            pending_browser_tasks: self
                .pending_browser_tasks
                .saturating_add(other.pending_browser_tasks),
            pending_timers: self.pending_timers.saturating_add(other.pending_timers),
            pending_microtasks: self
                .pending_microtasks
                .saturating_add(other.pending_microtasks),
            pending_reactions: self
                .pending_reactions
                .saturating_add(other.pending_reactions),
            payload_bytes: self.payload_bytes.saturating_add(other.payload_bytes),
        }
    }
    fn sub(self, other: Self) -> Self {
        Self {
            pending_browser_tasks: self
                .pending_browser_tasks
                .saturating_sub(other.pending_browser_tasks),
            pending_timers: self.pending_timers.saturating_sub(other.pending_timers),
            pending_microtasks: self
                .pending_microtasks
                .saturating_sub(other.pending_microtasks),
            pending_reactions: self
                .pending_reactions
                .saturating_sub(other.pending_reactions),
            payload_bytes: self.payload_bytes.saturating_sub(other.payload_bytes),
        }
    }
    fn fits(self) -> bool {
        self.pending_browser_tasks <= MAX_PENDING_BROWSER_TASKS
            && self.pending_timers <= MAX_PENDING_HOST_TIMERS
            && self.pending_microtasks <= MAX_PENDING_HOST_MICROTASKS
            && self.pending_reactions <= MAX_PENDING_HOST_REACTIONS
            && self.payload_bytes <= MAX_HOST_SCHEDULING_BYTES
    }
}

#[derive(Default)]
struct Account {
    usage: HostSchedulingUsage,
    #[cfg(any(test, coverage))]
    payload_peak: Option<usize>,
    failed: bool,
    next_task_serial: u64,
    clock_ms: u64,
    host_code_cache: std::rc::Weak<RefCell<super::HostCodeCache>>,
    document_heap: Option<crate::heap::DocumentHeapBudget>,
    checkpoint_depth: usize,
    checkpoint_pending: bool,
}

impl Account {
    fn refresh_managed_failure(&mut self) {
        self.failed |= self
            .document_heap
            .as_ref()
            .is_some_and(crate::heap::DocumentHeapBudget::is_failed);
    }
}

#[cfg(any(test, coverage))]
impl Account {
    fn observe_payload_peak(&mut self) {
        if let Some(peak) = &mut self.payload_peak {
            *peak = (*peak).max(self.usage.payload_bytes);
        }
    }
}

pub(super) struct SchedulingScope {
    account: Rc<RefCell<Account>>,
    own: HostSchedulingUsage,
}

/// One synchronous task/checkpoint defers per-realm completion without owning VMs.
pub(super) struct CheckpointDeferral {
    account: Rc<RefCell<Account>>,
}
impl Drop for CheckpointDeferral {
    fn drop(&mut self) {
        let mut account = self.account.borrow_mut();
        debug_assert!(account.checkpoint_depth != 0);
        account.checkpoint_depth -= 1;
    }
}

/// One shared native owner, independent of individual realm scopes.
pub(crate) struct SharedSchedulingCharge {
    account: Rc<RefCell<Account>>,
    bytes: usize,
    pending_microtasks: usize,
    pending_reactions: usize,
    pending_browser_tasks: usize,
    pending_timers: usize,
}

/// Keep a weak-retained allocation header charged after its data is destroyed.
pub(super) struct SharedNativeHeader {
    _charge: SharedSchedulingCharge,
}
impl SharedNativeHeader {
    pub(super) fn new(charge: SharedSchedulingCharge) -> Rc<Self> {
        Rc::new(Self { _charge: charge })
    }
}

impl SharedSchedulingCharge {
    pub(super) fn scheduling_scope(&self) -> SchedulingScope {
        SchedulingScope {
            account: self.account.clone(),
            own: HostSchedulingUsage::default(),
        }
    }
    /// Reconcile storage already owned by a retained frame after a move/edit.
    /// Producers still admit allocation separately. The VM reconciles its
    /// corresponding fields before checking limits or observing a peak, so a
    /// transfer cannot be counted in both ownership views.
    pub(super) fn reconcile_owned_bytes(&mut self, bytes: usize) {
        let mut account = self.account.borrow_mut();
        account.usage.payload_bytes = account.usage.payload_bytes - self.bytes + bytes;
        self.bytes = bytes;
    }

    pub(super) fn clear_host_code_cache(&self) {
        self.account.borrow_mut().host_code_cache = std::rc::Weak::new();
    }
    pub(crate) fn reserve_peer(&self, bytes: usize) -> Result<Self, RuntimeError> {
        let mut charge = Self {
            account: self.account.clone(),
            bytes: 0,
            pending_microtasks: 0,
            pending_reactions: 0,
            pending_browser_tasks: 0,
            pending_timers: 0,
        };
        charge.resize(bytes)?;
        Ok(charge)
    }
    /// Optional reclamation never turns declined scratch into termination.
    pub(super) fn try_reserve_peer(&self, bytes: usize) -> Option<Self> {
        let mut charge = Self {
            account: self.account.clone(),
            bytes: 0,
            pending_microtasks: 0,
            pending_reactions: 0,
            pending_browser_tasks: 0,
            pending_timers: 0,
        };
        charge.try_resize_for_reclamation(bytes).then_some(charge)
    }
    pub(super) fn bytes(&self) -> usize {
        self.bytes
    }
    pub(crate) fn split_off(&mut self, bytes: usize) -> Self {
        assert!(bytes <= self.bytes, "split existing native ownership");
        self.bytes -= bytes;
        Self {
            account: self.account.clone(),
            bytes,
            pending_microtasks: 0,
            pending_reactions: 0,
            pending_browser_tasks: 0,
            pending_timers: 0,
        }
    }

    pub(super) fn reserve_microtasks(&self, count: usize) -> Result<Self, RuntimeError> {
        self.reserve_pending_work(count, 0, 0, 0)
    }

    pub(super) fn reserve_reactions(&self, count: usize) -> Result<Self, RuntimeError> {
        self.reserve_pending_work(0, count, 0, 0)
    }

    pub(super) fn reserve_browser_tasks(&self, count: usize) -> Result<Self, RuntimeError> {
        self.reserve_pending_work(0, 0, count, 0)
    }

    pub(super) fn reserve_timers(&self, count: usize) -> Result<Self, RuntimeError> {
        self.reserve_pending_work(0, 0, 0, count)
    }

    pub(super) fn split_timers(&mut self, count: usize) -> Self {
        assert!(count <= self.pending_timers);
        self.pending_timers -= count;
        Self {
            account: self.account.clone(),
            bytes: 0,
            pending_microtasks: 0,
            pending_reactions: 0,
            pending_browser_tasks: 0,
            pending_timers: count,
        }
    }

    pub(super) fn release_timers(&mut self) {
        self.account.borrow_mut().usage.pending_timers -= self.pending_timers;
        self.pending_timers = 0;
    }

    pub(super) fn split_browser_tasks(&mut self, count: usize) -> Self {
        assert!(count <= self.pending_browser_tasks);
        self.pending_browser_tasks -= count;
        Self {
            account: self.account.clone(),
            bytes: 0,
            pending_microtasks: 0,
            pending_reactions: 0,
            pending_browser_tasks: count,
            pending_timers: 0,
        }
    }

    pub(super) fn release_browser_tasks(&mut self) {
        self.release_browser_tasks_to(0);
    }

    pub(super) fn release_browser_tasks_to(&mut self, count: usize) {
        assert!(
            count <= self.pending_browser_tasks,
            "task cleanup cannot acquire work"
        );
        self.account.borrow_mut().usage.pending_browser_tasks -= self.pending_browser_tasks - count;
        self.pending_browser_tasks = count;
    }

    fn reserve_pending_work(
        &self,
        microtasks: usize,
        reactions: usize,
        browser_tasks: usize,
        timers: usize,
    ) -> Result<Self, RuntimeError> {
        let mut charge = self.reserve_peer(0)?;
        let mut account = charge.account.borrow_mut();
        let extra = HostSchedulingUsage {
            pending_microtasks: microtasks,
            pending_reactions: reactions,
            pending_browser_tasks: browser_tasks,
            pending_timers: timers,
            ..Default::default()
        };
        let next = account.usage.add(extra);
        if account.failed || !next.fits() {
            account.failed = true;
            return Err(RuntimeError::SchedulingLimit);
        }
        account.usage = next;
        charge.pending_microtasks = microtasks;
        charge.pending_reactions = reactions;
        charge.pending_browser_tasks = browser_tasks;
        charge.pending_timers = timers;
        drop(account);
        Ok(charge)
    }

    pub(super) fn split_microtasks(&mut self, count: usize) -> Self {
        assert!(
            count <= self.pending_microtasks,
            "split existing pending work"
        );
        self.pending_microtasks -= count;
        Self {
            account: self.account.clone(),
            bytes: 0,
            pending_microtasks: count,
            pending_reactions: 0,
            pending_browser_tasks: 0,
            pending_timers: 0,
        }
    }

    pub(super) fn release_microtasks(&mut self) {
        self.account.borrow_mut().usage.pending_microtasks -= self.pending_microtasks;
        self.pending_microtasks = 0;
    }

    pub(super) fn reaction_count(&self) -> usize {
        self.pending_reactions
    }

    pub(super) fn split_reactions(&mut self, count: usize) -> Self {
        assert!(
            count <= self.pending_reactions,
            "split existing pending reactions"
        );
        self.pending_reactions -= count;
        Self {
            account: self.account.clone(),
            bytes: 0,
            pending_microtasks: 0,
            pending_reactions: count,
            pending_browser_tasks: 0,
            pending_timers: 0,
        }
    }

    pub(super) fn release_reactions_to(&mut self, count: usize) {
        assert!(
            count <= self.pending_reactions,
            "reaction cleanup cannot acquire work"
        );
        self.account.borrow_mut().usage.pending_reactions -= self.pending_reactions - count;
        self.pending_reactions = count;
    }

    pub(super) fn publish_microtasks(&self) {
        if self.pending_microtasks != 0 {
            self.account.borrow_mut().checkpoint_pending = true;
        }
    }
    /// Releasing existing storage is permitted after terminal admission failure.
    pub(crate) fn release_to(&mut self, bytes: usize) {
        assert!(
            bytes <= self.bytes,
            "cleanup cannot acquire native storage: retaining {bytes}, owned {}",
            self.bytes
        );
        self.account.borrow_mut().usage.payload_bytes -= self.bytes - bytes;
        self.bytes = bytes;
    }
    /// Transfer existing ownership without admitting or charging it again.
    pub(crate) fn absorb(&mut self, mut other: Self) -> Result<(), RuntimeError> {
        self.absorb_from(&mut other)
    }

    /// Keep the source guard intact on refusal so its data can drop before
    /// its credit. Successful transfer leaves that source as a zero-byte view.
    pub(super) fn absorb_from(&mut self, other: &mut Self) -> Result<(), RuntimeError> {
        if !Rc::ptr_eq(&self.account, &other.account) {
            return Err(self.fail());
        }
        let bytes = self
            .bytes
            .checked_add(other.bytes)
            .ok_or_else(|| self.fail())?;
        let pending_microtasks = self
            .pending_microtasks
            .checked_add(other.pending_microtasks)
            .ok_or_else(|| self.fail())?;
        let pending_reactions = self
            .pending_reactions
            .checked_add(other.pending_reactions)
            .ok_or_else(|| self.fail())?;
        let browser_tasks = self
            .pending_browser_tasks
            .checked_add(other.pending_browser_tasks)
            .ok_or_else(|| self.fail())?;
        let timers = self
            .pending_timers
            .checked_add(other.pending_timers)
            .ok_or_else(|| self.fail())?;
        self.bytes = bytes;
        self.pending_timers = timers;
        self.pending_browser_tasks = browser_tasks;
        self.pending_microtasks = pending_microtasks;
        self.pending_reactions = pending_reactions;
        other.bytes = 0;
        other.pending_microtasks = 0;
        other.pending_reactions = 0;
        other.pending_browser_tasks = 0;
        other.pending_timers = 0;
        Ok(())
    }

    pub(super) fn belongs_to(&self, scope: &SchedulingScope) -> bool {
        Rc::ptr_eq(&self.account, &scope.account)
    }

    pub(super) fn same_owner(&self, other: &Self) -> bool {
        Rc::ptr_eq(&self.account, &other.account)
    }

    pub(crate) fn resize(&mut self, bytes: usize) -> Result<(), RuntimeError> {
        if self.try_resize_for_reclamation(bytes) {
            Ok(())
        } else {
            Err(self.fail())
        }
    }

    /// Optional GC compaction may decline allocation without poisoning work.
    pub(super) fn try_resize_for_reclamation(&mut self, bytes: usize) -> bool {
        let mut account = self.account.borrow_mut();
        account.refresh_managed_failure();
        let next = account
            .usage
            .sub(HostSchedulingUsage {
                payload_bytes: self.bytes,
                ..HostSchedulingUsage::default()
            })
            .add(HostSchedulingUsage {
                payload_bytes: bytes,
                ..HostSchedulingUsage::default()
            });
        if account.failed || !next.fits() {
            return false;
        }
        account.usage = next;
        #[cfg(any(test, coverage))]
        account.observe_payload_peak();
        self.bytes = bytes;
        true
    }

    pub(super) fn fail(&self) -> RuntimeError {
        self.account.borrow_mut().failed = true;
        RuntimeError::SchedulingLimit
    }
}

/// Weak association avoids a native/managed account ownership cycle.
#[derive(Clone)]
pub(crate) struct NativeDocumentIdentity {
    account: std::rc::Weak<RefCell<Account>>,
}

/// Opaque access to the document native account for heap native storage.
#[derive(Clone)]
pub(crate) struct HeapNativeBudget {
    account: Rc<RefCell<Account>>,
}

/// Snapshots share one logical request. Retirement returns its pending count
/// while held snapshots keep the metadata allocation and original identity.
pub(crate) struct HeapReactionLease {
    pending: Cell<bool>,
    published: Cell<bool>,
    identity: Cell<Option<(ObjectId, u64, ObjectId)>>,
    storage: RefCell<Option<SharedSchedulingCharge>>,
}
impl HeapReactionLease {
    pub(crate) fn allocation_bytes() -> usize {
        std::mem::size_of::<Self>() + 2 * std::mem::size_of::<usize>()
    }
    pub(crate) fn unowned() -> Rc<Self> {
        Rc::new(Self {
            pending: Cell::new(true),
            published: Cell::new(false),
            identity: Cell::new(None),
            storage: RefCell::new(None),
        })
    }
    pub(crate) fn is_owned(&self) -> bool {
        self.storage.borrow().is_some()
    }
    pub(crate) fn is_pending(&self) -> bool {
        self.pending.get()
    }
    pub(crate) fn owner(&self) -> Option<HeapNativeBudget> {
        self.storage
            .borrow()
            .as_ref()
            .map(|charge| HeapNativeBudget {
                account: charge.account.clone(),
            })
    }
    pub(crate) fn can_join(&self, budget: &HeapNativeBudget) -> bool {
        self.storage
            .borrow()
            .as_ref()
            .is_none_or(|charge| budget.owns(charge))
    }
    pub(crate) fn bind(&self, generator: ObjectId, id: u64, target: ObjectId) -> bool {
        if !self.pending.get() {
            return false;
        }
        let key = (generator, id, target);
        match self.identity.get() {
            Some(current) => current == key,
            None => {
                self.identity.set(Some(key));
                true
            }
        }
    }
    pub(crate) fn check(&self) -> Result<(), RuntimeError> {
        if let Some(storage) = self.storage.borrow().as_ref() {
            storage.reserve_peer(0)?;
        }
        Ok(())
    }
    pub(crate) fn is_published(&self) -> bool {
        self.published.get()
    }
    pub(crate) fn set_published(&self, published: bool) {
        self.published.set(published);
    }
    pub(crate) fn retire(&self) {
        self.pending.set(false);
        self.published.set(false);
        if let Some(storage) = self.storage.borrow_mut().as_mut() {
            storage.release_reactions_to(0);
        }
    }
}
impl HeapNativeBudget {
    pub(crate) fn value_payload_bytes(value: &Value) -> usize {
        value_payload_bytes(value)
    }

    pub(crate) fn reserve_request_credit(
        &self,
        count: usize,
    ) -> Result<SharedSchedulingCharge, RuntimeError> {
        let bytes = count
            .checked_mul(HeapReactionLease::allocation_bytes())
            .ok_or_else(|| self.fail())?;
        let mut credit = self.reserve(0)?.reserve_reactions(count)?;
        credit.resize(bytes)?;
        Ok(credit)
    }
    pub(crate) fn reserve_reaction_lease(&self) -> Result<Rc<HeapReactionLease>, RuntimeError> {
        let credit = self.reserve_request_credit(1)?;
        Ok(Rc::new(HeapReactionLease {
            pending: Cell::new(true),
            published: Cell::new(false),
            identity: Cell::new(None),
            storage: RefCell::new(Some(credit)),
        }))
    }
    pub(crate) fn attach_reaction_lease(
        &self,
        lease: &HeapReactionLease,
        credit: &mut SharedSchedulingCharge,
    ) -> Result<(), RuntimeError> {
        if !self.owns(credit) || !lease.can_join(self) || !lease.is_pending() {
            return Err(self.fail());
        }
        if !lease.is_owned() {
            let mut incoming = credit.split_off(HeapReactionLease::allocation_bytes());
            incoming
                .absorb(credit.split_reactions(1))
                .expect("same document");
            *lease.storage.borrow_mut() = Some(incoming);
        }
        Ok(())
    }
    pub(crate) fn identity(&self) -> NativeDocumentIdentity {
        NativeDocumentIdentity {
            account: Rc::downgrade(&self.account),
        }
    }
    pub(crate) fn has_identity(&self, identity: &NativeDocumentIdentity) -> bool {
        std::rc::Weak::ptr_eq(&identity.account, &Rc::downgrade(&self.account))
    }
    pub(crate) fn host_code_cache(&self) -> Option<Rc<RefCell<super::HostCodeCache>>> {
        self.account.borrow().host_code_cache.upgrade()
    }
    pub(crate) fn install_host_code_cache(&self, cache: &Rc<RefCell<super::HostCodeCache>>) {
        self.account.borrow_mut().host_code_cache = Rc::downgrade(cache);
    }
    pub(crate) fn owns(&self, charge: &SharedSchedulingCharge) -> bool {
        Rc::ptr_eq(&self.account, &charge.account)
    }
    pub(crate) fn reserve(&self, bytes: usize) -> Result<SharedSchedulingCharge, RuntimeError> {
        let mut charge = SharedSchedulingCharge {
            account: self.account.clone(),
            bytes: 0,
            pending_microtasks: 0,
            pending_reactions: 0,
            pending_browser_tasks: 0,
            pending_timers: 0,
        };
        charge.resize(bytes)?;
        Ok(charge)
    }
    pub(crate) fn same_document(&self, other: &Self) -> bool {
        Rc::ptr_eq(&self.account, &other.account)
    }
    pub(crate) fn fail(&self) -> RuntimeError {
        self.account.borrow_mut().failed = true;
        RuntimeError::SchedulingLimit
    }
}

impl Drop for SharedSchedulingCharge {
    fn drop(&mut self) {
        let mut account = self.account.borrow_mut();
        account.usage.payload_bytes -= self.bytes;
        account.usage.pending_microtasks -= self.pending_microtasks;
        account.usage.pending_reactions -= self.pending_reactions;
        account.usage.pending_browser_tasks -= self.pending_browser_tasks;
        account.usage.pending_timers -= self.pending_timers;
    }
}

impl SchedulingScope {
    pub(super) fn defer_checkpoint(&self) -> Result<CheckpointDeferral, RuntimeError> {
        let mut account = self.account.borrow_mut();
        account.refresh_managed_failure();
        let next = account.checkpoint_depth.checked_add(1);
        if account.failed || next.is_none() {
            account.failed = true;
            return Err(RuntimeError::SchedulingLimit);
        }
        account.checkpoint_depth = next.unwrap();
        Ok(CheckpointDeferral {
            account: self.account.clone(),
        })
    }
    pub(super) fn checkpoint_deferred(&self) -> bool {
        self.account.borrow().checkpoint_depth != 0
    }
    pub(super) fn checkpoint_pending(&self) -> bool {
        self.account.borrow().checkpoint_pending
    }
    pub(super) fn request_checkpoint(&self) {
        self.account.borrow_mut().checkpoint_pending = true;
    }
    pub(super) fn complete_checkpoint(&self) {
        self.account.borrow_mut().checkpoint_pending = false;
    }
    /// Measure one finite native operation against the actual shared account.
    #[cfg(any(test, coverage))]
    pub(super) fn measure_payload_peak<T>(&self, operation: impl FnOnce() -> T) -> (T, usize) {
        struct Observation<'a>(&'a SchedulingScope);
        impl Drop for Observation<'_> {
            fn drop(&mut self) {
                self.0.account.borrow_mut().payload_peak = None;
            }
        }
        let before = {
            let mut account = self.account.borrow_mut();
            assert!(
                account.payload_peak.is_none(),
                "one finite observation at a time"
            );
            let before = account.usage.payload_bytes;
            account.payload_peak = Some(before);
            before
        };
        let observation = Observation(self);
        let result = operation();
        let peak = self.account.borrow().payload_peak.unwrap() - before;
        drop(observation);
        (result, peak)
    }
    /// Cleanup has already freed this part of the VM's published allocation.
    /// Refund it before releasing a candidate lease or preparing the next one.
    pub(super) fn release_owned_payload(&mut self, bytes: usize) {
        assert!(
            bytes <= self.own.payload_bytes,
            "release only published VM ownership"
        );
        self.own.payload_bytes -= bytes;
        self.account.borrow_mut().usage.payload_bytes -= bytes;
    }
    /// Move published VM storage to an independent Rust owner. The physical
    /// data and aggregate account do not change, so no admission is repeated.
    pub(super) fn detach_owned_payload(&mut self, bytes: usize) -> SharedSchedulingCharge {
        assert!(
            bytes <= self.own.payload_bytes,
            "detach published VM ownership"
        );
        self.own.payload_bytes -= bytes;
        SharedSchedulingCharge {
            account: self.account.clone(),
            bytes,
            pending_microtasks: 0,
            pending_reactions: 0,
            pending_browser_tasks: 0,
            pending_timers: 0,
        }
    }

    pub(super) fn detach_owned_microtasks(&mut self, count: usize) -> SharedSchedulingCharge {
        assert!(
            count <= self.own.pending_microtasks,
            "detach published pending jobs"
        );
        self.own.pending_microtasks -= count;
        SharedSchedulingCharge {
            account: self.account.clone(),
            bytes: 0,
            pending_microtasks: count,
            pending_reactions: 0,
            pending_browser_tasks: 0,
            pending_timers: 0,
        }
    }

    pub(super) fn detach_owned_browser_tasks(&mut self, count: usize) -> SharedSchedulingCharge {
        assert!(count <= self.own.pending_browser_tasks);
        self.own.pending_browser_tasks -= count;
        SharedSchedulingCharge {
            account: self.account.clone(),
            bytes: 0,
            pending_microtasks: 0,
            pending_reactions: 0,
            pending_browser_tasks: count,
            pending_timers: 0,
        }
    }

    pub(super) fn detach_owned_timers(&mut self, count: usize) -> SharedSchedulingCharge {
        assert!(count <= self.own.pending_timers);
        self.own.pending_timers -= count;
        SharedSchedulingCharge {
            account: self.account.clone(),
            bytes: 0,
            pending_microtasks: 0,
            pending_reactions: 0,
            pending_browser_tasks: 0,
            pending_timers: count,
        }
    }

    pub(super) fn detach_owned_reactions(&mut self, count: usize) -> SharedSchedulingCharge {
        assert!(
            count <= self.own.pending_reactions,
            "detach published reactions"
        );
        self.own.pending_reactions -= count;
        SharedSchedulingCharge {
            account: self.account.clone(),
            bytes: 0,
            pending_microtasks: 0,
            pending_reactions: count,
            pending_browser_tasks: 0,
            pending_timers: 0,
        }
    }
    #[cfg(any(test, coverage))]
    pub(super) fn task_serial_state(&self) -> u64 {
        self.account.borrow().next_task_serial
    }
    #[cfg(any(test, coverage))]
    pub(super) fn set_task_serial_for_test(&self, next: u64) {
        self.account.borrow_mut().next_task_serial = next;
    }

    pub(super) fn heap_native_budget(&self) -> HeapNativeBudget {
        HeapNativeBudget {
            account: self.account.clone(),
        }
    }
    pub(super) fn fail(&self) -> RuntimeError {
        self.account.borrow_mut().failed = true;
        RuntimeError::SchedulingLimit
    }

    pub(super) fn shared_charge(
        &self,
        bytes: usize,
    ) -> Result<SharedSchedulingCharge, RuntimeError> {
        let mut charge = SharedSchedulingCharge {
            account: self.account.clone(),
            bytes: 0,
            pending_microtasks: 0,
            pending_reactions: 0,
            pending_browser_tasks: 0,
            pending_timers: 0,
        };
        charge.resize(bytes)?;
        Ok(charge)
    }
    pub(super) fn try_reclamation_charge(&self, bytes: usize) -> Option<SharedSchedulingCharge> {
        let mut charge = SharedSchedulingCharge {
            account: self.account.clone(),
            bytes: 0,
            pending_microtasks: 0,
            pending_reactions: 0,
            pending_browser_tasks: 0,
            pending_timers: 0,
        };
        charge.try_resize_for_reclamation(bytes).then_some(charge)
    }
    pub(super) fn same_document(&self, other: &Self) -> bool {
        Rc::ptr_eq(&self.account, &other.account)
    }

    pub(super) fn advance_clock(&self, now_ms: u64) -> u64 {
        let mut account = self.account.borrow_mut();
        account.clock_ms = account.clock_ms.max(now_ms);
        account.clock_ms
    }

    pub(super) fn next_task_serial(&self) -> Result<u64, RuntimeError> {
        let mut account = self.account.borrow_mut();
        account.refresh_managed_failure();
        if account.failed {
            return Err(RuntimeError::SchedulingLimit);
        }
        let serial = account.next_task_serial;
        let Some(next) = serial.checked_add(1) else {
            account.failed = true;
            return Err(RuntimeError::SchedulingLimit);
        };
        account.next_task_serial = next;
        Ok(serial)
    }
    pub(super) fn document_heap_budget(&self) -> Option<crate::heap::DocumentHeapBudget> {
        self.account.borrow().document_heap.clone()
    }
    pub(super) fn bind_document_heap_budget(
        &self,
        budget: &crate::heap::DocumentHeapBudget,
    ) -> Result<(), RuntimeError> {
        let compatible = self
            .account
            .borrow()
            .document_heap
            .as_ref()
            .is_none_or(|existing| existing.same_document(budget));
        if !compatible || !budget.accepts_native(&self.heap_native_budget()) {
            return Err(self.fail());
        }
        budget
            .bind_native(&self.heap_native_budget())
            .map_err(|_| self.fail())?;
        self.account.borrow_mut().document_heap = Some(budget.clone());
        Ok(())
    }
    pub(super) fn reserve_task_serials(&self, count: usize) -> Result<u64, RuntimeError> {
        let mut account = self.account.borrow_mut();
        account.refresh_managed_failure();
        let next = u64::try_from(count)
            .ok()
            .and_then(|count| account.next_task_serial.checked_add(count));
        if account.failed || next.is_none() {
            account.failed = true;
            return Err(RuntimeError::SchedulingLimit);
        }
        let start = account.next_task_serial;
        account.next_task_serial = next.unwrap();
        Ok(start)
    }

    pub(super) fn new() -> Self {
        Self {
            account: Rc::new(RefCell::new(Account::default())),
            own: HostSchedulingUsage::default(),
        }
    }
    pub(super) fn child(&self) -> Self {
        Self {
            account: self.account.clone(),
            own: HostSchedulingUsage::default(),
        }
    }
    pub(super) fn check(
        &mut self,
        own: HostSchedulingUsage,
        extra: HostSchedulingUsage,
    ) -> Result<(), RuntimeError> {
        let mut account = self.account.borrow_mut();
        account.refresh_managed_failure();
        // Published jobs can arrive from native/error callbacks after script cleanup.
        account.checkpoint_pending |= own.pending_microtasks != 0;
        account.usage = account.usage.sub(self.own).add(own);
        #[cfg(any(test, coverage))]
        account.observe_payload_peak();
        self.own = own;
        if !account.usage.add(extra).fits() {
            account.failed = true;
        }
        if account.failed {
            Err(RuntimeError::SchedulingLimit)
        } else {
            Ok(())
        }
    }
}
impl Drop for SchedulingScope {
    fn drop(&mut self) {
        let mut account = self.account.borrow_mut();
        account.usage = account.usage.sub(self.own);
    }
}

pub(super) fn value_payload_bytes(value: &Value) -> usize {
    match value {
        Value::String(text) => text.owned_heap_payload_bytes().unwrap_or(usize::MAX),
        Value::Symbol(symbol) => symbol.description.as_ref().map_or(0, |text| {
            text.owned_heap_payload_bytes().unwrap_or(usize::MAX)
        }),
        Value::BigInt(number) => number
            .iter_u32_digits()
            .len()
            .saturating_mul(std::mem::size_of::<u32>()),
        _ => 0,
    }
}

pub(super) fn property_name_payload_bytes(key: &PropertyName) -> usize {
    match key {
        PropertyName::String(text) => text.owned_heap_payload_bytes().unwrap_or(usize::MAX),
        PropertyName::Symbol(symbol) => symbol.description.as_ref().map_or(0, |text| {
            text.owned_heap_payload_bytes().unwrap_or(usize::MAX)
        }),
    }
}
pub(super) fn capability_bytes(target: &ReactionTarget) -> usize {
    match target {
        ReactionTarget::Native(_) => 0,
        ReactionTarget::Capability(cap) => value_payload_bytes(&cap.promise)
            .saturating_add(value_payload_bytes(&cap.resolve))
            .saturating_add(value_payload_bytes(&cap.reject)),
    }
}
impl PromiseJob {
    pub(super) fn scheduling_payload_bytes(&self) -> usize {
        let values = match self {
            Self::Reaction {
                target,
                handler,
                value,
                ..
            } => capability_bytes(target)
                .saturating_add(value_payload_bytes(handler))
                .saturating_add(value_payload_bytes(value)),
            Self::FinalizationCleanup { callback, holdings } => {
                value_payload_bytes(callback).saturating_add(value_payload_bytes(holdings))
            }
            Self::HostMicrotask { callback } => value_payload_bytes(callback),
            Self::Thenable { thenable, then, .. } => {
                value_payload_bytes(thenable).saturating_add(value_payload_bytes(then))
            }
            Self::DynamicImport {
                referrer,
                specifier,
                ..
            } => referrer.capacity().saturating_add(specifier.capacity()),
            Self::ModuleAwait { value, .. }
            | Self::AsyncAwait { value, .. }
            | Self::AsyncGeneratorYield { value, .. }
            | Self::AsyncGeneratorDelegate { value, .. } => value_payload_bytes(value),
        };
        values.saturating_add(
            self.rejection_origin()
                .map_or(0, ThrowOrigin::payload_bytes),
        )
    }
}
pub(super) fn reaction_bytes(reaction: &PromiseReaction) -> usize {
    match reaction {
        PromiseReaction::Then(reaction) => capability_bytes(&reaction.target)
            .saturating_add(value_payload_bytes(&reaction.on_fulfilled))
            .saturating_add(value_payload_bytes(&reaction.on_rejected)),
        _ => 0,
    }
}
pub(super) fn values_bytes(values: &[Value]) -> usize {
    values.iter().fold(0usize, |total, value| {
        total.saturating_add(value_payload_bytes(value))
    })
}
pub(super) fn slots<T>(values: &Vec<T>) -> usize {
    values.capacity().saturating_mul(std::mem::size_of::<T>())
}
pub(super) fn map_slots<K, V, S: std::hash::BuildHasher>(values: &HashMap<K, V, S>) -> usize {
    values
        .capacity()
        .saturating_mul(std::mem::size_of::<(K, V)>())
}
pub(super) fn named_cells_bytes(values: &HashMap<String, DynamicEvalBinding>) -> usize {
    values
        .iter()
        .fold(map_slots(values), |bytes, (name, binding)| {
            bytes
                .saturating_add(name.capacity())
                .saturating_add(slots(&binding.shadowed_cells))
        })
}
pub(super) fn named_slots_bytes(values: &HashMap<usize, String>) -> usize {
    values.values().fold(map_slots(values), |bytes, name| {
        bytes.saturating_add(name.capacity())
    })
}
pub(super) fn completion_payload_bytes(completion: &Completion) -> usize {
    match completion {
        Completion::Return(value) | Completion::Yield(value) | Completion::Halt(value) => {
            value_payload_bytes(value)
        }
        Completion::TailCall(values) | Completion::TailRecur(values) => {
            slots(values).saturating_add(values_bytes(values))
        }
        Completion::Throw(error) => runtime_error_payload_bytes(error),
        _ => 0,
    }
}
pub(super) fn runtime_error_payload_bytes(error: &RuntimeError) -> usize {
    match error {
        RuntimeError::Thrown(value) => value_payload_bytes(value),
        RuntimeError::ReferenceError(message)
        | RuntimeError::TypeError(message)
        | RuntimeError::RangeError(message)
        | RuntimeError::SyntaxError(message)
        | RuntimeError::Test262(message)
        | RuntimeError::RegexWorker(message)
        | RuntimeError::ModuleResolution(message) => message.capacity(),
        _ => 0,
    }
}

/// Rust String clones copy initialized bytes without the source's spare capacity.
pub(super) fn runtime_error_copy_bytes(error: &RuntimeError) -> usize {
    match error {
        RuntimeError::ReferenceError(message)
        | RuntimeError::TypeError(message)
        | RuntimeError::RangeError(message)
        | RuntimeError::SyntaxError(message)
        | RuntimeError::Test262(message)
        | RuntimeError::RegexWorker(message)
        | RuntimeError::ModuleResolution(message) => message.len(),
        _ => runtime_error_payload_bytes(error),
    }
}
fn suspended_bytes(frame: &SuspendedModuleExecution) -> usize {
    // Leases follow Rust-held and suspended owners without charging them twice.
    frame
        .control_bytes()
        .saturating_add(frame.payload_bytes())
        .saturating_sub(frame.payload_charge.bytes())
        .saturating_sub(frame.control_charge.bytes())
}

impl AsyncContinuation {
    pub(super) fn scheduling_payload_bytes(&self) -> usize {
        suspended_bytes(&self.execution)
            .saturating_add(self.iterators.unleased_bytes())
            .saturating_add(self.handlers.unleased_bytes())
    }
}
impl ModuleContinuation {
    pub(super) fn scheduling_payload_bytes(&self) -> usize {
        suspended_bytes(&self.execution)
            .saturating_add(self.iterators.unleased_bytes())
            .saturating_add(self.handlers.unleased_bytes())
            .saturating_add(self.module.capacity())
            .saturating_sub(self.module_charge.bytes())
    }
}

/// Unique buffers of one outer code-unit clone. Runtime-parsed modules own
/// shared child headers/buffers and SourceText separately. Host-precompiled shared
/// storage and diagnostic maps remain separate ownership gates.
pub(super) fn owned_code_bytes(code: &Bytecode) -> usize {
    code_bytes(code, false)
}

pub(super) fn cloned_code_bytes(code: &Bytecode) -> usize {
    code_bytes(code, true)
}

fn code_buffer_len(length: usize, capacity: usize, cloned: bool) -> usize {
    if cloned {
        length
    } else {
        capacity
    }
}
fn code_slots<T>(values: &Vec<T>, cloned: bool) -> usize {
    code_buffer_len(values.len(), values.capacity(), cloned)
        .saturating_mul(std::mem::size_of::<T>())
}
fn code_text_bytes(text: &JsString, cloned: bool) -> usize {
    if cloned {
        text.len().saturating_mul(2)
    } else {
        text.owned_heap_payload_bytes().unwrap_or(usize::MAX)
    }
}
fn code_value_bytes(value: &Value, cloned: bool) -> usize {
    if !cloned {
        return value_payload_bytes(value);
    }
    match value {
        Value::String(text) => code_text_bytes(text, true),
        Value::Symbol(symbol) => symbol
            .description
            .as_ref()
            .map_or(0, |text| code_text_bytes(text, true)),
        _ => value_payload_bytes(value),
    }
}

fn code_bytes(code: &Bytecode, cloned: bool) -> usize {
    let mut bytes = code_slots(&code.code, cloned)
        .saturating_add(code_slots(&code.diagnostic_ast_owners, cloned))
        .saturating_add(code_slots(&code.root_statement_offsets, cloned))
        .saturating_add(code_slots(&code.identifier_offsets, cloned))
        .saturating_add(code_slots(&code.root_statement_ranges, cloned))
        .saturating_add(code_slots(&code.root_function_child_indices, cloned))
        .saturating_add(code_slots(&code.root_statement_child_ranges, cloned))
        .saturating_add(code_slots(&code.root_declaration_binding_slots, cloned))
        .saturating_add(code_slots(&code.constants, cloned))
        .saturating_add(code.constants.iter().fold(0usize, |bytes, value| {
            bytes.saturating_add(code_value_bytes(value, cloned))
        }))
        .saturating_add(code_slots(&code.bindings, cloned))
        .saturating_add(code_slots(&code.scopes, cloned))
        .saturating_add(code_slots(&code.global_function_names, cloned))
        .saturating_add(code_slots(&code.dynamic_eval_slots, cloned))
        .saturating_add(code_slots(&code.functions, cloned))
        .saturating_add(code_slots(&code.captures, cloned))
        .saturating_add(code_buffer_len(
            code.function_name.len(),
            code.function_name.capacity(),
            cloned,
        ))
        .saturating_add(code_slots(&code.arguments_mapped_slots, cloned))
        .saturating_add(code_slots(&code.templates, cloned))
        .saturating_add(code_slots(&code.handlers, cloned))
        .saturating_add(code_slots(&code.abrupt_jumps, cloned))
        .saturating_add(code_slots(&code.async_yield_delegates, cloned))
        .saturating_add(code_slots(&code.yield_delegates, cloned))
        .saturating_add(code_slots(&code.module_imports, cloned))
        .saturating_add(code_slots(&code.module_exports, cloned))
        .saturating_add(code_slots(&code.module_requests, cloned));
    for identifier in &code.identifier_offsets {
        bytes = bytes.saturating_add(code_buffer_len(
            identifier.name.len(),
            identifier.name.capacity(),
            cloned,
        ));
    }
    for binding in &code.bindings {
        bytes = bytes.saturating_add(code_buffer_len(
            binding.name.len(),
            binding.name.capacity(),
            cloned,
        ));
    }
    for scope in &code.scopes {
        bytes = bytes.saturating_add(code_slots(scope, cloned));
    }
    for name in &code.global_function_names {
        bytes = bytes.saturating_add(code_buffer_len(name.len(), name.capacity(), cloned));
    }
    for template in &code.templates {
        bytes = bytes
            .saturating_add(code_slots(&template.raw, cloned))
            .saturating_add(code_slots(&template.cooked, cloned));
        for text in template.raw.iter().chain(template.cooked.iter().flatten()) {
            bytes = bytes.saturating_add(code_text_bytes(text, cloned));
        }
    }
    for import in &code.module_imports {
        bytes = bytes.saturating_add(code_buffer_len(
            import.module_request.len(),
            import.module_request.capacity(),
            cloned,
        ));
        if let crate::bytecode::ModuleImportName::Named(name) = &import.import_name {
            bytes = bytes.saturating_add(code_buffer_len(name.len(), name.capacity(), cloned));
        }
    }
    for request in &code.module_requests {
        bytes = bytes.saturating_add(code_buffer_len(
            request.module_request.len(),
            request.module_request.capacity(),
            cloned,
        ));
    }
    for export in &code.module_exports {
        use crate::bytecode::ModuleExport;
        bytes = bytes.saturating_add(match export {
            ModuleExport::Local { export_name, .. } => {
                code_buffer_len(export_name.len(), export_name.capacity(), cloned)
            }
            ModuleExport::Indirect {
                export_name,
                module_request,
                import_name,
                ..
            } => code_buffer_len(export_name.len(), export_name.capacity(), cloned)
                .saturating_add(code_buffer_len(
                    module_request.len(),
                    module_request.capacity(),
                    cloned,
                ))
                .saturating_add(code_buffer_len(
                    import_name.len(),
                    import_name.capacity(),
                    cloned,
                )),
            ModuleExport::Star { module_request, .. } => {
                code_buffer_len(module_request.len(), module_request.capacity(), cloned)
            }
            ModuleExport::Namespace {
                export_name,
                module_request,
                ..
            }
            | ModuleExport::DeferredNamespace {
                export_name,
                module_request,
                ..
            }
            | ModuleExport::Source {
                export_name,
                module_request,
            } => code_buffer_len(export_name.len(), export_name.capacity(), cloned).saturating_add(
                code_buffer_len(module_request.len(), module_request.capacity(), cloned),
            ),
        });
    }
    // Synthetic exports retain a separate shared document owner; cloning
    // the code unit does not copy or adopt their primitive buffers.
    bytes
}

impl Vm {
    /// Call boundaries already root the callee, receiver, arguments and
    /// new.target. A fresh returned value is protected here if GC is needed.
    /// Collect here before new native payload can exhaust the page account;
    /// arbitrary admission sites may still own unregistered Rust temporaries.
    pub(super) fn collect_for_host_native_pressure(
        &mut self,
        value: &Value,
        arguments: &[Value],
    ) -> Result<(), RuntimeError> {
        let Some(scope) = &self.host_scheduling else {
            return Ok(());
        };
        self.heap.check_document_heap_budget()?;
        let incoming = arguments
            .iter()
            .fold(value_payload_bytes(value), |bytes, value| {
                bytes.saturating_add(value_payload_bytes(value))
            });
        let retained = scope.account.borrow().usage.payload_bytes;
        let heap_pressure = self
            .heap
            .document_heap_budget()
            .is_some_and(|(used, _)| used.saturating_add(incoming) >= self.next_host_heap_gc_bytes);
        // Resolving algorithms can retain the input while another native
        // call copies it into the settled owner. Collect before those
        // simultaneously live buffers exhaust admission, not just before
        // the first copy reaches the watermark.
        let native_pressure = retained.saturating_add(incoming.saturating_mul(3));
        if native_pressure < self.next_host_native_gc_bytes && !heap_pressure {
            return Ok(());
        }
        let root_base = self.async_frame_roots.len();
        self.extend_async_frame_roots(value.object_id().into_iter())?;
        let collected = self.collect_realm_garbage();
        self.async_frame_roots.truncate(root_base);
        collected?;
        self.synchronize_scheduling_usage()?;
        let live = self.host_scheduling_snapshot().payload_bytes;
        // A large live owner must not force a major collection at every call.
        // Incoming primitive payload still prompts a retry before the cap.
        self.next_host_native_gc_bytes = live
            .saturating_mul(2)
            .clamp(MAX_HOST_SCHEDULING_BYTES / 4, MAX_HOST_SCHEDULING_BYTES);
        if let Some((used, limit)) = self.heap.document_heap_budget() {
            self.next_host_heap_gc_bytes = used.saturating_mul(2).clamp(limit / 2, limit);
        }
        Ok(())
    }

    pub(super) fn checkpoint_context_is_idle(&self) -> bool {
        self.call_depth == 0 && !self.top_level_module && self.module_execution_depth == 0
    }
    pub(super) fn checkpoint_completion_deferred(&self) -> bool {
        self.host_scheduling.as_ref().is_some_and(|scope| {
            scope.checkpoint_deferred() || realm_reentrancy::has_active_document(scope)
        })
    }
    pub(super) fn checkpoint_completion_pending(&self) -> bool {
        self.host_scheduling
            .as_ref()
            .is_some_and(SchedulingScope::checkpoint_pending)
    }
    pub(super) fn prepare_checkpoint_completion(&mut self) -> Result<(), RuntimeError> {
        if !self.checkpoint_context_is_idle() {
            return Ok(());
        }
        self.report_pending_host_module_errors()?;
        self.notify_host_rejections()
    }
    pub(super) fn collect_after_checkpoint(&mut self) -> Result<(), RuntimeError> {
        if !self.checkpoint_context_is_idle() {
            return Ok(());
        }
        if self.host_scheduling.is_some()
            && self.own_scheduling_usage().payload_bytes >= MAX_HOST_SCHEDULING_BYTES / 4
        {
            self.collect_realm_garbage()?;
            self.synchronize_scheduling_usage()?;
        }
        self.enqueue_finalization_cleanup_jobs()
    }
    pub(super) fn finish_host_checkpoint(&mut self) -> Result<(), RuntimeError> {
        if !self.checkpoint_context_is_idle() || self.checkpoint_completion_deferred() {
            return Ok(());
        }
        self.prepare_checkpoint_completion()?;
        self.kept_weak_objects.clear();
        self.collect_after_checkpoint()
    }

    pub(super) fn own_scheduling_usage(&self) -> HostSchedulingUsage {
        let browser = self.browser_tasks.unowned_usage();
        let mut usage = HostSchedulingUsage {
            pending_browser_tasks: browser
                .pending_browser_tasks
                .saturating_add(self.host_finalization_tasks.unowned_count()),
            pending_timers: self.host_timers.unowned_count(),
            pending_microtasks: self.promise_jobs.unowned_microtasks(),
            pending_reactions: self
                .heap
                .unowned_async_generator_requests()
                .saturating_add(self.host_promises.unowned_wait_count())
                .saturating_add(browser.pending_reactions),
            payload_bytes: self
                .promise_jobs
                .unowned_payload_bytes()
                .saturating_add(self.heap.unowned_async_generator_request_bytes())
                .saturating_add(browser.payload_bytes)
                .saturating_add(self.active_scheduling_bytes)
                .saturating_add(self.global_binding_storage_bytes())
                .saturating_add(map_slots(&self.templates))
                .saturating_add(self.module_referrer_storage_bytes())
                .saturating_add(self.unowned_membrane_storage_bytes())
                .saturating_add(self.unowned_active_native_storage_bytes())
                .saturating_add(self.unowned_host_listener_storage_bytes())
                .saturating_add(self.unowned_host_family_storage_bytes())
                .saturating_add(self.unowned_host_registration_storage_bytes())
                .saturating_add(self.linked_module_cell_bytes.get())
                .saturating_add(self.owned_execution_payload_bytes()),
        };
        usage.payload_bytes = usage
            .payload_bytes
            .saturating_add(Self::diagnostic_origin_bytes(
                &self.pending_diagnostic_origin,
            ))
            .saturating_add(Self::diagnostic_origin_bytes(
                &self.uncaught_diagnostic_origin,
            ))
            .saturating_add(self.active_diagnostic_bytes.get())
            .saturating_add(self.active_diagnostic_frame.as_ref().map_or(
                0,
                crate::diagnostics::ActiveDiagnosticFrame::owned_payload_bytes,
            ));
        usage.payload_bytes = usage
            .payload_bytes
            .saturating_add(slots(&self.module_error_origins));
        usage.payload_bytes = usage
            .payload_bytes
            .saturating_add(self.host_neutral_module_value_bytes());
        usage.payload_bytes = usage.payload_bytes.saturating_add(
            self.host_console
                .as_ref()
                .map_or(0, |console| console.unowned_payload_bytes()),
        );
        if let Some(state) = &self.host_rejections {
            usage.pending_reactions = usage
                .pending_reactions
                .saturating_add(state.unowned_count())
                .saturating_add(state.active.saturating_sub(state.active_owned));
            usage.payload_bytes = usage.payload_bytes.saturating_add(state.unowned_bytes());
        }
        usage.payload_bytes = usage
            .payload_bytes
            .saturating_add(self.host_promises.pending.unowned_bytes());
        usage.payload_bytes = usage
            .payload_bytes
            .saturating_add(self.promise_thunk_origins.retained_bytes())
            .saturating_add(
                self.promise_thunk_origins
                    .values()
                    .map(ThrowOrigin::payload_bytes)
                    .sum::<usize>(),
            )
            .saturating_add(if self.promise_origin_override_storage.is_some() {
                0
            } else {
                self.promise_origin_override
                    .as_ref()
                    .map_or(0, ThrowOrigin::payload_bytes)
            });
        for record in self.promises.values() {
            usage.payload_bytes = usage
                .payload_bytes
                .saturating_add(record.unowned_status_payload_bytes());
            usage.pending_reactions = usage
                .pending_reactions
                .saturating_add(record.unowned_reaction_count());
            usage.payload_bytes = usage
                .payload_bytes
                .saturating_add(record.unowned_reaction_payload_bytes());
        }
        usage.payload_bytes = usage
            .payload_bytes
            .saturating_add(self.host_timers.unowned_bytes());
        usage.payload_bytes = usage.payload_bytes.saturating_add(
            self.host_animation_frames
                .capacity()
                .saturating_mul(std::mem::size_of::<HostAnimationFrame>()),
        );
        usage.payload_bytes = usage
            .payload_bytes
            .saturating_add(self.host_finalization_tasks.unowned_bytes());
        for frame in self.async_continuations.values() {
            usage.payload_bytes = usage
                .payload_bytes
                .saturating_add(frame.scheduling_payload_bytes());
        }
        for frame in self.module_continuations.values() {
            usage.payload_bytes = usage
                .payload_bytes
                .saturating_add(frame.scheduling_payload_bytes());
        }
        if let Some(frame) = &self.debugger_module_continuation {
            usage.payload_bytes = usage.payload_bytes.saturating_add(
                suspended_bytes(&frame.execution)
                    .saturating_add(frame.iterators.unleased_bytes())
                    .saturating_add(frame.handlers.unleased_bytes())
                    .saturating_add(frame.module.capacity())
                    .saturating_sub(frame.module_charge.bytes()),
            );
        }
        usage.payload_bytes = usage
            .payload_bytes
            .saturating_add(self.promises.unowned_bytes())
            .saturating_add(self.async_continuations.retained_bytes())
            .saturating_add(self.module_continuations.retained_bytes())
            .saturating_add(self.module_import_waiters.unowned_bytes());
        for (name, waiters) in self.module_import_waiters.iter() {
            usage.pending_reactions = usage
                .pending_reactions
                .saturating_add(waiters.unowned_count());
            usage.payload_bytes = usage
                .payload_bytes
                .saturating_add(waiters.unowned_bytes(name.capacity()));
        }
        usage.payload_bytes = usage
            .payload_bytes
            .saturating_add(self.deferred_import_waiters.unowned_bytes());
        for waiter in &self.deferred_import_waiters {
            usage.pending_reactions = usage
                .pending_reactions
                .saturating_add(waiter.pending.unowned_count());
            usage.payload_bytes = usage
                .payload_bytes
                .saturating_add(waiter.pending.unowned_bytes());
        }
        usage.payload_bytes = usage
            .payload_bytes
            .saturating_add(self.module_dependency_bytes())
            .saturating_add(self.namespace_cache_bytes())
            .saturating_add(self.module_identity_cache_bytes())
            .saturating_add(self.disposal_storage_bytes());
        usage
    }
    pub(super) fn admit_scheduling_work(
        &mut self,
        extra: HostSchedulingUsage,
    ) -> Result<(), RuntimeError> {
        if self.host_scheduling.is_none() {
            return Ok(());
        }
        self.bind_native_budget()?;
        self.check_scheduling_failure()?;
        let own = self.own_scheduling_usage();
        self.last_execution_payload_bytes = self.owned_execution_payload_bytes();
        self.host_scheduling.as_mut().unwrap().check(own, extra)
    }
    pub(super) fn synchronize_scheduling_usage(&mut self) -> Result<(), RuntimeError> {
        self.admit_scheduling_work(HostSchedulingUsage::default())
    }

    /// Unwinding must reconcile ownership even after admission has failed.
    /// This never clears the account's terminal failure or admits new work.
    pub(super) fn refresh_scheduling_ownership(&mut self) {
        let Some(scope) = &self.host_scheduling else {
            return;
        };
        let bytes = self.owned_execution_payload_bytes();
        if bytes == self.last_execution_payload_bytes && !scope.account.borrow().failed {
            return;
        }
        self.reconcile_scheduling_ownership();
    }

    /// Context transfer can remove continuations or change other owners even
    /// when primitive bytes are unchanged. Publish those changes as well.
    pub(super) fn reconcile_scheduling_ownership(&mut self) {
        if self.host_scheduling.is_none() {
            return;
        }
        let own = self.own_scheduling_usage();
        self.last_execution_payload_bytes = self.owned_execution_payload_bytes();
        let _ = self
            .host_scheduling
            .as_mut()
            .unwrap()
            .check(own, HostSchedulingUsage::default());
    }
    pub(super) fn check_scheduling_failure(&self) -> Result<(), RuntimeError> {
        if self.host_scheduling.as_ref().is_some_and(|scope| {
            let mut account = scope.account.borrow_mut();
            account.refresh_managed_failure();
            account.failed
        }) {
            Err(RuntimeError::SchedulingLimit)
        } else {
            self.heap
                .check_document_heap_budget()
                .map_err(RuntimeError::from)
        }
    }
    pub fn host_scheduling_usage(&mut self) -> Result<HostSchedulingUsage, RuntimeError> {
        self.synchronize_scheduling_usage()?;
        Ok(self.host_scheduling.as_ref().map_or_else(
            || self.own_scheduling_usage(),
            |scope| scope.account.borrow().usage,
        ))
    }
    pub(crate) fn host_scheduling_snapshot(&self) -> HostSchedulingUsage {
        let own = self.own_scheduling_usage();
        self.host_scheduling.as_ref().map_or(own, |scope| {
            scope.account.borrow().usage.sub(scope.own).add(own)
        })
    }
    /// Infallible Promise/thenable/finalizer and host job producers use this
    /// gate. Prepared awaits/Then registrations publish admitted work directly.
    /// Failure latches an uncatchable document abort before any later JS work;
    /// infallible internal settlement paths must never panic or report success.
    pub(super) fn enqueue_promise_job(&mut self, job: PromiseJob) {
        let _ = self.publish_promise_job(job);
    }
    /// Finite fixtures also exercise direct reaction append admission.
    #[cfg(any(test, coverage))]
    pub(super) fn enqueue_promise_reaction(
        &mut self,
        promise: ObjectId,
        reaction: PromiseReaction,
    ) {
        if self.publish_promise_reaction(promise, reaction).is_ok() {
            self.mark_promise_handled(promise);
        }
    }
}
