// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

//! Page-owned scheduling admission shared by a realm and its ShadowRealms.
//! Standalone ECMAScript VMs retain their unbounded host-neutral job policy.

use super::*;
use std::cell::RefCell;

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

    pub(super) fn for_deque_push<T>(values: &VecDeque<T>) -> Self {
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

    pub(super) fn reserve_deque<T>(self, values: &mut VecDeque<T>) -> bool {
        if values.capacity() < self.capacity {
            values.reserve_exact(self.capacity.saturating_sub(values.len()));
        }
        values.capacity() > self.capacity
    }
}

/// Prepare actual hash-table capacity without changing live owners. The
/// replacement receives existing entries by move only after joint admission.
pub(super) struct SchedulingMapCapacity<K, V> {
    replacement: Option<HashMap<K, V>>,
    additional_bytes: usize,
}

impl<K: Eq + std::hash::Hash, V> SchedulingMapCapacity<K, V> {
    pub(super) fn for_insert(
        values: &HashMap<K, V>,
        scope: Option<&SchedulingScope>,
    ) -> Result<Self, RuntimeError> {
        Self::for_additional(values, 1, scope)
    }

    pub(super) fn for_additional(
        values: &HashMap<K, V>,
        additional: usize,
        scope: Option<&SchedulingScope>,
    ) -> Result<Self, RuntimeError> {
        let required = values.len().saturating_add(additional);
        let replacement = if required > values.capacity() {
            let mut table = HashMap::new();
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

    pub(super) fn commit(self, values: &mut HashMap<K, V>) {
        if let Some(mut table) = self.replacement {
            table.extend(values.drain());
            *values = table;
        }
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

impl AwaitRegistration {
    pub(super) fn admit(
        &self,
        vm: &mut Vm,
        mut extra: HostSchedulingUsage,
    ) -> Result<(), RuntimeError> {
        match self {
            Self::Reaction { promise, reaction } => {
                extra.pending_reactions = extra.pending_reactions.saturating_add(1);
                extra.payload_bytes = extra.payload_bytes.saturating_add(reaction_bytes(reaction));
                let growth = SchedulingCapacity::for_vec_push(&vm.promises[promise].reactions);
                vm.admit_scheduling_work(extra.with_growth(growth))?;
                growth.reserve_vec(&mut vm.promises.get_mut(promise).unwrap().reactions);
            }
            Self::Job(job) => {
                extra.pending_microtasks = extra.pending_microtasks.saturating_add(1);
                extra.payload_bytes = extra
                    .payload_bytes
                    .saturating_add(job.scheduling_payload_bytes());
                let growth = SchedulingCapacity::for_deque_push(&vm.promise_jobs);
                vm.admit_scheduling_work(extra.with_growth(growth))?;
                growth.reserve_deque(&mut vm.promise_jobs);
            }
        }
        // Account for actual work slots before the frame table or its serial
        // changes. `extra` still includes the uncommitted frame/table storage.
        vm.admit_scheduling_work(extra)
    }

    pub(super) fn publish(self, vm: &mut Vm) -> Result<(), RuntimeError> {
        match self {
            Self::Reaction { promise, reaction } => vm.enqueue_promise_reaction(promise, reaction),
            Self::Job(job) => vm.enqueue_promise_job(job),
        }
        vm.check_scheduling_failure()
    }
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
    pub(super) fn with_map_growth<K, V>(mut self, growth: &SchedulingMapCapacity<K, V>) -> Self {
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
    failed: bool,
    next_task_serial: u64,
    clock_ms: u64,
}

pub(super) struct SchedulingScope {
    account: Rc<RefCell<Account>>,
    own: HostSchedulingUsage,
}

/// One shared native owner, independent of individual realm scopes.
pub(super) struct SharedSchedulingCharge {
    account: Rc<RefCell<Account>>,
    bytes: usize,
}

impl SharedSchedulingCharge {
    pub(super) fn belongs_to(&self, scope: &SchedulingScope) -> bool {
        Rc::ptr_eq(&self.account, &scope.account)
    }

    pub(super) fn resize(&mut self, bytes: usize) -> Result<(), RuntimeError> {
        let mut account = self.account.borrow_mut();
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
            account.failed = true;
            return Err(RuntimeError::SchedulingLimit);
        }
        account.usage = next;
        self.bytes = bytes;
        Ok(())
    }

    pub(super) fn fail(&self) -> RuntimeError {
        self.account.borrow_mut().failed = true;
        RuntimeError::SchedulingLimit
    }
}

impl Drop for SharedSchedulingCharge {
    fn drop(&mut self) {
        self.account.borrow_mut().usage.payload_bytes -= self.bytes;
    }
}

impl SchedulingScope {
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
        };
        charge.resize(bytes)?;
        Ok(charge)
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
        let serial = account.next_task_serial;
        let Some(next) = serial.checked_add(1) else {
            account.failed = true;
            return Err(RuntimeError::SchedulingLimit);
        };
        account.next_task_serial = next;
        Ok(serial)
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
        account.usage = account.usage.sub(self.own).add(own);
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
fn reaction_bytes(reaction: &PromiseReaction) -> usize {
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
fn slots<T>(values: &Vec<T>) -> usize {
    values.capacity().saturating_mul(std::mem::size_of::<T>())
}
fn map_slots<K, V>(values: &HashMap<K, V>) -> usize {
    values
        .capacity()
        .saturating_mul(std::mem::size_of::<(K, V)>())
}
fn named_cells_bytes(values: &HashMap<String, DynamicEvalBinding>) -> usize {
    values
        .iter()
        .fold(map_slots(values), |bytes, (name, binding)| {
            bytes
                .saturating_add(name.capacity())
                .saturating_add(slots(&binding.shadowed_cells))
        })
}
fn named_slots_bytes(values: &HashMap<usize, String>) -> usize {
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
fn suspended_bytes(frame: &SuspendedModuleExecution) -> usize {
    let mut bytes = frame
        .stack
        .capacity()
        .saturating_mul(std::mem::size_of::<Value>())
        .saturating_add(values_bytes(&frame.stack));
    bytes = bytes
        .saturating_add(
            frame
                .arguments
                .capacity()
                .saturating_mul(std::mem::size_of::<Value>()),
        )
        .saturating_add(values_bytes(&frame.arguments));
    bytes = bytes.saturating_add(
        frame
            .bindings
            .capacity()
            .saturating_mul(std::mem::size_of::<Option<Value>>()),
    );
    for value in frame.bindings.iter().flatten() {
        bytes = bytes.saturating_add(value_payload_bytes(value));
    }
    bytes = bytes.saturating_add(
        frame
            .binding_metadata
            .capacity()
            .saturating_mul(std::mem::size_of::<Binding>()),
    );
    for binding in &frame.binding_metadata {
        bytes = bytes.saturating_add(binding.name.capacity());
    }
    bytes = bytes
        .saturating_add(
            frame
                .with_objects
                .capacity()
                .saturating_mul(std::mem::size_of::<Value>()),
        )
        .saturating_add(values_bytes(&frame.with_objects));
    for value in [
        &frame.this,
        &frame.callee,
        &frame.new_target,
        &frame.completion,
    ] {
        bytes = bytes.saturating_add(value_payload_bytes(value));
    }
    bytes = bytes
        .saturating_add(slots(&frame.active_scopes))
        .saturating_add(slots(&frame.active_scope_slots));
    for scope in &frame.active_scope_slots {
        bytes = bytes.saturating_add(slots(scope));
    }
    bytes = bytes.saturating_add(slots(&frame.pending_completions));
    for completion in &frame.pending_completions {
        bytes = bytes.saturating_add(completion_payload_bytes(completion));
    }
    bytes = bytes.saturating_add(slots(&frame.completion_saves));
    for (value, _) in &frame.completion_saves {
        bytes = bytes.saturating_add(value_payload_bytes(value));
    }
    bytes = bytes
        .saturating_add(map_slots(&frame.cells))
        .saturating_add(named_cells_bytes(&frame.dynamic_eval_bindings))
        .saturating_add(named_slots_bytes(&frame.eval_dynamic_slots));
    bytes = bytes.saturating_add(slots(&frame.dynamic_eval_outer_bindings));
    for bindings in &frame.dynamic_eval_outer_bindings {
        bytes = bytes.saturating_add(named_cells_bytes(bindings));
    }
    bytes = bytes
        .saturating_add(named_slots_bytes(&frame.script_global_slots))
        .saturating_add(slots(&frame.variable_scope_lexicals));
    for name in &frame.variable_scope_lexicals {
        bytes = bytes.saturating_add(name.capacity());
    }
    bytes = bytes
        .saturating_add(map_slots(&frame.templates))
        .saturating_add(
            frame
                .active_module_name
                .as_ref()
                .map_or(0, String::capacity),
        );
    // Primitive buffers have a lease while suspended as well as while a
    // Rust frame owns them; only the remaining slots/metadata are added here.
    bytes.saturating_sub(frame.payload_charge.bytes())
}

impl AsyncContinuation {
    pub(super) fn scheduling_payload_bytes(&self) -> usize {
        suspended_bytes(&self.execution)
            .saturating_add(slots(&self.iterators))
            .saturating_add(values_bytes(&self.iterators))
            .saturating_add(slots(&self.handlers))
            .saturating_add(handler_origins_bytes(&self.handlers))
    }
}
impl ModuleContinuation {
    pub(super) fn scheduling_payload_bytes(&self) -> usize {
        suspended_bytes(&self.execution)
            .saturating_add(slots(&self.iterators))
            .saturating_add(values_bytes(&self.iterators))
            .saturating_add(slots(&self.handlers))
            .saturating_add(handler_origins_bytes(&self.handlers))
            .saturating_add(self.module.capacity())
            .saturating_add(owned_code_bytes(&self.code))
    }
}

fn handler_origins_bytes(handlers: &[HandlerFrame]) -> usize {
    handlers.iter().fold(0usize, |bytes, handler| {
        bytes.saturating_add(handler.pending_origin.payload_bytes())
    })
}

/// A module continuation owns a clone of the outer code unit. Child code and
/// SourceText are shared with the installed program and charged there.
fn owned_code_bytes(code: &Bytecode) -> usize {
    let mut bytes = slots(&code.code)
        .saturating_add(slots(&code.root_statement_offsets))
        .saturating_add(slots(&code.identifier_offsets))
        .saturating_add(slots(&code.root_statement_ranges))
        .saturating_add(slots(&code.root_function_child_indices))
        .saturating_add(slots(&code.root_statement_child_ranges))
        .saturating_add(slots(&code.root_declaration_binding_slots))
        .saturating_add(slots(&code.constants))
        .saturating_add(values_bytes(&code.constants))
        .saturating_add(slots(&code.bindings))
        .saturating_add(slots(&code.scopes))
        .saturating_add(slots(&code.global_function_names))
        .saturating_add(slots(&code.dynamic_eval_slots))
        .saturating_add(slots(&code.functions))
        .saturating_add(slots(&code.captures))
        .saturating_add(code.function_name.capacity())
        .saturating_add(slots(&code.arguments_mapped_slots))
        .saturating_add(slots(&code.templates))
        .saturating_add(slots(&code.handlers))
        .saturating_add(slots(&code.abrupt_jumps))
        .saturating_add(slots(&code.async_yield_delegates))
        .saturating_add(slots(&code.yield_delegates))
        .saturating_add(slots(&code.module_imports))
        .saturating_add(slots(&code.module_exports))
        .saturating_add(slots(&code.module_requests));
    for identifier in &code.identifier_offsets {
        bytes = bytes.saturating_add(identifier.name.capacity());
    }
    for binding in &code.bindings {
        bytes = bytes.saturating_add(binding.name.capacity());
    }
    for scope in &code.scopes {
        bytes = bytes.saturating_add(slots(scope));
    }
    for name in &code.global_function_names {
        bytes = bytes.saturating_add(name.capacity());
    }
    for template in &code.templates {
        bytes = bytes
            .saturating_add(slots(&template.raw))
            .saturating_add(slots(&template.cooked));
        for text in template.raw.iter().chain(template.cooked.iter().flatten()) {
            bytes = bytes.saturating_add(text.owned_heap_payload_bytes().unwrap_or(usize::MAX));
        }
    }
    for import in &code.module_imports {
        bytes = bytes.saturating_add(import.module_request.capacity());
        if let crate::bytecode::ModuleImportName::Named(name) = &import.import_name {
            bytes = bytes.saturating_add(name.capacity());
        }
    }
    for request in &code.module_requests {
        bytes = bytes.saturating_add(request.module_request.capacity());
    }
    for export in &code.module_exports {
        use crate::bytecode::ModuleExport;
        bytes = bytes.saturating_add(match export {
            ModuleExport::Local { export_name, .. } => export_name.capacity(),
            ModuleExport::Indirect {
                export_name,
                module_request,
                import_name,
                ..
            } => export_name
                .capacity()
                .saturating_add(module_request.capacity())
                .saturating_add(import_name.capacity()),
            ModuleExport::Star { module_request, .. } => module_request.capacity(),
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
            } => export_name
                .capacity()
                .saturating_add(module_request.capacity()),
        });
    }
    bytes.saturating_add(
        code.synthetic_default_export
            .as_ref()
            .map_or(0, value_payload_bytes),
    )
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
        self.async_frame_roots.extend(value.object_id());
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

    pub(super) fn finish_host_checkpoint(&mut self) -> Result<(), RuntimeError> {
        // Implicit module drains can run inside an outer execution context;
        // its WeakRef keep-alive guarantee has not ended at that point.
        if self.call_depth != 0 || self.top_level_module || self.module_execution_depth != 0 {
            return Ok(());
        }
        self.report_pending_host_module_errors()?;
        self.notify_host_rejections()?;
        self.kept_weak_objects.clear();
        if self.host_scheduling.is_some()
            && self.own_scheduling_usage().payload_bytes >= MAX_HOST_SCHEDULING_BYTES / 4
        {
            // Promise strings live outside managed heap byte pressure. Give
            // dead owner records a collection opportunity between host tasks
            // before they accumulate into a false scheduling admission limit.
            self.collect_realm_garbage()?;
            self.synchronize_scheduling_usage()?;
        }
        self.enqueue_finalization_cleanup_jobs()?;
        Ok(())
    }

    pub(super) fn own_scheduling_usage(&self) -> HostSchedulingUsage {
        let mut usage = HostSchedulingUsage {
            pending_browser_tasks: self
                .browser_tasks
                .len()
                .saturating_add(self.host_finalization_tasks.len()),
            pending_timers: self.host_timers.len(),
            pending_microtasks: self.promise_jobs.len(),
            pending_reactions: self
                .heap
                .pending_async_generator_requests()
                .saturating_add(self.host_promises.pending.len()),
            payload_bytes: self
                .promise_jobs
                .capacity()
                .saturating_mul(std::mem::size_of::<PromiseJob>())
                .saturating_add(self.active_scheduling_bytes)
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
            .saturating_add(self.debugger_diagnostic_payload_bytes())
            .saturating_add(self.active_diagnostic_frame.as_ref().map_or(
                0,
                crate::diagnostics::ActiveDiagnosticFrame::owned_payload_bytes,
            ));
        usage.payload_bytes = usage
            .payload_bytes
            .saturating_add(slots(&self.module_error_origins));
        for origin in &self.module_error_origins {
            usage.payload_bytes = usage
                .payload_bytes
                .saturating_add(std::mem::size_of::<ThrowOrigin>())
                .saturating_add(2 * std::mem::size_of::<usize>())
                .saturating_add(origin.upgrade().map_or(0, |origin| origin.payload_bytes()));
        }
        usage.payload_bytes = usage.payload_bytes.saturating_add(
            self.host_console
                .as_ref()
                .map_or(0, |console| console.payload_bytes()),
        );
        if let Some(state) = &self.host_rejections {
            usage.pending_reactions = usage
                .pending_reactions
                .saturating_add(state.pending.len())
                .saturating_add(state.active);
            usage.payload_bytes = usage.payload_bytes.saturating_add(
                state.pending.capacity() * std::mem::size_of::<host_rejections::PendingRejection>(),
            );
        }
        usage.payload_bytes = usage.payload_bytes.saturating_add(
            self.host_promises
                .pending
                .capacity()
                .saturating_mul(std::mem::size_of::<(
                    HostPromiseToken,
                    host_promises::PendingHostPromise,
                )>()),
        );
        for job in &self.promise_jobs {
            usage.payload_bytes = usage
                .payload_bytes
                .saturating_add(job.scheduling_payload_bytes());
        }
        usage.payload_bytes = usage
            .payload_bytes
            .saturating_add(map_slots(&self.promise_thunk_origins))
            .saturating_add(
                self.promise_thunk_origins
                    .values()
                    .map(ThrowOrigin::payload_bytes)
                    .sum::<usize>(),
            )
            .saturating_add(
                self.promise_origin_override
                    .as_ref()
                    .map_or(0, ThrowOrigin::payload_bytes),
            );
        for record in self.promises.values() {
            usage.payload_bytes = usage
                .payload_bytes
                .saturating_add(record.rejection_origin.payload_bytes());
            usage.pending_reactions = usage
                .pending_reactions
                .saturating_add(record.reactions.len());
            usage.payload_bytes = usage.payload_bytes.saturating_add(
                record
                    .reactions
                    .capacity()
                    .saturating_mul(std::mem::size_of::<PromiseReaction>()),
            );
            for reaction in &record.reactions {
                usage.payload_bytes = usage.payload_bytes.saturating_add(reaction_bytes(reaction));
            }
            if let PromiseStatus::Fulfilled(value) | PromiseStatus::Rejected(value) = &record.status
            {
                usage.payload_bytes = usage
                    .payload_bytes
                    .saturating_add(value_payload_bytes(value));
            }
        }
        usage.payload_bytes = usage.payload_bytes.saturating_add(
            self.host_timers
                .capacity()
                .saturating_mul(std::mem::size_of::<HostTimer>()),
        );
        for timer in &self.host_timers {
            usage.payload_bytes = usage
                .payload_bytes
                .saturating_add(timer.scheduling_payload_bytes());
        }
        usage.payload_bytes = usage.payload_bytes.saturating_add(
            self.host_animation_frames
                .capacity()
                .saturating_mul(std::mem::size_of::<HostAnimationFrame>()),
        );
        usage.payload_bytes = usage.payload_bytes.saturating_add(
            self.browser_tasks
                .capacity()
                .saturating_mul(std::mem::size_of::<browser_tasks::BrowserTask>()),
        );
        for task in &self.browser_tasks {
            usage.payload_bytes = usage.payload_bytes.saturating_add(match task {
                browser_tasks::BrowserTask::DragString { text, .. } => {
                    text.owned_heap_payload_bytes().unwrap_or(usize::MAX)
                }
                browser_tasks::BrowserTask::HostPromise { completion, .. } => {
                    completion.payload_bytes()
                }
                browser_tasks::BrowserTask::PromiseRejections { entries, .. } => {
                    usage.pending_reactions = usage.pending_reactions.saturating_add(entries.len());
                    entries.capacity() * std::mem::size_of::<host_rejections::PendingRejection>()
                }
                _ => 0,
            });
        }
        usage.payload_bytes = usage.payload_bytes.saturating_add(
            self.host_finalization_tasks
                .capacity()
                .saturating_mul(std::mem::size_of::<host_finalization::HostFinalizationTask>()),
        );
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
        usage.payload_bytes = usage
            .payload_bytes
            .saturating_add(map_slots(&self.promises))
            .saturating_add(map_slots(&self.async_continuations))
            .saturating_add(map_slots(&self.module_continuations))
            .saturating_add(map_slots(&self.module_import_waiters));
        for (name, waiters) in &self.module_import_waiters {
            usage.pending_reactions = usage.pending_reactions.saturating_add(waiters.len());
            usage.payload_bytes = usage
                .payload_bytes
                .saturating_add(name.capacity())
                .saturating_add(slots(waiters));
        }
        usage.payload_bytes = usage
            .payload_bytes
            .saturating_add(slots(&self.deferred_import_waiters));
        for waiter in &self.deferred_import_waiters {
            usage.pending_reactions = usage.pending_reactions.saturating_add(1);
            usage.payload_bytes = usage.payload_bytes.saturating_add(
                waiter
                    .pending
                    .capacity()
                    .saturating_mul(std::mem::size_of::<String>()),
            );
            for name in &waiter.pending {
                usage.payload_bytes = usage.payload_bytes.saturating_add(name.capacity());
            }
        }
        usage.payload_bytes = usage
            .payload_bytes
            .saturating_add(self.module_dependency_bytes());
        usage
    }
    pub(super) fn admit_scheduling_work(
        &mut self,
        extra: HostSchedulingUsage,
    ) -> Result<(), RuntimeError> {
        if self.host_scheduling.is_none() {
            return Ok(());
        }
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
        if self
            .host_scheduling
            .as_ref()
            .is_some_and(|scope| scope.account.borrow().failed)
        {
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
    /// All Promise/await/thenable/finalizer and host producers use this gate.
    /// Failure latches an uncatchable document abort before any later JS work;
    /// infallible internal settlement paths must never panic or report success.
    pub(super) fn enqueue_promise_job(&mut self, job: PromiseJob) {
        let extra = HostSchedulingUsage {
            pending_microtasks: 1,
            payload_bytes: job.scheduling_payload_bytes(),
            ..HostSchedulingUsage::default()
        };
        let growth = SchedulingCapacity::for_deque_push(&self.promise_jobs);
        if self
            .admit_scheduling_work(extra.with_growth(growth))
            .is_err()
        {
            return;
        }
        if growth.reserve_deque(&mut self.promise_jobs)
            && self.admit_scheduling_work(extra).is_err()
        {
            return;
        }
        self.promise_jobs.push_back(job);
        let _ = self.synchronize_scheduling_usage();
    }
    pub(super) fn enqueue_promise_reaction(
        &mut self,
        promise: ObjectId,
        reaction: PromiseReaction,
    ) {
        self.mark_promise_handled(promise);
        let extra = HostSchedulingUsage {
            pending_reactions: 1,
            payload_bytes: reaction_bytes(&reaction),
            ..HostSchedulingUsage::default()
        };
        let growth = SchedulingCapacity::for_vec_push(
            &self
                .promises
                .get(&promise)
                .expect("reaction owner was validated")
                .reactions,
        );
        if self
            .admit_scheduling_work(extra.with_growth(growth))
            .is_err()
        {
            return;
        }
        let exceeded = growth.reserve_vec(&mut self.promises.get_mut(&promise).unwrap().reactions);
        if exceeded && self.admit_scheduling_work(extra).is_err() {
            return;
        }
        self.promises
            .get_mut(&promise)
            .expect("reaction owner was validated")
            .reactions
            .push(reaction);
        let _ = self.synchronize_scheduling_usage();
    }
}
