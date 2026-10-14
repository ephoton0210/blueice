// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

//! Page-owned scheduling admission shared by a realm and its ShadowRealms.
//! Standalone ECMAScript VMs retain their unbounded host-neutral job policy.

use super::*;
use std::cell::RefCell;

pub const MAX_PENDING_HOST_MICROTASKS: usize = 1024;
pub const MAX_PENDING_HOST_REACTIONS: usize = 1024;
pub const MAX_HOST_SCHEDULING_BYTES: usize = 8 * 1024 * 1024;

/// Owned scheduling slot capacities and payloads, separate from managed heap
/// and shared installed bytecode. Allocator/control-table overhead, spare
/// BigInt limb capacity and process RSS are not represented by this ledger.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct HostSchedulingUsage {
    pub pending_microtasks: usize,
    pub pending_reactions: usize,
    pub payload_bytes: usize,
}

impl HostSchedulingUsage {
    fn add(self, other: Self) -> Self {
        Self {
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
        self.pending_microtasks <= MAX_PENDING_HOST_MICROTASKS
            && self.pending_reactions <= MAX_PENDING_HOST_REACTIONS
            && self.payload_bytes <= MAX_HOST_SCHEDULING_BYTES
    }
}

#[derive(Default)]
struct Account {
    usage: HostSchedulingUsage,
    failed: bool,
}

pub(super) struct SchedulingScope {
    account: Rc<RefCell<Account>>,
    own: HostSchedulingUsage,
}

impl SchedulingScope {
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
    fn check(
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
fn capability_bytes(target: &ReactionTarget) -> usize {
    match target {
        ReactionTarget::Native(_) => 0,
        ReactionTarget::Capability(cap) => value_payload_bytes(&cap.promise)
            .saturating_add(value_payload_bytes(&cap.resolve))
            .saturating_add(value_payload_bytes(&cap.reject)),
    }
}
impl PromiseJob {
    pub(super) fn scheduling_payload_bytes(&self) -> usize {
        match self {
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
        }
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
fn values_bytes(values: &[Value]) -> usize {
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
fn completion_payload_bytes(completion: &Completion) -> usize {
    match completion {
        Completion::Return(value) | Completion::Yield(value) | Completion::Halt(value) => {
            value_payload_bytes(value)
        }
        Completion::TailCall(values) | Completion::TailRecur(values) => {
            slots(values).saturating_add(values_bytes(values))
        }
        Completion::Throw(RuntimeError::Thrown(value)) => value_payload_bytes(value),
        Completion::Throw(
            RuntimeError::ReferenceError(message)
            | RuntimeError::TypeError(message)
            | RuntimeError::RangeError(message)
            | RuntimeError::SyntaxError(message)
            | RuntimeError::Test262(message)
            | RuntimeError::RegexWorker(message)
            | RuntimeError::ModuleResolution(message),
        ) => message.capacity(),
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
    bytes
}

impl AsyncContinuation {
    pub(super) fn scheduling_payload_bytes(&self) -> usize {
        suspended_bytes(&self.execution)
            .saturating_add(slots(&self.iterators))
            .saturating_add(values_bytes(&self.iterators))
            .saturating_add(slots(&self.handlers))
    }
}
impl ModuleContinuation {
    pub(super) fn scheduling_payload_bytes(&self) -> usize {
        suspended_bytes(&self.execution)
            .saturating_add(slots(&self.iterators))
            .saturating_add(values_bytes(&self.iterators))
            .saturating_add(slots(&self.handlers))
            .saturating_add(self.module.capacity())
            .saturating_add(owned_code_bytes(&self.code))
    }
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
    fn own_scheduling_usage(&self) -> HostSchedulingUsage {
        let mut usage = HostSchedulingUsage {
            pending_microtasks: self.promise_jobs.len(),
            pending_reactions: self.heap.pending_async_generator_requests(),
            payload_bytes: self
                .promise_jobs
                .capacity()
                .saturating_mul(std::mem::size_of::<PromiseJob>())
                .saturating_add(self.active_scheduling_bytes),
        };
        for job in &self.promise_jobs {
            usage.payload_bytes = usage
                .payload_bytes
                .saturating_add(job.scheduling_payload_bytes());
        }
        for record in self.promises.values() {
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
            if let browser_tasks::BrowserTask::DragString { text, .. } = task {
                usage.payload_bytes = usage
                    .payload_bytes
                    .saturating_add(text.owned_heap_payload_bytes().unwrap_or(usize::MAX));
            }
        }
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
        self.host_scheduling.as_mut().unwrap().check(own, extra)
    }
    pub(super) fn synchronize_scheduling_usage(&mut self) -> Result<(), RuntimeError> {
        self.admit_scheduling_work(HostSchedulingUsage::default())
    }
    pub(super) fn check_scheduling_failure(&self) -> Result<(), RuntimeError> {
        if self
            .host_scheduling
            .as_ref()
            .is_some_and(|scope| scope.account.borrow().failed)
        {
            Err(RuntimeError::SchedulingLimit)
        } else {
            Ok(())
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
        if self
            .admit_scheduling_work(HostSchedulingUsage {
                pending_microtasks: 1,
                payload_bytes: job
                    .scheduling_payload_bytes()
                    .saturating_add(4 * std::mem::size_of::<PromiseJob>()),
                ..HostSchedulingUsage::default()
            })
            .is_err()
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
        if self
            .admit_scheduling_work(HostSchedulingUsage {
                pending_reactions: 1,
                payload_bytes: reaction_bytes(&reaction)
                    .saturating_add(4 * std::mem::size_of::<PromiseReaction>()),
                ..HostSchedulingUsage::default()
            })
            .is_err()
        {
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
