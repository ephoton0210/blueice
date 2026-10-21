// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

//! Bounded, owner-installed page diagnostics. No live JS value is retained.
//! https://console.spec.whatwg.org/

use super::*;
use std::collections::VecDeque;
pub(super) mod formatting;
use super::host_report_storage::{ConsoleOwners, ReportOrigin};
use super::host_scheduling::SharedSchedulingCharge;
use super::native_value_storage::NativeValue;
use super::web_event_storage::EventContext;
use formatting::Output;
mod storage;
use storage::{ConsoleLabel, ConsoleLabels};

#[cfg(any(test, coverage))]
#[path = "../../tests/fixtures/host_report_storage.rs"]
mod report_storage_tests;

#[cfg(any(test, coverage))]
#[path = "../../tests/fixtures/console_runtime_storage.rs"]
mod runtime_storage_tests;

#[cfg(any(test, coverage))]
#[path = "../../tests/fixtures/console_text_storage.rs"]
mod text_storage_tests;

pub const MAX_HOST_CONSOLE_RECORDS: usize = 256;
pub const MAX_HOST_CONSOLE_BYTES: usize = 256 * 1024;
pub const MAX_HOST_CONSOLE_MESSAGE_BYTES: usize = 8 * 1024;
pub const MAX_HOST_CONSOLE_LABELS: usize = 128;
pub const MAX_HOST_CONSOLE_LABEL_BYTES: usize = 256;
pub const MAX_HOST_CONSOLE_GROUP_DEPTH: u32 = 32;

#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize)]
pub struct HostConsoleRecord {
    pub sequence: u64,
    pub timestamp_ms: u64,
    pub method: &'static str,
    pub level: &'static str,
    pub group_depth: u32,
    pub message: String,
    pub truncated: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub source: Option<crate::diagnostics::DiagnosticSourceLocation>,
}

impl HostConsoleRecord {
    fn owned_text_bytes(&self) -> usize {
        self.message.capacity()
            + self
                .source
                .as_ref()
                .map_or(0, |source| source.filename.capacity())
    }
}

#[derive(Debug, Clone, Default, PartialEq, Eq, serde::Serialize)]
pub struct HostConsoleBatch {
    pub records: Vec<HostConsoleRecord>,
    /// Evicted records since the previous drain/clear; saturated on overflow.
    pub dropped_records: u64,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum ConsoleFunction {
    Assert,
    Clear,
    Debug,
    Error,
    Info,
    Log,
    Table,
    Trace,
    Warn,
    Dir,
    Dirxml,
    Count,
    CountReset,
    Group,
    GroupCollapsed,
    GroupEnd,
    Time,
    TimeLog,
    TimeEnd,
}

impl ConsoleFunction {
    const ALL: [(Self, &'static str); 19] = [
        (Self::Assert, "assert"),
        (Self::Clear, "clear"),
        (Self::Debug, "debug"),
        (Self::Error, "error"),
        (Self::Info, "info"),
        (Self::Log, "log"),
        (Self::Table, "table"),
        (Self::Trace, "trace"),
        (Self::Warn, "warn"),
        (Self::Dir, "dir"),
        (Self::Dirxml, "dirxml"),
        (Self::Count, "count"),
        (Self::CountReset, "countReset"),
        (Self::Group, "group"),
        (Self::GroupCollapsed, "groupCollapsed"),
        (Self::GroupEnd, "groupEnd"),
        (Self::Time, "time"),
        (Self::TimeLog, "timeLog"),
        (Self::TimeEnd, "timeEnd"),
    ];
    fn name(self) -> &'static str {
        Self::ALL.iter().find(|(f, _)| *f == self).unwrap().1
    }
    fn level(self) -> &'static str {
        match self {
            Self::Debug => "debug",
            Self::Info | Self::Count | Self::TimeEnd => "info",
            Self::Warn | Self::CountReset => "warn",
            Self::Error | Self::Assert => "error",
            _ => "log",
        }
    }
}

