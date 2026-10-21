// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

//! Host operations retain opaque completion capabilities. Promise objects,
//! roots and settlement stay in the owning realm and document task scheduler.

use super::host_objects::RegistrationRecord;
use super::host_scheduling::SharedSchedulingCharge;
use super::*;

#[cfg(any(test, coverage))]
#[path = "../../tests/fixtures/host_wait_map_storage.rs"]
mod map_storage_tests;
#[cfg(any(test, coverage))]
#[path = "../../tests/fixtures/host_async_registration_storage.rs"]
mod registration_tests;

/// An external completion's logical source, chosen by the embedding owner.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum HostTaskSource {
    Networking,
    UserInteraction,
}

/// A copyable completion capability. It contains no JavaScript value or root,
/// cannot be constructed by script, and authorizes only its creating realm.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct HostPromiseToken {
    heap: u64,
    serial: u64,
}

/// An asynchronous primitive-input capability. Returning successfully starts
/// the operation; it does not settle the returned JavaScript Promise.
pub trait HostAsyncFunction: 'static {
    fn start(
        &mut self,
        token: HostPromiseToken,
        args: &[HostValue],
    ) -> Result<(), HostFunctionError>;

    /// The realm abandoned an unfinished wait. Implementations release or
    /// cancel external work here; a completion might already be in flight.
    fn cancel(&mut self, _token: HostPromiseToken) {}
}

impl<F> HostAsyncFunction for F
where
    F: for<'args> FnMut(HostPromiseToken, &'args [HostValue]) -> Result<(), HostFunctionError>
        + 'static,
{
    fn start(
        &mut self,
        token: HostPromiseToken,
        args: &[HostValue],
    ) -> Result<(), HostFunctionError> {
        self(token, args)
    }
}

/// A promise-returning operation on a VM-verified private wrapper. Only the
/// wrapper's opaque owner key and primitive arguments reach the embedder.
pub trait HostAsyncObjectMethod: 'static {
    fn start(
        &mut self,
        token: HostPromiseToken,
        key: HostObjectKey,
        args: &[HostValue],
    ) -> Result<(), HostFunctionError>;
    fn cancel(&mut self, _token: HostPromiseToken) {}
}

impl<F> HostAsyncObjectMethod for F
where
    F: for<'args> FnMut(
            HostPromiseToken,
            HostObjectKey,
            &'args [HostValue],
        ) -> Result<(), HostFunctionError>
        + 'static,
{
    fn start(
        &mut self,
        token: HostPromiseToken,
        key: HostObjectKey,
        args: &[HostValue],
    ) -> Result<(), HostFunctionError> {
        self(token, key, args)
    }
}

/// Data for one later settlement task. A wrapper completion lets a request
/// return a realm-owned response/body facade without exporting an object ID.
#[derive(Debug, Clone, PartialEq)]
pub enum HostPromiseCompletion {
    Resolve(HostValue),
    Reject(HostValue),
    RejectError(HostFunctionError),
    ResolveObject {
        family: HostObjectFamily,
        key: HostObjectKey,
    },
}

impl HostPromiseCompletion {
    pub(super) fn payload_bytes(&self) -> usize {
        match self {
            Self::Resolve(HostValue::String(value)) | Self::Reject(HostValue::String(value)) => {
                value.owned_heap_payload_bytes().unwrap_or(usize::MAX)
            }
            Self::RejectError(error) => error.0.capacity(),
            _ => 0,
        }
    }
}

enum Operation {
    Function(Box<dyn HostAsyncFunction>),
    Method {
        family: HostObjectFamily,
        method: Box<dyn HostAsyncObjectMethod>,
    },
}

impl Operation {
    fn cancel(&mut self, token: HostPromiseToken) {
        match self {
            Self::Function(function) => function.cancel(token),
            Self::Method { method, .. } => method.cancel(token),
        }
    }
}

struct Registration {
    source: HostTaskSource,
    operation: Operation,
}

impl RegistrationRecord for Registration {
    fn callback_bytes(&self) -> usize {
        match &self.operation {
            Operation::Function(function) => std::mem::size_of_val(&**function),
            Operation::Method { method, .. } => std::mem::size_of_val(&**method),
        }
    }
}

