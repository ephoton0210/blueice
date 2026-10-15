// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

//! Bounded, owner-installed page diagnostics. No live JS value is retained.
//! https://console.spec.whatwg.org/

use super::*;
use std::collections::{BTreeMap, VecDeque};
pub(super) mod formatting;
use formatting::Output;

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
    counts: BTreeMap<Vec<u16>, u64>,
    timers: BTreeMap<Vec<u16>, u64>,
}

impl ConsoleState {
    fn new() -> Self {
        Self {
            records: VecDeque::with_capacity(MAX_HOST_CONSOLE_RECORDS),
            bytes: 0,
            dropped: 0,
            sequence: 0,
            depth: 0,
            counts: BTreeMap::new(),
            timers: BTreeMap::new(),
        }
    }
    pub(super) fn payload_bytes(&self) -> usize {
        self.records.capacity() * std::mem::size_of::<HostConsoleRecord>()
            + self.bytes
            + (self.counts.len() + self.timers.len()) * std::mem::size_of::<(Vec<u16>, u64)>()
            + self
                .counts
                .keys()
                .chain(self.timers.keys())
                .map(|key| key.capacity() * std::mem::size_of::<u16>())
                .sum::<usize>()
    }
    fn push(&mut self, function: ConsoleFunction, level: &'static str, now: u64, output: Output) {
        let record = HostConsoleRecord {
            sequence: self.sequence.saturating_add(1),
            timestamp_ms: now,
            method: function.name(),
            level,
            group_depth: self.depth.min(u64::from(MAX_HOST_CONSOLE_GROUP_DEPTH)) as u32,
            message: output.text,
            truncated: output.truncated || self.depth > u64::from(MAX_HOST_CONSOLE_GROUP_DEPTH),
        };
        self.sequence = record.sequence;
        while self.records.len() >= MAX_HOST_CONSOLE_RECORDS
            || self.bytes + record.message.capacity() > MAX_HOST_CONSOLE_BYTES
        {
            let old = self
                .records
                .pop_front()
                .expect("one bounded message fits the empty console");
            self.bytes -= old.message.capacity();
            self.dropped = self.dropped.saturating_add(1);
        }
        self.bytes += record.message.capacity();
        self.records.push_back(record);
    }
}