pub(super) struct ConsoleState {
    records: VecDeque<HostConsoleRecord>,
    bytes: usize,
    dropped: u64,
    sequence: u64,
    depth: u64,
    counts: ConsoleLabels,
    timers: ConsoleLabels,
    // Records and their backing outlive the VM when the state is detached.
    record_storage: Option<SharedSchedulingCharge>,
}

struct NativeConsoleRecord {
    record: HostConsoleRecord,
    message_storage: Option<SharedSchedulingCharge>,
    source_storage: Option<SharedSchedulingCharge>,
}

impl ConsoleState {
    fn new(owner: Option<&SharedSchedulingCharge>) -> Result<Self, RuntimeError> {
        let mut record_storage = owner
            .map(|owner| {
                owner.reserve_peer(
                    MAX_HOST_CONSOLE_RECORDS * std::mem::size_of::<HostConsoleRecord>(),
                )
            })
            .transpose()?;
        let mut records = VecDeque::new();
        records
            .try_reserve_exact(MAX_HOST_CONSOLE_RECORDS)
            .map_err(|_| {
                owner.map_or(RuntimeError::SchedulingLimit, SharedSchedulingCharge::fail)
            })?;
        if let Some(storage) = &mut record_storage {
            storage.resize(records.capacity() * std::mem::size_of::<HostConsoleRecord>())?;
        }
        Ok(Self {
            records,
            bytes: 0,
            dropped: 0,
            sequence: 0,
            depth: 0,
            counts: ConsoleLabels::new(owner)?,
            timers: ConsoleLabels::new(owner)?,
            record_storage,
        })
    }
    fn record_bytes(&self) -> usize {
        self.records.capacity() * std::mem::size_of::<HostConsoleRecord>() + self.bytes
    }

    pub(super) fn can_join(&self, scope: &SchedulingScope) -> bool {
        self.record_storage
            .as_ref()
            .is_none_or(|owner| owner.belongs_to(scope))
            && self.counts.can_join(scope)
            && self.timers.can_join(scope)
    }

    fn owner_view(&self) -> Result<Option<SharedSchedulingCharge>, RuntimeError> {
        let mut result = self
            .record_storage
            .as_ref()
            .map(|owner| owner.reserve_peer(0))
            .transpose()?;
        for owner in [self.counts.owner_view()?, self.timers.owner_view()?]
            .into_iter()
            .flatten()
        {
            if let Some(current) = &result {
                if !current.same_owner(&owner) {
                    return Err(current.fail());
                }
            } else {
                result = Some(owner);
            }
        }
        Ok(result)
    }

    pub(super) fn unowned_payload_bytes(&self) -> usize {
        (if self.record_storage.is_none() {
            self.record_bytes()
        } else {
            0
        }) + self.counts.unowned_bytes()
            + self.timers.unowned_bytes()
    }

    #[cfg(any(test, coverage))]
    pub(super) fn payload_bytes(&self) -> usize {
        self.records.capacity() * std::mem::size_of::<HostConsoleRecord>()
            + self.bytes
            + self.counts.owned_bytes()
            + self.timers.owned_bytes()
    }
    fn clear_records(&mut self) {
        self.records.clear();
        self.bytes = 0;
        let backing = self.record_bytes();
        if let Some(storage) = &mut self.record_storage {
            storage.release_to(backing);
        }
    }

    fn push_prepared(&mut self, mut incoming: NativeConsoleRecord) {
        while self.records.len() >= MAX_HOST_CONSOLE_RECORDS
            || self.bytes + incoming.record.owned_text_bytes() > MAX_HOST_CONSOLE_BYTES
        {
            let old = self
                .records
                .pop_front()
                .expect("one bounded message fits the empty console");
            self.bytes -= old.owned_text_bytes();
            drop(old);
            let retained = self.record_bytes();
            if let Some(storage) = &mut self.record_storage {
                storage.release_to(retained);
            }
            self.dropped = self.dropped.saturating_add(1);
        }
        for storage in [
            incoming.message_storage.take(),
            incoming.source_storage.take(),
        ]
        .into_iter()
        .flatten()
        {
            self.record_storage
                .as_mut()
                .expect("prepared record has an owner")
                .absorb(storage)
                .expect("prepared record uses the console account");
        }
        incoming.record.sequence = self.sequence.saturating_add(1);
        incoming.record.group_depth =
            self.depth.min(u64::from(MAX_HOST_CONSOLE_GROUP_DEPTH)) as u32;
        incoming.record.truncated |= self.depth > u64::from(MAX_HOST_CONSOLE_GROUP_DEPTH);
        self.sequence = incoming.record.sequence;
        self.bytes += incoming.record.owned_text_bytes();
        assert!(self.records.len() < self.records.capacity());
        self.records.push_back(incoming.record);
    }
}