pub(super) struct PendingHostPromise {
    promise: ObjectId,
    root: RootId,
    function: u32,
    queued: bool,
    // Cancellation identity follows the request even outside its native call.
    entry_owner: Option<SharedSchedulingCharge>,
    // The wait belongs to the registry's original document until retirement.
    count_storage: Option<SharedSchedulingCharge>,
}

#[derive(Default)]
pub(super) struct HostPromiseState {
    functions: HostRegistrations<Registration>,
    pub(super) pending:
        retained_control_map::RetainedControlMap<HostPromiseToken, PendingHostPromise>,
}

impl HostPromiseState {
    pub(super) fn unowned_wait_count(&self) -> usize {
        self.pending
            .values()
            .filter(|wait| wait.count_storage.is_none())
            .count()
    }

    pub(super) fn counts_can_join(&self, scope: &SchedulingScope) -> bool {
        self.pending.values().all(|wait| {
            wait.count_storage
                .as_ref()
                .is_none_or(|owner| owner.belongs_to(scope))
        })
    }

    pub(super) fn validate_counts(&self) -> Result<(), RuntimeError> {
        for wait in self.pending.values() {
            if let Some(owner) = &wait.count_storage {
                owner.reserve_peer(0)?;
            }
        }
        Ok(())
    }
}

impl Drop for HostPromiseState {
    fn drop(&mut self) {
        for (token, pending) in self.pending.iter() {
            self.functions[pending.function as usize]
                .operation
                .cancel(*token);
        }
    }
}

struct HostAsyncContext {
    caller: Option<SharedSchedulingCharge>,
    original: Option<SharedSchedulingCharge>,
}

impl HostAsyncContext {
    fn capture(vm: &mut Vm) -> Result<Self, RuntimeError> {
        let caller = vm.argument_list_owner()?;
        let original = vm
            .host_promises
            .functions
            .retain_for_entry(caller.as_ref())?;
        Ok(Self { caller, original })
    }
    fn registration_owner(&self) -> Option<&SharedSchedulingCharge> {
        self.original.as_ref().or(self.caller.as_ref())
    }
    fn entry_owner(&self) -> Result<Option<SharedSchedulingCharge>, RuntimeError> {
        self.caller
            .as_ref()
            .or(self.original.as_ref())
            .map(|owner| owner.reserve_peer(0))
            .transpose()
    }
    fn validate(&self, vm: &Vm) -> Result<(), RuntimeError> {
        for owner in [self.caller.as_ref(), self.original.as_ref()]
            .into_iter()
            .flatten()
        {
            owner.reserve_peer(0)?;
        }
        vm.argument_list_owner()?;
        Ok(())
    }
}

struct HostCompletionPublication {
    completion: HostPromiseCompletion,
    task: browser_task_storage::PreparedBrowserTask,
}

struct HostCompletionDelivery {
    completion: HostPromiseCompletion,
    // Every refusal destroys the content before returning its native credit.
    payload: execution_payload::ExecutionPayloadCharge,
}

impl Vm {
    pub(super) fn retain_host_wait_counts(
        &mut self,
        hint: Option<&SharedSchedulingCharge>,
    ) -> Result<(), RuntimeError> {
        let original = self.host_promises.pending.owner_view()?;
        self.host_promises.validate_counts()?;
        if let Some(scope) = &self.host_scheduling {
            if !self.host_promises.counts_can_join(scope)
                || original
                    .as_ref()
                    .is_some_and(|owner| !owner.belongs_to(scope))
                || hint.is_some_and(|owner| !owner.belongs_to(scope))
            {
                return Err(scope.fail());
            }
        }
        let count = self.host_promises.unowned_wait_count();
        let Some(owner) = original.as_ref().or(hint).filter(|_| count != 0) else {
            return Ok(());
        };
        self.synchronize_scheduling_usage()?;
        let mut storage = match &mut self.host_scheduling {
            Some(scope) => scope.detach_owned_reactions(count),
            None => owner.reserve_reactions(count)?,
        };
        for wait in self.host_promises.pending.values_mut() {
            if wait.count_storage.is_none() {
                wait.count_storage = Some(storage.split_reactions(1));
            }
        }
        Ok(())
    }

    pub(super) fn host_async_registration_storage_can_join(&self, scope: &SchedulingScope) -> bool {
        self.host_promises.functions.can_join(scope)
    }
    pub(super) fn attach_host_async_registration_storage(
        &mut self,
        scope: &SchedulingScope,
    ) -> Result<(), RuntimeError> {
        self.host_promises.functions.attach(scope)
    }
    pub(super) fn unowned_host_async_registration_storage_bytes(&self) -> usize {
        self.host_promises.functions.unowned_bytes()
    }

