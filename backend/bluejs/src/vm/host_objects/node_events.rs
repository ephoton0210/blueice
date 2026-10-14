// This Source Code Form is subject to the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

//! Script dispatch uses a freshly owner-validated ancestry snapshot. It
//! never invokes a native default action or creates a native input grant.

use super::*;

impl Vm {
    /// Enables arbitrary named listeners and script dispatch for one private
    /// node family. The owner supplies target-first live document ancestry.
    /// Opaque keys and callback objects never cross into page-visible data.
    pub fn install_host_node_event_methods(
        &mut self,
        family: HostObjectFamily,
        path: impl FnMut(HostObjectKey) -> Result<Vec<HostObjectKey>, HostFunctionError> + 'static,
    ) -> Result<(), RuntimeError> {
        let prototype = self.host_family_prototype(family)?;
        if self.host_object_families[family.index as usize]
            .event_path
            .is_some()
        {
            return Err(RuntimeError::TypeError(
                "Node event methods already installed".into(),
            ));
        }
        self.install_web_events()?;
        self.install_web_event_target()?;
        if !self.host_object_families[family.index as usize].event_target {
            self.install_host_click_event_methods(family)?;
        }
        self.install_host_callable_native(
            prototype,
            "dispatchEvent",
            1,
            NativeFunction::HostNodeDispatch(family.index),
        )?;
        self.heap
            .set_prototype(prototype, Some(self.globals["%WebEventTarget%"]))?;
        self.host_object_families[family.index as usize].event_path = Some(Box::new(path));
        Ok(())
    }

    pub(in super::super) fn host_node_event_family(&self, receiver: &Value) -> Option<u32> {
        let object = receiver.object_id()?;
        self.host_object_families
            .iter()
            .enumerate()
            .find_map(|(index, family)| {
                (family.event_path.is_some() && family.keys_by_wrapper.contains_key(&object))
                    .then_some(index as u32)
            })
    }

    pub(super) fn root_host_event_type(&mut self, name: JsString) -> Result<RootId, RuntimeError> {
        let base = self.stack.len();
        let result = (|| {
            let prototype = self.object_prototype;
            let metadata = self.with_roots(|heap| heap.alloc_object(Some(prototype)))?;
            self.stack.push(Value::Object(metadata));
            self.define_data(metadata, "type", Value::String(name), false, false, false)?;
            Ok(self.heap.root(metadata)?)
        })();
        self.stack.truncate(base);
        result
    }

    pub(in super::super) fn host_node_dispatch_call(
        &mut self,
        family_index: u32,
        receiver: &Value,
        args: &[Value],
        construct: bool,
    ) -> Result<Value, RuntimeError> {
        if construct || args.is_empty() {
            return Err(RuntimeError::TypeError(
                "dispatchEvent requires an Event".into(),
            ));
        }
        let target = receiver
            .object_id()
            .ok_or_else(|| RuntimeError::TypeError("Invalid node receiver".into()))?;
        let key = self
            .host_object_families
            .get(family_index as usize)
            .and_then(|family| family.keys_by_wrapper.get(&target).copied())
            .ok_or_else(|| RuntimeError::TypeError("Invalid node receiver".into()))?;
        let event = args[0]
            .object_id()
            .filter(|event| self.heap.drag_event(*event).is_ok())
            .ok_or_else(|| RuntimeError::TypeError("dispatchEvent requires an Event".into()))?;
        if self.heap.drag_event(event)?.phase != 0 {
            return self.drag_named_error("InvalidStateError", "Event is already dispatching");
        }
        let path = self.host_node_event_path(family_index, key)?;
        self.dispatch_node_event_path(family_index, target, event, &path, false)
    }

    pub(super) fn host_node_event_path(
        &mut self,
        family_index: u32,
        key: HostObjectKey,
    ) -> Result<Vec<HostObjectKey>, RuntimeError> {
        let path = self.host_object_families[family_index as usize]
            .event_path
            .as_mut()
            .ok_or_else(|| RuntimeError::TypeError("Node event path unavailable".into()))?(
            key
        )
        .map_err(|error| RuntimeError::TypeError(error.0))?;
        if path.first() != Some(&key)
            || path.len() > 256
            || path
                .iter()
                .any(|ancestor| !ancestor.matches_owner(key.owner, key.generation))
            || path.iter().copied().collect::<HashSet<_>>().len() != path.len()
        {
            return Err(RuntimeError::TypeError(
                "Invalid node event ancestry".into(),
            ));
        }
        Ok(path)
    }

