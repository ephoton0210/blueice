// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

use super::*;
use crate::vm::host_scheduling::SharedSchedulingCharge;
use crate::vm::native_value_storage::NativeValue;
use crate::vm::web_event_storage::EventContext;

pub(in crate::vm) type RetainedText = (String, bool, Option<SharedSchedulingCharge>);

pub(in crate::vm) struct Output {
    pub text: String,
    pub truncated: bool,
    limit: usize,
    full: bool,
    metered: bool,
    failed: bool,
    storage: Option<SharedSchedulingCharge>,
}
impl Output {
    pub fn new() -> Self {
        Self::with_limit(MAX_HOST_CONSOLE_MESSAGE_BYTES)
    }
    pub fn with_limit(limit: usize) -> Self {
        Self {
            text: String::new(),
            truncated: false,
            limit,
            full: false,
            metered: false,
            failed: false,
            storage: None,
        }
    }
    pub(in crate::vm) fn owned(
        owner: Option<&SharedSchedulingCharge>,
    ) -> Result<Self, RuntimeError> {
        Ok(Self {
            metered: true,
            storage: owner.map(|owner| owner.reserve_peer(0)).transpose()?,
            ..Self::new()
        })
    }

    pub(super) fn owned_with_limit(
        limit: usize,
        owner: Option<&SharedSchedulingCharge>,
    ) -> Result<Self, RuntimeError> {
        let mut output = Self::owned(owner)?;
        output.limit = limit;
        Ok(output)
    }

    pub(in crate::vm) fn message_owned(
        text: &str,
        owner: Option<&SharedSchedulingCharge>,
    ) -> Result<Self, RuntimeError> {
        let mut output = Self::owned(owner)?;
        output.push(text);
        output.validate()?;
        Ok(output)
    }

    pub(super) fn formatted(&mut self, args: std::fmt::Arguments<'_>) -> Result<(), RuntimeError> {
        std::fmt::write(self, args).map_err(|_| RuntimeError::SchedulingLimit)?;
        self.validate()
    }

    pub(in crate::vm) fn validate(&self) -> Result<(), RuntimeError> {
        if self.failed {
            return Err(RuntimeError::SchedulingLimit);
        }
        self.owner_view()?;
        Ok(())
    }

    pub(in crate::vm) fn owner_view(&self) -> Result<Option<SharedSchedulingCharge>, RuntimeError> {
        self.storage
            .as_ref()
            .map(|owner| owner.reserve_peer(0))
            .transpose()
    }

    pub(in crate::vm) fn retain_for_owner(
        &mut self,
        owner: Option<&SharedSchedulingCharge>,
    ) -> Result<(), RuntimeError> {
        self.validate()?;
        if let Some(owner) = owner {
            if self
                .storage
                .as_ref()
                .is_none_or(|storage| !storage.same_owner(owner))
            {
                self.storage = Some(owner.reserve_peer(self.text.capacity())?);
            }
        }
        Ok(())
    }

    pub(in crate::vm) fn into_retained_text(self) -> Result<RetainedText, RuntimeError> {
        self.validate()?;
        Ok((self.text, self.truncated, self.storage))
    }

    fn character(&mut self, c: char) -> Result<(), RuntimeError> {
        let needed = self.text.len() + c.len_utf8();
        if needed > self.limit {
            self.truncated = true;
            self.full = true;
            return Ok(());
        }
        if self.metered && needed > self.text.capacity() {
            let capacity = self
                .text
                .capacity()
                .saturating_mul(2)
                .max(8)
                .max(needed)
                .min(self.limit);
            let mut storage = self
                .storage
                .as_ref()
                .map(|owner| owner.reserve_peer(capacity))
                .transpose()?;
            let mut text = String::new();
            text.try_reserve_exact(capacity).map_err(|_| {
                self.storage
                    .as_ref()
                    .map_or(RuntimeError::SchedulingLimit, SharedSchedulingCharge::fail)
            })?;
            if let Some(storage) = &mut storage {
                storage.resize(text.capacity())?;
            }
            text.push_str(&self.text);
            self.text = text;
            self.storage = storage;
        }
        self.text.push(c);
        Ok(())
    }