    #[cfg(any(test, coverage))]
    pub(super) fn host_completion_queued_for_test(&self, token: HostPromiseToken) -> bool {
        self.host_promises.pending[&token].queued
    }

    pub fn install_host_async_function(
        &mut self,
        name: &str,
        length: u32,
        source: HostTaskSource,
        function: impl HostAsyncFunction,
    ) -> Result<(), RuntimeError> {
        self.argument_list_owner()?;
        self.host_promises.functions.owner_view()?;
        let global = self
            .global("globalThis")?
            .object_id()
            .expect("global object");
        if !host_property_name_is_valid(name)
            || execution::LAZY_STANDARD_GLOBALS.contains(&name)
            || matches!(name, "undefined" | "NaN" | "Infinity")
            || self.heap.get_own(global, name)?.is_some()
        {
            return Err(RuntimeError::TypeError(
                "host global name is invalid or already defined".into(),
            ));
        }
        self.register_host_async_function(
            global,
            name,
            length,
            source,
            std::mem::size_of_val(&function),
            move || Operation::Function(Box::new(function)),
        )
    }

    pub fn install_host_async_object_method(
        &mut self,
        family: HostObjectFamily,
        name: &str,
        length: u32,
        source: HostTaskSource,
        method: impl HostAsyncObjectMethod,
    ) -> Result<(), RuntimeError> {
        self.argument_list_owner()?;
        self.host_promises.functions.owner_view()?;
        let prototype = self.host_family_prototype(family)?;
        if !host_property_name_is_valid(name) || self.heap.get_own(prototype, name)?.is_some() {
            return Err(RuntimeError::TypeError(
                "host method name is invalid or already defined".into(),
            ));
        }
        self.register_host_async_function(
            prototype,
            name,
            length,
            source,
            std::mem::size_of_val(&method),
            move || Operation::Method {
                family,
                method: Box::new(method),
            },
        )
    }

    fn register_host_async_function(
        &mut self,
        owner: ObjectId,
        name: &str,
        length: u32,
        source: HostTaskSource,
        callback_bytes: usize,
        make: impl FnOnce() -> Operation,
    ) -> Result<(), RuntimeError> {
        let context = HostAsyncContext::capture(self)?;
        let index = host_function_index(self.host_promises.functions.len())?;
        let prepared = self
            .host_promises
            .functions
            .prepare(1, context.registration_owner())?;
        let callbacks = prepared.callbacks(&[callback_bytes], || {
            [Registration {
                source,
                operation: make(),
            }]
        })?;
        context.validate(self)?;
        self.install_host_callable_native(owner, name, length, NativeFunction::HostAsync(index))?;
        self.host_promises.functions.commit(prepared, callbacks);
        Ok(())
    }

    fn remove_host_promise(&mut self, token: HostPromiseToken) -> Option<PendingHostPromise> {
        let mut pending = self.host_promises.pending.remove(&token);
        if let Some(storage) = pending
            .as_mut()
            .and_then(|wait| wait.count_storage.as_mut())
        {
            // Settlement/cancellation has retired the external wait even when
            // its root and completion remain live during the consumer call.
            storage.release_reactions_to(0);
        }
        if self.host_promises.pending.is_empty() {
            self.host_promises.pending.release_empty();
        }
        pending
    }

    fn cancel_host_promise(&mut self, token: HostPromiseToken) {
        let Some(pending) = self.remove_host_promise(token) else {
            return;
        };
        self.host_promises.functions[pending.function as usize]
            .operation
            .cancel(token);
        self.browser_tasks.retain(|task| !matches!(task, browser_tasks::BrowserTask::HostPromise { token: queued, .. } if *queued == token));
        self.release_root(pending.root);
        self.reconcile_scheduling_ownership();
    }

