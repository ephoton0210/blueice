// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

use super::*;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum HostFileSelectionEvent {
    Input,
    Change,
    Cancel,
}
impl HostFileSelectionEvent {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Input => "input",
            Self::Change => "change",
            Self::Cancel => "cancel",
        }
    }
    pub(super) fn from_value(value: &Value) -> Result<Self, RuntimeError> {
        match value {
            Value::String(value) if value == "input" => Ok(Self::Input),
            Value::String(value) if value == "change" => Ok(Self::Change),
            Value::String(value) if value == "cancel" => Ok(Self::Cancel),
            _ => Err(RuntimeError::TypeError(
                "Unsupported file selection event".into(),
            )),
        }
    }
}

impl Vm {
    /// Keeps callbacks in the VM and enables only the browser's click and
    /// file-selection event vocabulary on this exact wrapper family.
    pub fn install_host_file_selection_event_methods(
        &mut self,
        family: HostObjectFamily,
    ) -> Result<(), RuntimeError> {
        self.install_host_click_event_methods(family)?;
        self.host_object_families[family.index as usize].file_events = true;
        Ok(())
    }

    /// Delivers an owner-validated live ancestry path. It creates no native
    /// selection capability and cannot dispatch into another wrapper family.
    pub fn dispatch_host_file_selection_event(
        &mut self,
        family: HostObjectFamily,
        path: &[HostObjectKey],
        kind: HostFileSelectionEvent,
    ) -> Result<(), RuntimeError> {
        self.ensure_no_debugger_continuation()?;
        self.host_family_prototype(family)?;
        if !self.host_object_families[family.index as usize].file_events
            || path.is_empty()
            || path.len() > 256
            || path
                .iter()
                .any(|key| !key.matches_owner(path[0].owner, path[0].generation))
            || path.iter().copied().collect::<HashSet<_>>().len() != path.len()
        {
            return Err(RuntimeError::TypeError(
                "Invalid file event ancestry".into(),
            ));
        }
        if self.active_host_click_event.is_some() {
            return Err(RuntimeError::TypeError(
                "An event is already dispatching".into(),
            ));
        }
        let target = self.mint_host_wrapper(family.index as usize, path[0])?;
        self.remaining_instructions = self.config.instruction_budget;
        let prototype = self.object_prototype;
        let event = self.with_roots(|heap| heap.alloc_host_selection_event(prototype))?;
        let event_root = self.heap.root(event)?;
        let result = (|| {
            for (name, value) in [
                ("type", Value::String(kind.as_str().into())),
                ("target", Value::Object(target)),
                ("bubbles", Value::Bool(true)),
                ("cancelable", Value::Bool(false)),
                (
                    "composed",
                    Value::Bool(kind == HostFileSelectionEvent::Input),
                ),
                ("defaultPrevented", Value::Bool(false)),
                ("isTrusted", Value::Bool(true)),
            ] {
                self.define_data(event, name, value, false, true, false)?;
            }
            let function = self.function_prototype()?;
            self.install_native_getter(
                event,
                function,
                "currentTarget",
                NativeFunction::HostSelectionCurrentTarget,
            )?;
            self.install_native(
                event,
                function,
                "preventDefault",
                0,
                NativeFunction::HostSelectionPreventDefault,
            )?;
            self.install_native(
                event,
                function,
                "stopPropagation",
                0,
                NativeFunction::HostSelectionStopPropagation(false),
            )?;
            self.install_native(
                event,
                function,
                "stopImmediatePropagation",
                0,
                NativeFunction::HostSelectionStopPropagation(true),
            )?;
            self.active_host_click_event = Some(ActiveHostClickEvent {
                object: event,
                default_prevented: false,
                propagation_stopped: false,
                immediate_stopped: false,
            });
            for key in path {
                let callbacks = self
                    .host_click_listeners
                    .iter()
                    .filter(|listener| {
                        listener.family_index == family.index
                            && listener.key == *key
                            && listener.selection == Some(kind)
                    })
                    .map(|listener| listener.callback)
                    .collect::<Vec<_>>();
                if callbacks.is_empty() {
                    continue;
                }
                let current = self.mint_host_wrapper(family.index as usize, *key)?;
                self.heap
                    .set_host_selection_current_target(event, Some(current))?;
                let mut roots = Vec::with_capacity(callbacks.len());
                for callback in &callbacks {
                    match self.heap.root(*callback) {
                        Ok(root) => roots.push(root),
                        Err(error) => {
                            for root in roots {
                                let _ = self.heap.unroot(root);
                            }
                            return Err(error.into());
                        }
                    }
                }
                let outcome = (|| {
                    for callback in callbacks {
                        if !self.host_click_listeners.iter().any(|listener| {
                            listener.family_index == family.index
                                && listener.key == *key
                                && listener.selection == Some(kind)
                                && listener.callback == callback
                        }) {
                            continue;
                        }
                        if let Err(error) = self.call_native(
                            Value::Object(callback),
                            Value::Object(current),
                            vec![Value::Object(event)],
                            false,
                        ) {
                            if !matches!(
                                error,
                                RuntimeError::Thrown(_)
                                    | RuntimeError::ReferenceError(_)
                                    | RuntimeError::TypeError(_)
                                    | RuntimeError::RangeError(_)
                                    | RuntimeError::SyntaxError(_)
                            ) {
                                return Err(error);
                            }
                        }
                        if self
                            .active_host_click_event
                            .as_ref()
                            .is_some_and(|event| event.immediate_stopped)
                        {
                            break;
                        }
                    }
                    Ok(())
                })();
                for root in roots {
                    self.heap.unroot(root)?;
                }
                outcome?;
                if self
                    .active_host_click_event
                    .as_ref()
                    .is_some_and(|event| event.propagation_stopped)
                {
                    break;
                }
            }
            Ok(())
        })();
        self.active_host_click_event = None;
        self.heap.set_host_selection_current_target(event, None)?;
        self.heap.unroot(event_root)?;
        result
    }

    pub(in super::super) fn host_selection_current_target(
        &self,
        receiver: &Value,
    ) -> Result<Value, RuntimeError> {
        let object = receiver
            .object_id()
            .ok_or_else(|| RuntimeError::TypeError("Invalid selection event receiver".into()))?;
        self.heap
            .host_selection_current_target(object)
            .map(|target| target.map_or(Value::Null, Value::Object))
            .map_err(|_| RuntimeError::TypeError("Invalid selection event receiver".into()))
    }
    pub(in super::super) fn host_selection_prevent_default(
        &self,
        receiver: &Value,
    ) -> Result<Value, RuntimeError> {
        self.host_selection_current_target(receiver)?;
        Ok(Value::Undefined)
    }
    pub(in super::super) fn host_selection_stop_propagation(
        &mut self,
        receiver: &Value,
        immediate: bool,
    ) -> Result<Value, RuntimeError> {
        self.host_selection_current_target(receiver)?;
        if let Some(active) = self
            .active_host_click_event
            .as_mut()
            .filter(|active| Some(active.object) == receiver.object_id())
        {
            active.propagation_stopped = true;
            active.immediate_stopped |= immediate;
        }
        Ok(Value::Undefined)
    }
}
