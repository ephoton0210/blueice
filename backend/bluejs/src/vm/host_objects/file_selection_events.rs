// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

use super::node_events::{valid_host_event_path, HostEventContext};
use super::*;
use crate::vm::error_input_storage::utf16_from_utf8;
use crate::vm::realm_call_storage::RealmCallRoots;

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
    /// Keeps callbacks in the VM and enables the browser's click, file
    /// selection, and drag event vocabulary on this exact wrapper family.
    pub fn install_host_file_selection_event_methods(
        &mut self,
        family: HostObjectFamily,
    ) -> Result<(), RuntimeError> {
        self.install_host_click_event_methods(family)?;
        self.host_object_families[family.index as usize].file_events = true;
        self.host_object_families[family.index as usize].drag_events = true;
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
        let context = HostEventContext::capture(self)?;
        if !self.host_object_families[family.index as usize].file_events
            || !valid_host_event_path(path)
        {
            return Err(RuntimeError::TypeError(
                "Invalid file event ancestry".into(),
            ));
        }
        if self.active_host_click_event.is_some() || self.active_host_drag_event.is_some() {
            return Err(RuntimeError::TypeError(
                "An event is already dispatching".into(),
            ));
        }
        if self.host_object_families[family.index as usize]
            .event_path
            .is_some()
        {
            context.validate(self)?;
            self.remaining_instructions = self.config.instruction_budget;
            self.dispatch_native_node_event(
                family.index,
                path,
                kind.as_str(),
                false,
                kind == HostFileSelectionEvent::Input,
            )?;
            return Ok(());
        }
        context.with_roots(self, std::iter::empty(), 3, |vm, roots| {
            vm.dispatch_legacy_file_event(family, path, kind, &context, roots)
        })
    }

    fn dispatch_legacy_file_event(
        &mut self,
        family: HostObjectFamily,
        path: &[HostObjectKey],
        kind: HostFileSelectionEvent,
        context: &HostEventContext,
        roots: &RealmCallRoots,
    ) -> Result<(), RuntimeError> {
        let target = self.mint_host_wrapper(family.index as usize, path[0]);
        let target = context.validate(self).and(target)?;
        roots.push(&self.heap, target)?;
        self.remaining_instructions = self.config.instruction_budget;
        let prototype = self.object_prototype;
        let event = self.with_roots(|heap| heap.alloc_host_selection_event(prototype))?;
        roots.push(&self.heap, event)?;
        let result = (|| {
            let name = utf16_from_utf8(kind.as_str(), context.event.owner.as_ref())?;
            let (name, storage) = name.into_retained_parts();
            property_definition_storage::PropertyDefinitionInput::from_retained(
                "type".into(),
                PropertyDescriptor::data(name, false, true, false),
                storage,
            )
            .define(self, event)?;
            for (name, value) in [
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
                passive: false,
            });
            for (index, capture) in super::listener_phases(path.len()) {
                context.validate(self)?;
                let key = &path[index];
                let callbacks = self.listener_snapshot(
                    |listener| {
                        listener.family_index == family.index
                            && listener.key == *key
                            && listener.selection == Some(kind)
                            && listener.capture == capture
                    },
                    false,
                )?;
                let current = self.mint_host_wrapper(family.index as usize, *key);
                let current = context.validate(self).and(current)?;
                roots.push(&self.heap, current)?;
                self.heap
                    .set_host_selection_current_target(event, Some(current))?;
                let outcome = (|| {
                    for serial in callbacks.serials.iter().copied() {
                        context.validate(self)?;
                        callbacks.validate(self)?;
                        if self
                            .active_host_click_event
                            .as_ref()
                            .is_some_and(|event| event.immediate_stopped)
                        {
                            break;
                        }
                        let called = self.invoke_host_listener(serial, current, event);
                        context.validate(self)?;
                        callbacks.validate(self)?;
                        if let Err(error) = called {
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
                roots.truncate(2);
                outcome?;
                if self
                    .active_host_click_event
                    .as_ref()
                    .is_some_and(|event| event.propagation_stopped)
                    && !(index == 0 && capture)
                {
                    break;
                }
            }
            Ok(())
        })();
        self.active_host_click_event = None;
        let cleanup = self
            .heap
            .set_host_selection_current_target(event, None)
            .map_err(RuntimeError::from);
        context.validate(self).and(cleanup).and(result)
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