    pub(super) fn check_host_promise_owner(
        &mut self,
        token: HostPromiseToken,
    ) -> Result<(), RuntimeError> {
        let owners = self
            .host_promises
            .functions
            .owner_view()
            .and_then(|_| self.validate_promise_map_owners());
        if let Err(error) = owners {
            // The complete registry belongs to this original document. Retire
            // its waits without allocating a snapshot during cancellation.
            loop {
                let next = self.host_promises.pending.keys().next().copied();
                let Some(token) = next else {
                    break;
                };
                self.cancel_host_promise(token);
            }
            return Err(error);
        }
        let entry = self
            .host_promises
            .pending
            .get(&token)
            .and_then(|pending| pending.entry_owner.as_ref())
            .map(|owner| owner.reserve_peer(0))
            .transpose();
        if let Err(error) = entry {
            self.cancel_host_promise(token);
            return Err(error);
        }
        Ok(())
    }

    pub(super) fn host_async_function_call(
        &mut self,
        index: u32,
        receiver: Value,
        args: &[Value],
        construct: bool,
    ) -> Result<Value, RuntimeError> {
        let context = HostAsyncContext::capture(self)?;
        if construct {
            return Err(RuntimeError::TypeError(
                "host async functions are not constructors".into(),
            ));
        }
        if index as usize >= self.host_promises.functions.len() {
            return Err(RuntimeError::TypeError(
                "host async function is unavailable".into(),
            ));
        }
        let before = self.heap.stats();
        let result = self.host_async_function_call_prepared(index, receiver, args, &context);
        self.finish_promise_record_attempt(before);
        result
    }

    fn host_async_function_call_prepared(
        &mut self,
        index: u32,
        receiver: Value,
        args: &[Value],
        context: &HostAsyncContext,
    ) -> Result<Value, RuntimeError> {
        self.retain_host_wait_map_storage(context.registration_owner())?;
        let count_owner = self.host_promises.pending.owner_view()?;
        let count_storage = count_owner
            .as_ref()
            .or(context.registration_owner())
            .map(|owner| owner.reserve_reactions(1))
            .transpose()?;
        self.heap.check_root_registration_count(1)?;
        let mut arguments = if context.caller.is_some() || context.original.is_none() {
            self.prepare_host_wait_arguments(args)?
        } else {
            self.prepare_host_arguments(args, context.registration_owner())?
        };
        let waits = promise_queue_storage::PreparedMap::for_insert(
            &self.host_promises.pending,
            self.host_scheduling.as_ref(),
        )?;
        let promises = promise_queue_storage::PreparedMap::for_insert(
            &self.promises,
            self.host_scheduling.as_ref(),
        )?;
        let prepared = self.prepare_promise_record_with_capacity(promises, None)?;
        let promise = prepared.promise;
        let objects = [promise];
        let roots = self.heap.prepare_root_batch(&objects)?;
        self.check_scheduling_failure()?;
        let entry_owner = context.entry_owner()?;
        context.validate(self)?;
        self.validate_promise_map_owners()?;
        // The token is the final fallible step. No JavaScript or collection
        // runs while publishing the admitted root, record and wait owners.
        let token = HostPromiseToken {
            heap: self.object_prototype.heap,
            serial: self.next_host_task_serial()?,
        };
        let root = self.heap.commit_single_root(roots);
        prepared.commit(self);
        waits.commit(&mut self.host_promises.pending);
        self.host_promises.pending.insert(
            token,
            PendingHostPromise {
                promise,
                root,
                function: index,
                queued: false,
                entry_owner,
                count_storage,
            },
        );
        self.reconcile_scheduling_ownership();
        let mut started_successfully = false;
        let started = (|| {
            let family = match &self.host_promises.functions[index as usize].operation {
                Operation::Function(_) => None,
                Operation::Method { family, .. } => Some(*family),
            };
            let key = family
                .map(|family| self.host_receiver_key(family, &receiver))
                .transpose()?;
            if let Some(error) = arguments.error.take() {
                return Err(error);
            }
            let result = match &mut self.host_promises.functions[index as usize].operation {
                Operation::Function(function) => function.start(token, &arguments.values),
                Operation::Method { method, .. } => {
                    method.start(token, key.expect("verified receiver"), &arguments.values)
                }
            };
            started_successfully = result.is_ok();
            context.validate(self)?;
            self.validate_promise_map_owners()?;
            let owner = arguments.owner_view()?;
            self.argument_list_owner()?;
            self.finish_host_callback(result, owner.as_ref())
        })();
        drop(arguments);
        if let Err(error) = started {
            if started_successfully {
                self.host_promises.functions[index as usize]
                    .operation
                    .cancel(token);
            }
            let result = if host_timers::is_page_exception(&error) {
                self.error_value(error)
                    .and_then(|error| self.settle_promise(promise, PromiseStatus::Rejected(error)))
            } else {
                Err(error)
            };
            self.remove_host_promise(token);
            self.release_root(root);
            self.synchronize_scheduling_usage()?;
            result?;
        } else {
            self.synchronize_scheduling_usage()?;
        }
        context.validate(self)?;
        self.validate_promise_map_owners()?;
        Ok(Value::Object(promise))
    }

