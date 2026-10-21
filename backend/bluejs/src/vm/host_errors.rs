// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

//! Owner-selected HTML exception reporting. Live error values stay in the
//! realm; only bounded previews enter the existing console transport.
//! https://html.spec.whatwg.org/multipage/webappapis.html#report-an-exception

use super::*;
use crate::heap::WebListener;
use crate::native::{HostErrorNative, TargetNative};
use error_input_storage::utf16_from_utf8;
use host_console::formatting::Output;
use host_report_storage::{ConsoleOwners, PreparedReportEvent, ReportOrigin};
use native_value_storage::NativeValue;

impl Vm {
    pub(super) fn report_pending_host_module_errors(&mut self) -> Result<(), RuntimeError> {
        if self.host_error_reporting.is_none() {
            return Ok(());
        }
        self.dispatch_pending_host_module_errors(|vm, error| {
            let owners = vm.console_owners(None)?;
            let value = NativeValue::copied(&error.value, owners.owner.as_ref())?;
            let origin = ReportOrigin::copied(error.origin.source.as_ref(), owners.owner.as_ref())?;
            vm.report_host_error_prepared(value, origin, owners)
        })
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
                let owners = self.console_owners(None)?;
                let value = NativeValue::copied(value, owners.owner.as_ref())?;
                let origin = ReportOrigin::copied(
                    self.active_diagnostic_frame
                        .as_ref()
                        .and_then(|frame| frame.location()),
                    owners.owner.as_ref(),
                )?;
                self.report_host_error_prepared(value, origin, owners)?;
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
        if self.host_error_reporting.is_none() || !host_timers::is_page_exception(error) {
            return Ok(());
        }
        let owners = self.console_owners(None)?;
        let origin = ReportOrigin::copied(
            self.pending_diagnostic_origin.as_ref(),
            owners.owner.as_ref(),
        )?;
        let value = self.copy_error_value(error, owners.owner.as_ref())?;
        self.report_host_error_prepared(value, origin, owners)
    }

    pub(super) fn report_host_exception_with_origin(
        &mut self,
        error: &RuntimeError,
        origin: Option<crate::diagnostics::DiagnosticSourceLocation>,
    ) -> Result<(), RuntimeError> {
        if self.host_error_reporting.is_none() || !host_timers::is_page_exception(error) {
            return Ok(());
        }
        let owners = self.console_owners(None)?;
        let origin = ReportOrigin::adopt(origin, owners.owner.as_ref())?;
        let value = self.copy_error_value(error, owners.owner.as_ref())?;
        self.report_host_error_prepared(value, origin, owners)
    }

    pub(super) fn current_diagnostic_source_location(
        &self,
    ) -> Option<crate::diagnostics::DiagnosticSourceLocation> {
        self.active_diagnostic_frame.as_ref()?.location().cloned()
    }

    fn report_host_error_prepared(
        &mut self,
        value: NativeValue,
        mut origin: ReportOrigin,
        owners: ConsoleOwners,
    ) -> Result<(), RuntimeError> {
        let owners = owners.with_sources(value.owner_view()?, origin.owner_view()?)?;
        let value = value.retain_for_owner(owners.owner.as_ref())?;
        origin.retain_for_owner(owners.owner.as_ref())?;
        let dispatch = self.host_error_reporting == Some(false);
        let roots = realm_call_storage::RealmCallRoots::new(
            usize::from(value.value().object_id().is_some()) + usize::from(dispatch),
            owners.owner.as_ref(),
        )?;
        if let Some(object) = value.value().object_id() {
            roots.push(&self.heap, object)?;
        }
        let frame = roots.clone();
        let result = self.with_realm_call_roots(frame, |vm| {
            vm.report_host_error_rooted(value, origin, dispatch, &roots, &owners)
        });
        owners.validate().and(result)
    }

    fn report_host_error_rooted(
        &mut self,
        value: NativeValue,
        origin: ReportOrigin,
        dispatch: bool,
        roots: &realm_call_storage::RealmCallRoots,
        owners: &ConsoleOwners,
    ) -> Result<(), RuntimeError> {
        let base = self.stack.len();
        let throw_site = self.pending_throw_site;
        let uncaught_site = self.uncaught_throw_site;
        let pending_origin = ReportOrigin::copied(
            self.pending_diagnostic_origin.as_ref(),
            owners.owner.as_ref(),
        )?;
        let uncaught_origin = ReportOrigin::copied(
            self.uncaught_diagnostic_origin.as_ref(),
            owners.owner.as_ref(),
        )?;
        let result = (|| {
            let mut message = Output::owned(owners.owner.as_ref())?;
            self.console_preview(value.value(), &mut message)?;
            message.validate()?;
            let mut unhandled = true;
            if dispatch {
                let mut state = PreparedReportEvent::new(self, owners.owner.as_ref())?;
                state.set(
                    "message",
                    utf16_from_utf8(&message.text, owners.owner.as_ref())?,
                );
                let (filename, line, column) = origin
                    .value
                    .as_ref()
                    .map(|source| (source.filename.as_str(), source.line, source.column))
                    .unwrap_or(("", 0, 0));
                state.set(
                    "filename",
                    utf16_from_utf8(filename, owners.owner.as_ref())?,
                );
                state.number("lineno", line);
                state.number("colno", column);
                state.set("error", value);
                let prototype = self.globals["%WebErrorEvent%"];
                let event = state.into_heap(self, prototype)?;
                roots.push(&self.heap, event)?;
                self.finish_web_event(event)?;
                let global = self.global("globalThis")?.object_id().unwrap();
                self.host_error_reporting = Some(true);
                let fired = self.dispatch_web_target_event(global, event, true);
                self.host_error_reporting = Some(false);
                owners.validate()?;
                unhandled = fired? == Value::Bool(true);
            }
            if unhandled {
                let mut output = Output::owned(owners.owner.as_ref())?;
                output.push("Uncaught ");
                output.push(&message.text);
                output.truncated |= message.truncated;
                self.console_emit_guarded(ConsoleFunction::Error, "error", output, origin, owners)?;
            }
            Ok(())
        })();
        self.stack.truncate(base);
        self.pending_throw_site = throw_site;
        self.uncaught_throw_site = uncaught_site;
        pending_origin.restore(&mut self.pending_diagnostic_origin);
        uncaught_origin.restore(&mut self.uncaught_diagnostic_origin);
        let reconciled = self.synchronize_scheduling_usage();
        owners.validate().and(reconciled).and(result)
    }
}