impl Vm {
    pub fn install_host_console(&mut self) -> Result<(), RuntimeError> {
        let global = self.global("globalThis")?.object_id().unwrap();
        if self.host_console.is_some() || self.heap.get_own(global, "console")?.is_some() {
            return Err(RuntimeError::TypeError(
                "console is already installed or defined".into(),
            ));
        }
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
            self.host_console = Some(ConsoleState::new());
            self.synchronize_scheduling_usage()
        })();
        self.stack.truncate(base);
        result
    }

    /// Transfers handle-free records without executing script, jobs or tasks.
    pub fn drain_host_console(&mut self) -> Result<HostConsoleBatch, RuntimeError> {
        let result = match &mut self.host_console {
            None => HostConsoleBatch::default(),
            Some(console) => {
                console.bytes = 0;
                HostConsoleBatch {
                    records: console.records.drain(..).collect(),
                    dropped_records: std::mem::take(&mut console.dropped),
                }
            }
        };
        self.synchronize_scheduling_usage()?;
        Ok(result)
    }

    pub(super) fn console_emit(
        &mut self,
        function: ConsoleFunction,
        level: &'static str,
        output: Output,
    ) -> Result<(), RuntimeError> {
        let now = self.sample_host_timer_clock();
        self.host_console
            .as_mut()
            .expect("console native has its state")
            .push(function, level, now, output);
        self.synchronize_scheduling_usage()
    }

    fn console_label(
        &mut self,
        value: Option<&Value>,
    ) -> Result<Option<(Vec<u16>, String)>, RuntimeError> {
        let string = match value {
            None | Some(Value::Undefined) => "default".into(),
            Some(value) => self.coerce_string(value)?,
        };
        let mut label = Output::with_limit(MAX_HOST_CONSOLE_LABEL_BYTES);
        label.string(&string);
        if label.truncated {
            self.console_emit(
                ConsoleFunction::Warn,
                "warn",
                Output::message("Console label exceeds the byte limit"),
            )?;
            Ok(None)
        } else {
            Ok(Some((string.as_code_units().to_vec(), label.text)))
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
        self.charge_step()?;
        match function {
            ConsoleFunction::Clear => {
                let state = self.host_console.as_mut().unwrap();
                state.records.clear();
                state.bytes = 0;
                state.dropped = 0;
                state.depth = 0;
                self.console_emit(function, "log", Output::message("Console was cleared"))?;
            }
            ConsoleFunction::GroupEnd => {
                let state = self.host_console.as_mut().unwrap();
                state.depth = state.depth.saturating_sub(1);
                self.console_emit(function, "log", Output::message(""))?;
            }
            ConsoleFunction::Count | ConsoleFunction::CountReset => {
                let Some((key, label)) = self.console_label(args.first())? else {
                    return Ok(Value::Undefined);
                };
                let state = self.host_console.as_mut().unwrap();
                if function == ConsoleFunction::CountReset {
                    if let Some(count) = state.counts.get_mut(&key) {
                        *count = 0;
                    } else {
                        self.console_emit(
                            function,
                            "warn",
                            Output::message(&format!("Count '{label}' does not exist")),
                        )?;
                    }
                } else if !state.counts.contains_key(&key)
                    && state.counts.len() == MAX_HOST_CONSOLE_LABELS
                {
                    self.console_emit(
                        function,
                        "warn",
                        Output::message("Console count label limit reached"),
                    )?;
                } else {
                    let count = state.counts.entry(key).or_default();
                    *count = count.saturating_add(1);
                    let message = format!("{label}: {count}");
                    self.console_emit(function, "info", Output::message(&message))?;
                }
            }
            ConsoleFunction::Time | ConsoleFunction::TimeLog | ConsoleFunction::TimeEnd => {
                let Some((key, label)) = self.console_label(args.first())? else {
                    return Ok(Value::Undefined);
                };
                let now = self.sample_host_timer_clock();
                let state = self.host_console.as_mut().unwrap();
                let start = state.timers.get(&key).copied();
                if function == ConsoleFunction::Time {
                    if start.is_some() {
                        self.console_emit(
                            function,
                            "warn",
                            Output::message(&format!("Timer '{label}' already exists")),
                        )?;
                    } else if state.timers.len() == MAX_HOST_CONSOLE_LABELS {
                        self.console_emit(
                            function,
                            "warn",
                            Output::message("Console timer label limit reached"),
                        )?;
                    } else {
                        state.timers.insert(key, now);
                    }
                } else if let Some(start) = start {
                    if function == ConsoleFunction::TimeEnd {
                        state.timers.remove(&key);
                    }
                    let mut output =
                        Output::message(&format!("{label}: {} ms", now.saturating_sub(start)));
                    if function == ConsoleFunction::TimeLog {
                        for value in args.iter().skip(1) {
                            output.push(" ");
                            self.console_preview(value, &mut output)?;
                        }
                    }
                    self.console_emit(function, function.level(), output)?;
                } else {
                    self.console_emit(
                        function,
                        "warn",
                        Output::message(&format!("Timer '{label}' does not exist")),
                    )?;
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
                let mut output = Output::new();
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
                    self.console_trace(&mut output);
                }
                self.console_emit(function, function.level(), output)?;
                if matches!(
                    function,
                    ConsoleFunction::Group | ConsoleFunction::GroupCollapsed
                ) {
                    let state = self.host_console.as_mut().unwrap();
                    state.depth = state.depth.saturating_add(1);
                }
            }
        }
        self.synchronize_scheduling_usage()?;
        Ok(Value::Undefined)
    }
}