    pub fn queue_host_promise_completion(
        &mut self,
        token: HostPromiseToken,
        completion: HostPromiseCompletion,
    ) -> Result<bool, RuntimeError> {
        self.check_scheduling_failure()?;
        self.argument_list_owner()?;
        self.check_host_promise_owner(token)?;
        let Some(pending) = self.host_promises.pending.get(&token) else {
            return Ok(false);
        };
        if token.heap != self.object_prototype.heap || pending.queued {
            return Ok(false);
        }
        let source = self.host_promises.functions[pending.function as usize].source;
        match &completion {
            HostPromiseCompletion::Resolve(HostValue::String(text))
            | HostPromiseCompletion::Reject(HostValue::String(text))
                if text.byte_len() > self.config.max_string_bytes =>
            {
                return Err(RuntimeError::StringLimit {
                    limit: self.config.max_string_bytes,
                });
            }
            HostPromiseCompletion::RejectError(error)
                if error.0.encode_utf16().count().saturating_mul(2)
                    > self.config.max_string_bytes =>
            {
                return Err(RuntimeError::StringLimit {
                    limit: self.config.max_string_bytes,
                });
            }
            HostPromiseCompletion::ResolveObject { family, .. } => {
                self.host_family_prototype(*family)?;
            }
            _ => {}
        }
        let owner = self.host_promises.pending.owner_view()?;
        let task = self.prepare_browser_task_with_owner(
            HostSchedulingUsage {
                payload_bytes: completion.payload_bytes(),
                ..HostSchedulingUsage::default()
            },
            owner.as_ref(),
        )?;
        let input = HostCompletionPublication { completion, task };
        self.check_host_promise_owner(token)?;
        self.argument_list_owner()?;
        let serial = self.next_host_task_serial()?;
        self.host_promises
            .pending
            .get_mut(&token)
            .expect("validated wait")
            .queued = true;
        input.task.commit(
            self,
            browser_tasks::BrowserTask::HostPromise {
                token,
                completion: input.completion,
                source,
                serial,
            },
        );
        self.synchronize_scheduling_usage()?;
        Ok(true)
    }

    pub(super) fn run_host_promise_task(
        &mut self,
        token: HostPromiseToken,
        completion: HostPromiseCompletion,
        completion_payload: execution_payload::ExecutionPayloadCharge,
    ) -> Result<(), RuntimeError> {
        let input = HostCompletionDelivery {
            completion,
            payload: completion_payload,
        };
        self.check_host_promise_owner(token)?;
        let Some(pending) = self.remove_host_promise(token) else {
            return Ok(());
        };
        let result = (|| {
            self.synchronize_scheduling_usage()?;
            match input.completion {
                HostPromiseCompletion::Resolve(value) => {
                    let value = value.into();
                    drop(input.payload);
                    self.resolve_promise(pending.promise, value)
                }
                HostPromiseCompletion::Reject(value) => {
                    let value = value.into();
                    drop(input.payload);
                    self.settle_promise(pending.promise, PromiseStatus::Rejected(value))
                }
                HostPromiseCompletion::RejectError(error) => {
                    let value = self.error_value(RuntimeError::TypeError(error.0))?;
                    drop(input.payload);
                    self.settle_promise(pending.promise, PromiseStatus::Rejected(value))
                }
                HostPromiseCompletion::ResolveObject { family, key } => {
                    let object = self.mint_host_family_wrapper(family, key)?;
                    drop(input.payload);
                    self.resolve_promise(pending.promise, Value::Object(object))
                }
            }
        })();
        let result = match result {
            Err(error) if host_timers::is_page_exception(&error) => {
                self.error_value(error).and_then(|value| {
                    self.settle_promise(pending.promise, PromiseStatus::Rejected(value))
                })
            }
            result => result,
        };
        self.release_root(pending.root);
        self.synchronize_scheduling_usage()?;
        result
    }
}
