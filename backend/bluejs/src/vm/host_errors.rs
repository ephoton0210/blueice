// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

//! Owner-selected HTML exception reporting. Live error values stay in the
//! realm; only bounded previews enter the existing console transport.
//! https://html.spec.whatwg.org/multipage/webappapis.html#report-an-exception

use super::*;
use crate::heap::WebListener;
use crate::native::{HostErrorNative, TargetNative};
use host_console::formatting::Output;

impl Vm {
    pub(super) fn report_pending_host_module_errors(&mut self) -> Result<(), RuntimeError> {
        if self.host_error_reporting.is_none() {
            return Ok(());
        }
        // Existing module records own these error values and their GC roots.
        // Clear the delivery flag before entering user code.
        let mut errors = Vec::new();
        if let Some(graph) = &mut self.module_graph {
            for record in graph.linked.values_mut() {
                if record.host_error_pending && record.evaluated {
                    record.host_error_pending = false;
                    if let Some(error) = &record.error {
                        errors.push(error.clone());
                    }
                }
            }
        }
        for error in errors {
            self.report_host_error_value(error.value, error.origin.source.clone())?;
        }
        Ok(())
    }

    pub fn install_host_error_reporting(&mut self) -> Result<(), RuntimeError> {
        let global = self.global("globalThis")?.object_id().unwrap();
        if self.host_error_reporting.is_some()
            || self.host_console.is_none()
            || !self.globals.contains_key("%WebEventTarget%")
        {
            return Err(RuntimeError::TypeError("error reporting requires the owner's Event and console bindings and may be installed only once".into()));
        }
        for name in [
            "ErrorEvent",
            "reportError",
            "onerror",
            "addEventListener",
            "removeEventListener",
            "dispatchEvent",
        ] {
            if self.heap.get_own(global, name)?.is_some() {
                return Err(RuntimeError::TypeError(
                    "global error binding already defined".into(),
                ));
            }
        }
        self.install_web_error_events()?;
        self.heap.initialize_global_event_target(global)?;
        let prototype = self.globals["%WebEventTarget%"];
        self.heap.set_prototype(global, Some(prototype))?;
        let function = self.function_prototype()?;
        for (name, length, native) in [
            ("reportError", 1, HostErrorNative::Report),
            (
                "addEventListener",
                2,
                HostErrorNative::Event(TargetNative::Add),
            ),
            (
                "removeEventListener",
                2,
                HostErrorNative::Event(TargetNative::Remove),
            ),
            (
                "dispatchEvent",
                1,
                HostErrorNative::Event(TargetNative::Dispatch),
            ),
        ] {
            self.install_native(
                global,
                function,
                name,
                length,
                NativeFunction::HostError(native),
            )?;
            self.heap.define_own_property(
                global,
                name,
                PropertyDescriptor {
                    enumerable: Some(true),
                    ..Default::default()
                },
            )?;
        }
        self.install_native_accessor(
            global,
            function,
            "onerror",
            NativeFunction::HostError(HostErrorNative::Handler(false)),
            NativeFunction::HostError(HostErrorNative::Handler(true)),
        )?;
        self.heap.define_own_property(
            global,
            "onerror",
            PropertyDescriptor {
                enumerable: Some(true),
                ..Default::default()
            },
        )?;
        self.host_error_reporting = Some(false);
        Ok(())
    }

    pub(super) fn host_error_call(
        &mut self,
        native: HostErrorNative,
        receiver: &Value,
        args: &[Value],
        construct: bool,
    ) -> Result<Value, RuntimeError> {
        if construct {
            return Err(RuntimeError::TypeError(
                "global event functions are not constructors".into(),
            ));
        }
        let global = self.global("globalThis")?;
        let receiver = if matches!(receiver, Value::Undefined | Value::Null) {
            &global
        } else {
            receiver
        };
        if let HostErrorNative::Event(native) = native {
            return self.web_target_call(native, receiver, args, false);
        }
        if *receiver != global {
            return Err(RuntimeError::TypeError(
                "Invalid global error receiver".into(),
            ));
        }
        match native {
            HostErrorNative::Report => {
                let Some(value) = args.first() else {
                    return Err(RuntimeError::TypeError(
                        "reportError requires an argument".into(),
                    ));
                };
                let origin = self.current_diagnostic_source_location();
                self.report_host_error_value(value.clone(), origin)?;
                Ok(Value::Undefined)
            }
            HostErrorNative::Handler(set) => {
                self.host_event_handler(global.object_id().unwrap(), "error", set, args)
            }
            HostErrorNative::Event(_) => unreachable!(),
        }
    }