    fn append_character(&mut self, c: char) {
        if self.character(c).is_err() {
            self.failed = true;
            self.full = true;
        }
    }
    #[cfg(any(test, coverage))]
    pub fn message(text: &str) -> Self {
        let mut output = Self::new();
        output.push(text);
        output
    }
    pub fn push(&mut self, text: &str) {
        if self.full {
            return;
        }
        for c in text.chars() {
            self.append_character(c);
            if self.full {
                break;
            }
        }
    }
    pub fn units(&mut self, units: &[u16]) {
        if self.full {
            return;
        }
        for c in char::decode_utf16(units.iter().copied()) {
            let c = c.unwrap_or(char::REPLACEMENT_CHARACTER);
            self.append_character(c);
            if self.full {
                break;
            }
        }
    }
    pub fn string(&mut self, string: &JsString) {
        self.units(string.as_code_units());
    }
}

impl std::fmt::Write for Output {
    fn write_str(&mut self, text: &str) -> std::fmt::Result {
        self.push(text);
        if self.failed {
            Err(std::fmt::Error)
        } else {
            Ok(())
        }
    }
}

impl Vm {
    pub(in crate::vm) fn console_preview(
        &mut self,
        value: &Value,
        output: &mut Output,
    ) -> Result<(), RuntimeError> {
        let context = EventContext::capture(self)?;
        output.validate()?;
        self.charge_step()?;
        let result = (|| {
            match value {
                Value::String(value) => output.string(value),
                Value::Symbol(value) => {
                    output.push("Symbol(");
                    if let Some(description) = &value.description {
                        output.string(description);
                    }
                    output.push(")");
                }
                Value::BigInt(value) if value.bits() > (MAX_HOST_CONSOLE_MESSAGE_BYTES as u64) => {
                    output.push("[BigInt exceeds preview limit]");
                    output.truncated = true;
                }
                Value::BigInt(value) => {
                    output.formatted(format_args!("{value}n"))?;
                }
                Value::Object(id) => {
                    match self.heap.debugger_owned_value_preview(
                        value,
                        self.object_prototype,
                        self.array_prototype,
                    ) {
                        Ok(preview) => render_preview(&preview, output)?,
                        Err(RuntimeError::SchedulingLimit) => {
                            return Err(RuntimeError::SchedulingLimit)
                        }
                        Err(_) => {
                            if self.is_callable(value)? {
                                output.push("[Function");
                                if let Some(name) =
                                    self.heap.own_string_property_ref(*id, "name")?
                                {
                                    output.push(": ");
                                    output.string(name);
                                }
                                output.push("]");
                            } else {
                                output.push("[Object");
                                if let Some(message) =
                                    self.heap.own_string_property_ref(*id, "message")?
                                {
                                    output.push(": ");
                                    output.string(message);
                                }
                                output.push("]");
                                output.truncated = true;
                            }
                        }
                    }
                }
                _ => {
                    let text = self.coerce_string(value);
                    let text = context.validate().and(output.validate()).and(text)?;
                    let owner = output.owner_view()?;
                    let text = NativeValue::adopt(Value::String(text), owner.as_ref())?;
                    let Value::String(text) = text.value() else {
                        unreachable!()
                    };
                    output.string(text);
                }
            }
            Ok(())
        })();
        context.validate().and(output.validate()).and(result)
    }