impl Vm {
    pub(super) fn console_owners(
        &mut self,
        hint: Option<&SharedSchedulingCharge>,
    ) -> Result<ConsoleOwners, RuntimeError> {
        let context = EventContext::capture(self)?;
        let caller = self.argument_list_owner()?;
        let original = self
            .host_console
            .as_ref()
            .map(ConsoleState::owner_view)
            .transpose()?
            .flatten();
        let owner = original
            .as_ref()
            .or(hint)
            .or(context.owner.as_ref())
            .or(caller.as_ref())
            .map(|owner| owner.reserve_peer(0))
            .transpose()?;
        if let (Some(scope), Some(owner)) = (&self.host_scheduling, &owner) {
            if !owner.belongs_to(scope) {
                return Err(scope.fail());
            }
        }
        let needs_owner = self.host_console.as_ref().is_some_and(|console| {
            console.record_storage.is_none()
                || console.counts.needs_owner()
                || console.timers.needs_owner()
        });
        if needs_owner {
            if let Some(owner) = &owner {
                self.synchronize_scheduling_usage()?;
                let bytes = self.host_console.as_ref().unwrap().unowned_payload_bytes();
                let mut storage = if let Some(scope) = &mut self.host_scheduling {
                    scope.detach_owned_payload(bytes)
                } else {
                    owner.reserve_peer(bytes)?
                };
                let console = self.host_console.as_mut().unwrap();
                if console.record_storage.is_none() {
                    console.record_storage = Some(storage.split_off(console.record_bytes()));
                }
                if console.counts.needs_owner() {
                    console
                        .counts
                        .install_owner(storage.split_off(console.counts.owned_bytes()));
                }
                if console.timers.needs_owner() {
                    console
                        .timers
                        .install_owner(storage.split_off(console.timers.owned_bytes()));
                }
                assert_eq!(storage.bytes(), 0);
                self.synchronize_scheduling_usage()?;
            }
        }
        context.validate()?;
        Ok(ConsoleOwners::new(owner, caller, context))
    }

    pub fn install_host_console(&mut self) -> Result<(), RuntimeError> {
        let owners = self.console_owners(None)?;
        let global = self.global("globalThis")?.object_id().unwrap();
        if self.host_console.is_some() || self.heap.get_own(global, "console")?.is_some() {
            return Err(RuntimeError::TypeError(
                "console is already installed or defined".into(),
            ));
        }
        let state = ConsoleState::new(owners.owner.as_ref())?;
        let base = self.stack.len();
        let result = (|| {
            let object_prototype = self.object_prototype;
            let prototype = self.with_roots(|heap| heap.alloc_object(Some(object_prototype)))?;
            self.stack.push(Value::Object(prototype));
            let console = self.with_roots(|heap| heap.alloc_object(Some(prototype)))?;
            self.stack.push(Value::Object(console));
            self.define_data(
                console,
                JsSymbol::well_known("toStringTag"),
                Value::String("console".into()),
                false,
                false,
                true,
            )?;
            let prototype = self.function_prototype()?;
            for (function, name) in ConsoleFunction::ALL {
                self.install_native(
                    console,
                    prototype,
                    name,
                    0,
                    NativeFunction::Console(function),
                )?;
                self.heap.define_own_property(
                    console,
                    name,
                    PropertyDescriptor {
                        enumerable: Some(true),
                        ..Default::default()
                    },
                )?;
            }
            self.define_data(global, "console", Value::Object(console), true, false, true)?;
            self.host_console = Some(state);
            self.synchronize_scheduling_usage()
        })();
        self.stack.truncate(base);
        result
    }