    pub(super) fn host_event_handler(
        &mut self,
        global: ObjectId,
        name: &'static str,
        set: bool,
        args: &[Value],
    ) -> Result<Value, RuntimeError> {
        let mut state = self.heap.web_event_target(global)?.clone();
        let old = state
            .listeners
            .iter()
            .position(|listener| listener.handler && listener.name == name);
        if !set {
            return Ok(old.map_or(Value::Null, |index| {
                Value::Object(state.listeners[index].callback)
            }));
        }
        match (old, native::argument(args, 0).object_id()) {
            (Some(index), Some(callback)) => state.listeners[index].callback = callback,
            (Some(index), None) => {
                state.listeners.remove(index);
            }
            (None, Some(callback)) => {
                if state.listeners.len() >= 4096 {
                    return Err(RuntimeError::RangeError(
                        "Event listener limit exceeded".into(),
                    ));
                }
                state.next_serial = state.next_serial.checked_add(1).ok_or_else(|| {
                    RuntimeError::RangeError("Event listener identity exhausted".into())
                })?;
                state.listeners.push(WebListener {
                    serial: state.next_serial,
                    name: name.into(),
                    callback,
                    capture: false,
                    once: false,
                    passive: false,
                    handler: true,
                    signal: None,
                });
            }
            _ => {}
        }
        self.with_roots(|heap| heap.set_web_event_target(global, state))?;
        Ok(Value::Undefined)
    }

    pub(super) fn report_host_exception(
        &mut self,
        error: &RuntimeError,
    ) -> Result<(), RuntimeError> {
        self.report_host_exception_with_origin(error, self.pending_diagnostic_origin.clone())
    }

    pub(super) fn report_host_exception_with_origin(
        &mut self,
        error: &RuntimeError,
        origin: Option<crate::diagnostics::DiagnosticSourceLocation>,
    ) -> Result<(), RuntimeError> {
        if self.host_error_reporting.is_none() || !host_timers::is_page_exception(error) {
            return Ok(());
        }
        let value = self.error_value(error.clone())?;
        self.report_host_error_value(value, origin)
    }

    pub(super) fn current_diagnostic_source_location(
        &self,
    ) -> Option<crate::diagnostics::DiagnosticSourceLocation> {
        self.active_diagnostic_frame.as_ref()?.location().cloned()
    }

    fn report_host_error_value(
        &mut self,
        value: Value,
        origin: Option<crate::diagnostics::DiagnosticSourceLocation>,
    ) -> Result<(), RuntimeError> {
        let base = self.stack.len();
        let throw_site = self.pending_throw_site;
        let uncaught_site = self.uncaught_throw_site;
        let pending_origin = self.pending_diagnostic_origin.clone();
        let uncaught_origin = self.uncaught_diagnostic_origin.clone();
        self.stack.push(value.clone());
        let result = (|| {
            let mut message = Output::new();
            self.console_preview(&value, &mut message)?;
            let mut unhandled = true;
            if self.host_error_reporting == Some(false) {
                let mut state = self.new_web_event_state(6, "error".into());
                state.cancelable = true;
                state.set("message", Value::String(message.text.clone().into()));
                let (filename, line, column) = origin
                    .as_ref()
                    .map(|source| (source.filename.as_str(), source.line, source.column))
                    .unwrap_or(("", 0, 0));
                state.set("filename", Value::String(filename.into()));
                state.set("lineno", Value::Number(f64::from(line)));
                state.set("colno", Value::Number(f64::from(column)));
                state.set("error", value);
                let prototype = self.globals["%WebErrorEvent%"];
                let event = self.with_roots(|heap| heap.alloc_drag_event(state, prototype))?;
                self.stack.push(Value::Object(event));
                self.finish_web_event(event)?;
                let global = self.global("globalThis")?.object_id().unwrap();
                self.host_error_reporting = Some(true);
                let fired = self.dispatch_web_target_event(global, event, true);
                self.host_error_reporting = Some(false);
                unhandled = fired? == Value::Bool(true);
            }
            if unhandled {
                let mut output = Output::message("Uncaught ");
                output.push(&message.text);
                output.truncated |= message.truncated;
                self.console_emit_with_source(ConsoleFunction::Error, "error", output, origin)?;
            }
            Ok(())
        })();
        self.stack.truncate(base);
        self.pending_throw_site = throw_site;
        self.uncaught_throw_site = uncaught_site;
        self.pending_diagnostic_origin = pending_origin;
        self.uncaught_diagnostic_origin = uncaught_origin;
        result
    }
}