    pub(super) fn console_format(
        &mut self,
        values: &[Value],
        output: &mut Output,
    ) -> Result<(), RuntimeError> {
        let context = EventContext::capture(self)?;
        output.validate()?;
        let Some(first) = values.first() else {
            return Ok(());
        };
        let mut used = 1;
        if let Value::String(format) = first {
            let units = format.as_code_units();
            let mut cursor = 0;
            let mut literal = 0;
            let mut scans = 0_u32;
            while cursor + 1 < units.len() && used < values.len() {
                scans += 1;
                if scans == 64 {
                    self.charge_step()?;
                    scans = 0;
                }
                if units[cursor] != u16::from(b'%') {
                    cursor += 1;
                    continue;
                }
                let specifier = units[cursor + 1];
                if ![b's', b'd', b'i', b'f', b'o', b'O', b'c', b'%']
                    .iter()
                    .any(|c| u16::from(*c) == specifier)
                {
                    cursor += 2;
                    continue;
                }
                output.units(&units[literal..cursor]);
                context.validate()?;
                output.validate()?;
                if specifier == u16::from(b'%') {
                    output.push("%");
                } else {
                    self.charge_step()?;
                    let value = &values[used];
                    used += 1;
                    match specifier as u8 {
                        b's' => {
                            if let Value::Symbol(symbol) = value {
                                output.push("Symbol(");
                                if let Some(description) = &symbol.description {
                                    output.string(description);
                                }
                                output.push(")");
                            } else {
                                let converted = self.string_constructor_argument(value, false);
                                let converted =
                                    context.validate().and(output.validate()).and(converted)?;
                                let owner = output.owner_view()?;
                                let converted =
                                    NativeValue::adopt(Value::String(converted), owner.as_ref())?;
                                let Value::String(text) = converted.value() else {
                                    unreachable!()
                                };
                                output.string(text);
                            }
                        }
                        b'd' | b'i' | b'f' => {
                            let number = if matches!(value, Value::Symbol(_)) {
                                Value::Number(f64::NAN)
                            } else if specifier == u16::from(b'f') {
                                let parsed = self.parse_float(value);
                                context.validate().and(output.validate()).and(parsed)?
                            } else {
                                let parsed = self.parse_int(value, &Value::Number(10.0));
                                context.validate().and(output.validate()).and(parsed)?
                            };
                            self.console_preview(&number, output)?;
                        }
                        b'o' | b'O' => self.console_preview(value, output)?,
                        b'c' => { /* Styling is deliberately not interpreted by the text printer. */
                        }
                        _ => unreachable!(),
                    }
                }
                context.validate()?;
                output.validate()?;
                cursor += 2;
                literal = cursor;
            }
            output.units(&units[literal..]);
        } else {
            self.console_preview(first, output)?;
        }
        for value in &values[used..] {
            output.push(" ");
            self.console_preview(value, output)?;
        }
        context.validate().and(output.validate())
    }

    pub(super) fn console_trace(&self, output: &mut Output) -> Result<(), RuntimeError> {
        let context = EventContext::capture(self)?;
        output.validate()?;
        if self.call_stack.is_empty() {
            output.push("\n    at <script>");
        }
        for id in self.call_stack.iter().rev().take(8) {
            output.push("\n    at ");
            if let Ok(Some(name)) = self.heap.own_string_property_ref(*id, "name") {
                output.string(name);
            } else {
                output.push("<anonymous>");
            }
        }
        if self.call_stack.len() > 8 {
            output.truncated = true;
        }
        context.validate().and(output.validate())
    }
}

fn render_preview(value: &VmDebuggerValuePreview, output: &mut Output) -> Result<(), RuntimeError> {
    match value {
        VmDebuggerValuePreview::Undefined => output.push("undefined"),
        VmDebuggerValuePreview::Null => output.push("null"),
        VmDebuggerValuePreview::Bool(value) => output.push(if *value { "true" } else { "false" }),
        VmDebuggerValuePreview::NumberBits(bits) => {
            let value = f64::from_bits(*bits);
            if value.is_nan() {
                output.push("NaN");
            } else if value == f64::INFINITY {
                output.push("Infinity");
            } else if value == f64::NEG_INFINITY {
                output.push("-Infinity");
            } else {
                output.formatted(format_args!("{value}"))?;
            }
        }
        VmDebuggerValuePreview::BigIntBytes(bytes) => {
            let owner = output.owner_view()?;
            let mut storage = owner
                .as_ref()
                .map(|owner| owner.reserve_peer(bytes.len().div_ceil(4).saturating_mul(4)))
                .transpose()?;
            let value = Value::BigInt(num_bigint::BigInt::from_signed_bytes_le(bytes));
            if let Some(storage) = &mut storage {
                storage.resize(host_scheduling::value_payload_bytes(&value))?;
            }
            let value = NativeValue::from_retained_parts(value, storage);
            let Value::BigInt(value) = value.value() else {
                unreachable!()
            };
            output.formatted(format_args!("{value}n"))?;
        }
        VmDebuggerValuePreview::StringUnits(units) => {
            output.push("\"");
            output.units(units);
            output.push("\"");
        }
        VmDebuggerValuePreview::Array(values) => {
            output.push("[");
            for (index, value) in values.iter().enumerate() {
                if index > 0 {
                    output.push(", ");
                }
                if let Some(value) = value {
                    render_preview(value, output)?;
                } else {
                    output.push("<empty>");
                }
            }
            output.push("]");
        }
        VmDebuggerValuePreview::Record(values) => {
            output.push("{");
            for (index, (key, value)) in values.iter().enumerate() {
                if index > 0 {
                    output.push(", ");
                }
                output.string(key);
                output.push(": ");
                render_preview(value, output)?;
            }
            output.push("}");
        }
    }
    output.validate()
}