    /// Transfers handle-free records without executing script, jobs or tasks.
    pub fn drain_host_console(&mut self) -> Result<HostConsoleBatch, RuntimeError> {
        let owners = self.console_owners(None)?;
        let count = self
            .host_console
            .as_ref()
            .map_or(0, |console| console.records.len());
        let mut storage = owners
            .owner
            .as_ref()
            .map(|owner| owner.reserve_peer(count * std::mem::size_of::<HostConsoleRecord>()))
            .transpose()?;
        let mut records = Vec::new();
        records.try_reserve_exact(count).map_err(|_| {
            owners
                .owner
                .as_ref()
                .map_or(RuntimeError::SchedulingLimit, SharedSchedulingCharge::fail)
        })?;
        if let Some(storage) = &mut storage {
            storage.resize(host_scheduling::slots(&records))?;
        }
        let dropped_records = match &mut self.host_console {
            None => 0,
            Some(console) => {
                if let Some(record_storage) = &mut console.record_storage {
                    storage
                        .as_mut()
                        .expect("drained records have the console account")
                        .absorb(record_storage.split_off(console.bytes))?;
                }
                records.extend(console.records.drain(..));
                console.bytes = 0;
                std::mem::take(&mut console.dropped)
            }
        };
        let result = HostConsoleBatch {
            records,
            dropped_records,
        };
        self.synchronize_scheduling_usage()?;
        owners.validate()?;
        // Pure public handoff: embedding-held batch ownership remains external.
        drop(storage);
        Ok(result)
    }

    #[cfg(any(test, coverage))]
    pub(super) fn console_emit_with_source(
        &mut self,
        function: ConsoleFunction,
        level: &'static str,
        output: Output,
        source: Option<crate::diagnostics::DiagnosticSourceLocation>,
    ) -> Result<(), RuntimeError> {
        self.console_emit_with_leased_source(function, level, output, source, None)
    }

    pub(super) fn console_emit_with_leased_source(
        &mut self,
        function: ConsoleFunction,
        level: &'static str,
        output: Output,
        source: Option<crate::diagnostics::DiagnosticSourceLocation>,
        charge: Option<host_scheduling::SharedSchedulingCharge>,
    ) -> Result<(), RuntimeError> {
        let origin = ReportOrigin::from_parts(source, charge);
        let source_owner = origin.owner_view()?;
        let output_owner = output.owner_view()?;
        let owners = self
            .console_owners(source_owner.as_ref().or(output_owner.as_ref()))?
            .with_sources(source_owner, output_owner)?;
        self.console_emit_guarded(function, level, output, origin, &owners)
    }

    pub(super) fn console_emit_guarded(
        &mut self,
        function: ConsoleFunction,
        level: &'static str,
        output: Output,
        origin: ReportOrigin,
        owners: &ConsoleOwners,
    ) -> Result<(), RuntimeError> {
        let incoming =
            self.console_prepare_guarded(function, level, output, origin, owners, None)?;
        self.console_publish_prepared(incoming, owners, |_| {}, |_| {})
    }

    fn console_prepare_guarded(
        &mut self,
        function: ConsoleFunction,
        level: &'static str,
        mut output: Output,
        mut origin: ReportOrigin,
        owners: &ConsoleOwners,
        timestamp: Option<u64>,
    ) -> Result<NativeConsoleRecord, RuntimeError> {
        owners.validate()?;
        output.retain_for_owner(owners.owner.as_ref())?;
        origin.retain_for_owner(owners.owner.as_ref())?;
        let (message, truncated, message_storage) = output.into_retained_text()?;
        let (source, source_storage) = origin.into_parts();
        let mut incoming = NativeConsoleRecord {
            record: HostConsoleRecord {
                sequence: 0,
                timestamp_ms: 0,
                method: function.name(),
                level,
                group_depth: 0,
                message,
                truncated,
                source,
            },
            message_storage,
            source_storage,
        };
        let now = timestamp.unwrap_or_else(|| self.sample_host_timer_clock());
        owners.validate()?;
        incoming.record.timestamp_ms = now;
        Ok(incoming)
    }

