// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

//! Host operations retain opaque completion capabilities. Promise objects,
//! roots and settlement stay in the owning realm and document task scheduler.

use super::*;

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

pub(super) struct PendingHostPromise {
    promise: ObjectId,
    root: RootId,
    function: u32,
    queued: bool,
}

#[derive(Default)]
pub(super) struct HostPromiseState {
    functions: Vec<Registration>,
    pub(super) pending: HashMap<HostPromiseToken, PendingHostPromise>,
}

impl Drop for HostPromiseState {
    fn drop(&mut self) {
        for (token, pending) in &self.pending {
            self.functions[pending.function as usize]
                .operation
                .cancel(*token);
        }
    }
}

impl Vm {
    pub fn install_host_async_function(
        &mut self,
        name: &str,
        length: u32,
        source: HostTaskSource,
        function: impl HostAsyncFunction,
    ) -> Result<(), RuntimeError> {
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
            Operation::Function(Box::new(function)),
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
            Operation::Method {
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
        operation: Operation,
    ) -> Result<(), RuntimeError> {
        let index = host_function_index(self.host_promises.functions.len())?;
        self.install_host_callable_native(owner, name, length, NativeFunction::HostAsync(index))?;
        self.host_promises
            .functions
            .push(Registration { source, operation });
        Ok(())
    }

    fn remove_host_promise(&mut self, token: HostPromiseToken) -> Option<PendingHostPromise> {
        let pending = self.host_promises.pending.remove(&token);
        if self.host_promises.pending.is_empty() {
            self.host_promises.pending.shrink_to_fit();
        }
        pending
    }

    pub(super) fn host_async_function_call(
        &mut self,
        index: u32,
        receiver: Value,
        args: &[Value],
        construct: bool,
    ) -> Result<Value, RuntimeError> {
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
        let bytes = args
            .iter()
            .fold(0usize, |total, value| {
                total.saturating_add(host_scheduling::value_payload_bytes(value))
            })
            .saturating_add(args.len().saturating_mul(std::mem::size_of::<HostValue>()));
        self.admit_scheduling_work(HostSchedulingUsage {
            pending_reactions: 1,
            payload_bytes: bytes
                .saturating_add(4 * std::mem::size_of::<(HostPromiseToken, PendingHostPromise)>()),
            ..HostSchedulingUsage::default()
        })?;
        let token = HostPromiseToken {
            heap: self.object_prototype.heap,
            serial: self.next_host_task_serial()?,
        };
        let promise = self.new_promise()?;
        let root = self.heap.root(promise)?;
        self.host_promises.pending.insert(
            token,
            PendingHostPromise {
                promise,
                root,
                function: index,
                queued: false,
            },
        );
        self.active_scheduling_bytes = self.active_scheduling_bytes.saturating_add(bytes);
        let started = (|| {
            self.synchronize_scheduling_usage()?;
            let family = match &self.host_promises.functions[index as usize].operation {
                Operation::Function(_) => None,
                Operation::Method { family, .. } => Some(*family),
            };
            let key = family
                .map(|family| self.host_receiver_key(family, &receiver))
                .transpose()?;
            // Match the admitted slot count: Result collection can grow the
            // vector geometrically even when the input length is known.
            let mut arguments = Vec::with_capacity(args.len());
            for value in args {
                arguments.push(
                    HostValue::try_from(value)
                        .map_err(|error| RuntimeError::TypeError(error.to_string()))?,
                );
            }
            match &mut self.host_promises.functions[index as usize].operation {
                Operation::Function(function) => function.start(token, &arguments),
                Operation::Method { method, .. } => {
                    method.start(token, key.expect("verified receiver"), &arguments)
                }
            }
            .map_err(|error| RuntimeError::TypeError(error.to_string()))
        })();
        self.active_scheduling_bytes = self.active_scheduling_bytes.saturating_sub(bytes);
        if let Err(error) = started {
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
        Ok(Value::Object(promise))
    }

    pub fn queue_host_promise_completion(
        &mut self,
        token: HostPromiseToken,
        completion: HostPromiseCompletion,
    ) -> Result<bool, RuntimeError> {
        self.check_scheduling_failure()?;
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
        self.reserve_browser_task(HostSchedulingUsage {
            payload_bytes: completion.payload_bytes(),
            ..HostSchedulingUsage::default()
        })?;
        let serial = self.next_host_task_serial()?;
        self.host_promises
            .pending
            .get_mut(&token)
            .expect("validated wait")
            .queued = true;
        self.browser_tasks
            .push_back(browser_tasks::BrowserTask::HostPromise {
                token,
                completion,
                source,
                serial,
            });
        self.synchronize_scheduling_usage()?;
        Ok(true)
    }

    pub(super) fn run_host_promise_task(
        &mut self,
        token: HostPromiseToken,
        completion: HostPromiseCompletion,
        completion_payload: execution_payload::ExecutionPayloadCharge,
    ) -> Result<(), RuntimeError> {
        let Some(pending) = self.remove_host_promise(token) else {
            return Ok(());
        };
        let result = (|| {
            self.synchronize_scheduling_usage()?;
            match completion {
                HostPromiseCompletion::Resolve(value) => {
                    let value = value.into();
                    drop(completion_payload);
                    self.resolve_promise(pending.promise, value)
                }
                HostPromiseCompletion::Reject(value) => {
                    let value = value.into();
                    drop(completion_payload);
                    self.settle_promise(pending.promise, PromiseStatus::Rejected(value))
                }
                HostPromiseCompletion::RejectError(error) => {
                    let value = self.error_value(RuntimeError::TypeError(error.0))?;
                    drop(completion_payload);
                    self.settle_promise(pending.promise, PromiseStatus::Rejected(value))
                }
                HostPromiseCompletion::ResolveObject { family, key } => {
                    let object = self.mint_host_family_wrapper(family, key)?;
                    drop(completion_payload);
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