    pub(super) fn dispatch_native_node_event(
        &mut self,
        family_index: u32,
        path: &[HostObjectKey],
        name: &str,
        cancelable: bool,
        composed: bool,
    ) -> Result<Value, RuntimeError> {
        let base = self.stack.len();
        let result = (|| {
            let target = self.mint_host_wrapper(family_index as usize, path[0])?;
            let mut state = self.new_web_event_state(0, name.into());
            state.cancelable = cancelable;
            state.set("bubbles", Value::Bool(true));
            state.set("composed", Value::Bool(composed));
            let prototype = self.globals["%WebEvent%"];
            let event = self.with_roots(|heap| heap.alloc_drag_event(state, prototype))?;
            self.stack.push(Value::Object(event));
            self.finish_web_event(event)?;
            self.dispatch_node_event_path(family_index, target, event, path, true)
        })();
        self.stack.truncate(base);
        result
    }

    fn dispatch_node_event_path(
        &mut self,
        family_index: u32,
        target: ObjectId,
        event: ObjectId,
        path: &[HostObjectKey],
        trusted: bool,
    ) -> Result<Value, RuntimeError> {
        let base = self.stack.len();
        self.stack
            .extend([Value::Object(target), Value::Object(event)]);
        let result = (|| {
            let wrappers = path
                .iter()
                .map(|key| self.mint_host_wrapper(family_index as usize, *key))
                .collect::<Result<Vec<_>, _>>()?;
            let mut state = self.heap.drag_event(event)?.clone();
            let name = match state.get("type") {
                Value::String(name) => name,
                _ => unreachable!("branded event type"),
            };
            let bubbles = state.get("bubbles") == Value::Bool(true);
            state.set("target", Value::Object(target));
            state.set("isTrusted", Value::Bool(trusted));
            state.path = wrappers.clone();
            self.with_roots(|heap| heap.set_drag_event_state(event, state))?;
            for (index, capture) in listener_phases(path.len()) {
                if self.heap.drag_event(event)?.propagation_stopped {
                    break;
                }
                if !capture && index > 0 && !bubbles {
                    continue;
                }
                let current = wrappers[index];
                let mut state = self.heap.drag_event(event)?.clone();
                state.phase = if index == 0 {
                    2
                } else if capture {
                    1
                } else {
                    3
                };
                state.current_target = Some(current);
                self.with_roots(|heap| heap.set_drag_event_state(event, state))?;
                let snapshot: Vec<_> = self
                    .host_click_listeners
                    .iter()
                    .filter(|listener| {
                        listener.family_index == family_index
                            && listener.key == path[index]
                            && listener.event_type == name
                            && listener.capture == capture
                    })
                    .map(|listener| listener.serial)
                    .collect();
                for serial in snapshot {
                    if self.heap.drag_event(event)?.immediate_stopped {
                        break;
                    }
                    let listener = self
                        .host_click_listeners
                        .iter()
                        .find(|listener| listener.serial == serial)
                        .cloned();
                    let Some(listener) = listener else {
                        continue;
                    };
                    let callback_base = self.stack.len();
                    self.stack.push(Value::Object(listener.callback));
                    let called = self.invoke_host_listener(&listener, current, event);
                    self.stack.truncate(callback_base);
                    if let Err(error) = called {
                        if !super::super::web_drag_drop::drag_page_exception(&error) {
                            return Err(error);
                        }
                    }
                }
            }
            Ok(Value::Bool(!self.heap.drag_event(event)?.default_prevented))
        })();
        let cleanup = (|| {
            let mut state = self.heap.drag_event(event)?.clone();
            state.phase = 0;
            state.current_target = None;
            state.path.clear();
            state.passive = false;
            state.propagation_stopped = false;
            state.immediate_stopped = false;
            self.with_roots(|heap| heap.set_drag_event_state(event, state))
        })();
        self.stack.truncate(base);
        cleanup?;
        result
    }
}