    fn console_prepare_output(
        &mut self,
        function: ConsoleFunction,
        level: &'static str,
        output: Output,
        owners: &ConsoleOwners,
        timestamp: Option<u64>,
    ) -> Result<NativeConsoleRecord, RuntimeError> {
        let origin = ReportOrigin::adopt(
            self.current_diagnostic_source_location(),
            owners.owner.as_ref(),
        )?;
        self.console_prepare_guarded(function, level, output, origin, owners, timestamp)
    }

    fn console_publish_prepared(
        &mut self,
        incoming: NativeConsoleRecord,
        owners: &ConsoleOwners,
        before: impl FnOnce(&mut ConsoleState),
        after: impl FnOnce(&mut ConsoleState),
    ) -> Result<(), RuntimeError> {
        // All input/backing/source admission and the last guest-capable clock
        // sample precede this pure state/record transaction.
        self.synchronize_scheduling_usage()?;
        owners.validate()?;
        let state = self
            .host_console
            .as_mut()
            .expect("console native has its state");
        before(state);
        state.push_prepared(incoming);
        after(state);
        let result = self.synchronize_scheduling_usage();
        owners.validate().and(result)
    }

    fn console_label(
        &mut self,
        value: Option<&Value>,
        owners: &ConsoleOwners,
    ) -> Result<Option<ConsoleLabel>, RuntimeError> {
        owners.validate()?;
        let key = match value {
            None | Some(Value::Undefined) => {
                error_input_storage::utf16_from_utf8("default", owners.owner.as_ref())?
            }
            Some(Value::String(text)) => {
                crate::vm::web_drag_drop::storage::copy_string(text, owners.owner.as_ref())?
            }
            Some(value) => {
                let text = self.coerce_string(value);
                let text = owners.validate().and(text)?;
                NativeValue::adopt(Value::String(text), owners.owner.as_ref())?
            }
        };
        let mut display =
            Output::owned_with_limit(MAX_HOST_CONSOLE_LABEL_BYTES, owners.owner.as_ref())?;
        let Value::String(text) = key.value() else {
            unreachable!()
        };
        display.string(text);
        display.validate()?;
        if display.truncated {
            let output = Output::message_owned(
                "Console label exceeds the byte limit",
                owners.owner.as_ref(),
            )?;
            let record =
                self.console_prepare_output(ConsoleFunction::Warn, "warn", output, owners, None)?;
            self.console_publish_prepared(record, owners, |_| {}, |_| {})?;
            Ok(None)
        } else {
            Ok(Some(ConsoleLabel { key, display }))
        }
    }

    pub(super) fn console_call(
        &mut self,
        function: ConsoleFunction,
        args: &[Value],
        construct: bool,
    ) -> Result<Value, RuntimeError> {
        if construct {
            return Err(RuntimeError::TypeError(
                "console methods are not constructors".into(),
            ));
        }
        let owners = self.console_owners(None)?;
        let roots =
            owners
                .context
                .prepare_roots(self, args.iter().filter_map(Value::object_id), 0)?;
        let result =
            self.with_realm_call_roots(roots, |vm| vm.console_call_owned(function, args, &owners));
        owners.validate().and(result)
    }

    fn console_call_owned(
        &mut self,
        function: ConsoleFunction,
        args: &[Value],
        owners: &ConsoleOwners,
    ) -> Result<Value, RuntimeError> {
        self.charge_step()?;
        owners.validate()?;
        match function {
            ConsoleFunction::Clear => {
                let output = Output::message_owned("Console was cleared", owners.owner.as_ref())?;
                let record = self.console_prepare_output(function, "log", output, owners, None)?;
                self.console_publish_prepared(
                    record,
                    owners,
                    |state| {
                        state.clear_records();
                        state.dropped = 0;
                        state.depth = 0;
                    },
                    |_| {},
                )?;
            }
            ConsoleFunction::GroupEnd => {
                let output = Output::owned(owners.owner.as_ref())?;
                let record = self.console_prepare_output(function, "log", output, owners, None)?;
                self.console_publish_prepared(
                    record,
                    owners,
                    |state| state.depth = state.depth.saturating_sub(1),
                    |_| {},
                )?;
            }
            ConsoleFunction::Count | ConsoleFunction::CountReset => {
                let Some(label) = self.console_label(args.first(), owners)? else {
                    return Ok(Value::Undefined);
                };
                if function == ConsoleFunction::CountReset {
                    if let Some(count) = self
                        .host_console
                        .as_mut()
                        .unwrap()
                        .counts
                        .get_mut(label.units())
                    {
                        *count = 0;
                        self.synchronize_scheduling_usage()?;
                        return Ok(Value::Undefined);
                    }
                }
                let timestamp = self.sample_host_timer_clock();
                owners.validate()?;
                let current = self
                    .host_console
                    .as_ref()
                    .unwrap()
                    .counts
                    .get(label.units())
                    .copied();
                if function == ConsoleFunction::CountReset {
                    if let Some(count) = self
                        .host_console
                        .as_mut()
                        .unwrap()
                        .counts
                        .get_mut(label.units())
                    {
                        *count = 0;
                    } else {
                        let mut output = Output::owned(owners.owner.as_ref())?;
                        output.formatted(format_args!(
                            "Count '{}' does not exist",
                            label.display.text
                        ))?;
                        let record = self.console_prepare_output(
                            function,
                            "warn",
                            output,
                            owners,
                            Some(timestamp),
                        )?;
                        self.console_publish_prepared(record, owners, |_| {}, |_| {})?;
                    }
                } else if current.is_none()
                    && self.host_console.as_ref().unwrap().counts.len() == MAX_HOST_CONSOLE_LABELS
                {
                    let output = Output::message_owned(
                        "Console count label limit reached",
                        owners.owner.as_ref(),
                    )?;
                    let record = self.console_prepare_output(
                        function,
                        "warn",
                        output,
                        owners,
                        Some(timestamp),
                    )?;
                    self.console_publish_prepared(record, owners, |_| {}, |_| {})?;
                } else {
                    let count = current.unwrap_or(0).saturating_add(1);
                    let mut output = Output::owned(owners.owner.as_ref())?;
                    output.formatted(format_args!("{}: {count}", label.display.text))?;
                    let record = self.console_prepare_output(
                        function,
                        "info",
                        output,
                        owners,
                        Some(timestamp),
                    )?;
                    let prepared = self
                        .host_console
                        .as_ref()
                        .unwrap()
                        .counts
                        .prepare(label.key)?;
                    drop(label.display);
                    self.console_publish_prepared(
                        record,
                        owners,
                        |state| prepared.commit(&mut state.counts, count),
                        |_| {},
                    )?;
                }
            }
            ConsoleFunction::Time | ConsoleFunction::TimeLog | ConsoleFunction::TimeEnd => {
                let Some(label) = self.console_label(args.first(), owners)? else {
                    return Ok(Value::Undefined);
                };
                let now = self.sample_host_timer_clock();
                owners.validate()?;
                let start = self
                    .host_console
                    .as_ref()
                    .unwrap()
                    .timers
                    .get(label.units())
                    .copied();
                if function == ConsoleFunction::Time
                    && start.is_none()
                    && self.host_console.as_ref().unwrap().timers.len() < MAX_HOST_CONSOLE_LABELS
                {
                    let prepared = self
                        .host_console
                        .as_ref()
                        .unwrap()
                        .timers
                        .prepare(label.key)?;
                    drop(label.display);
                    self.synchronize_scheduling_usage()?;
                    owners.validate()?;
                    prepared.commit(&mut self.host_console.as_mut().unwrap().timers, now);
                } else {
                    // Trailing previews do not invoke getters. Keep their
                    // independently owned text through the final clock sample.
                    let mut tail = Output::owned(owners.owner.as_ref())?;
                    if function == ConsoleFunction::TimeLog && start.is_some() {
                        for value in args.iter().skip(1) {
                            tail.push(" ");
                            self.console_preview(value, &mut tail)?;
                        }
                    }
                    tail.validate()?;
                    let timestamp = self.sample_host_timer_clock();
                    owners.validate()?;
                    let current = self
                        .host_console
                        .as_ref()
                        .unwrap()
                        .timers
                        .get(label.units())
                        .copied();
                    if function == ConsoleFunction::Time
                        && current.is_none()
                        && self.host_console.as_ref().unwrap().timers.len()
                            < MAX_HOST_CONSOLE_LABELS
                    {
                        let prepared = self
                            .host_console
                            .as_ref()
                            .unwrap()
                            .timers
                            .prepare(label.key)?;
                        drop(label.display);
                        self.synchronize_scheduling_usage()?;
                        owners.validate()?;
                        prepared.commit(&mut self.host_console.as_mut().unwrap().timers, now);
                    } else {
                        let mut output = Output::owned(owners.owner.as_ref())?;
                        let level = if function == ConsoleFunction::Time {
                            if current.is_some() {
                                output.formatted(format_args!(
                                    "Timer '{}' already exists",
                                    label.display.text
                                ))?;
                            } else {
                                output.push("Console timer label limit reached");
                            }
                            "warn"
                        } else if let Some(start) = current {
                            output.formatted(format_args!(
                                "{}: {} ms",
                                label.display.text,
                                now.saturating_sub(start)
                            ))?;
                            output.push(&tail.text);
                            output.truncated |= tail.truncated;
                            function.level()
                        } else {
                            output.formatted(format_args!(
                                "Timer '{}' does not exist",
                                label.display.text
                            ))?;
                            "warn"
                        };
                        output.validate()?;
                        drop(tail);
                        let record = self.console_prepare_output(
                            function,
                            level,
                            output,
                            owners,
                            Some(timestamp),
                        )?;
                        self.console_publish_prepared(
                            record,
                            owners,
                            |state| {
                                if function == ConsoleFunction::TimeEnd && current.is_some() {
                                    state.timers.remove(label.units());
                                }
                            },
                            |_| {},
                        )?;
                    }
                }
            }
            _ => {
                if function == ConsoleFunction::Assert {
                    if let Some(value) = args.first() {
                        if self.to_boolean(value)? {
                            return Ok(Value::Undefined);
                        }
                    }
                }
                let data = if function == ConsoleFunction::Assert {
                    args.get(1..).unwrap_or(&[])
                } else {
                    args
                };
                if data.is_empty()
                    && !matches!(
                        function,
                        ConsoleFunction::Assert
                            | ConsoleFunction::Trace
                            | ConsoleFunction::Group
                            | ConsoleFunction::GroupCollapsed
                            | ConsoleFunction::Dir
                            | ConsoleFunction::Table
                    )
                {
                    return Ok(Value::Undefined);
                }
                let mut output = Output::owned(owners.owner.as_ref())?;
                if function == ConsoleFunction::Assert {
                    output.push("Assertion failed");
                    if !data.is_empty() {
                        output.push(if matches!(data.first(), Some(Value::String(_))) {
                            ": "
                        } else {
                            " "
                        });
                    }
                }
                if matches!(function, ConsoleFunction::Dir | ConsoleFunction::Table) {
                    self.console_preview(data.first().unwrap_or(&Value::Undefined), &mut output)?;
                } else {
                    self.console_format(data, &mut output)?;
                }
                if function == ConsoleFunction::Trace {
                    self.console_trace(&mut output)?;
                }
                let record =
                    self.console_prepare_output(function, function.level(), output, owners, None)?;
                self.console_publish_prepared(
                    record,
                    owners,
                    |_| {},
                    |state| {
                        if matches!(
                            function,
                            ConsoleFunction::Group | ConsoleFunction::GroupCollapsed
                        ) {
                            state.depth = state.depth.saturating_add(1);
                        }
                    },
                )?;
            }
        }
        let result = self.synchronize_scheduling_usage();
        owners.validate().and(result)?;
        Ok(Value::Undefined)
    }
}
