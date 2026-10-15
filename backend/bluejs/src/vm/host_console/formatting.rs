// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

use super::*;

pub(in crate::vm) struct Output {
    pub text: String,
    pub truncated: bool,
    limit: usize,
    full: bool,
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
        }
    }
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
            if self.text.len() + c.len_utf8() > self.limit {
                self.truncated = true;
                self.full = true;
                break;
            }
            self.text.push(c);
        }
    }
    pub fn units(&mut self, units: &[u16]) {
        if self.full {
            return;
        }
        for c in char::decode_utf16(units.iter().copied()) {
            let c = c.unwrap_or(char::REPLACEMENT_CHARACTER);
            if self.text.len() + c.len_utf8() > self.limit {
                self.truncated = true;
                self.full = true;
                break;
            }
            self.text.push(c);
        }
    }
    pub fn string(&mut self, string: &JsString) {
        self.units(string.as_code_units());
    }
}

impl Vm {
    pub(in crate::vm) fn console_preview(
        &mut self,
        value: &Value,
        output: &mut Output,
    ) -> Result<(), RuntimeError> {
        self.charge_step()?;
        match value {
            Value::String(value) => output.string(value),
            Value::Symbol(value) => output.string(&value.descriptive_string()),
            Value::BigInt(value) if value.bits() > (MAX_HOST_CONSOLE_MESSAGE_BYTES as u64) => {
                output.push("[BigInt exceeds preview limit]");
                output.truncated = true;
            }
            Value::BigInt(value) => {
                output.push(&value.to_string());
                output.push("n");
            }
            Value::Object(id) => {
                match self.heap.debugger_value_preview(
                    value,
                    self.object_prototype,
                    self.array_prototype,
                ) {
                    Ok(preview) => render_preview(&preview, output),
                    Err(_) => {
                        if self.is_callable(value)? {
                            output.push("[Function");
                            if let Some(Value::String(name)) = self.heap.get_own(*id, "name")? {
                                output.push(": ");
                                output.string(&name);
                            }
                            output.push("]");
                        } else {
                            output.push("[Object");
                            if let Some(Value::String(message)) =
                                self.heap.get_own(*id, "message")?
                            {
                                output.push(": ");
                                output.string(&message);
                            }
                            output.push("]");
                            output.truncated = true;
                        }
                    }
                }
            }
            _ => output.string(&self.coerce_string(value)?),
        }
        Ok(())
    }

    pub(super) fn console_format(
        &mut self,
        values: &[Value],
        output: &mut Output,
    ) -> Result<(), RuntimeError> {
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
                if specifier == u16::from(b'%') {
                    output.push("%");
                } else {
                    self.charge_step()?;
                    let value = &values[used];
                    used += 1;
                    match specifier as u8 {
                        b's' => output.string(&self.string_constructor_argument(value, false)?),
                        b'd' | b'i' | b'f' => {
                            let number = if matches!(value, Value::Symbol(_)) {
                                Value::Number(f64::NAN)
                            } else if specifier == u16::from(b'f') {
                                self.parse_float(value)?
                            } else {
                                self.parse_int(value, &Value::Number(10.0))?
                            };
                            self.console_preview(&number, output)?;
                        }
                        b'o' | b'O' => self.console_preview(value, output)?,
                        b'c' => { /* Styling is deliberately not interpreted by the text printer. */
                        }
                        _ => unreachable!(),
                    }
                }
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
        Ok(())
    }

    pub(super) fn console_trace(&self, output: &mut Output) {
        if self.call_stack.is_empty() {
            output.push("\n    at <script>");
        }
        for id in self.call_stack.iter().rev().take(8) {
            output.push("\n    at ");
            if let Ok(Some(Value::String(name))) = self.heap.get_own(*id, "name") {
                output.string(&name);
            } else {
                output.push("<anonymous>");
            }
        }
        if self.call_stack.len() > 8 {
            output.truncated = true;
        }
    }
}

fn render_preview(value: &VmDebuggerValuePreview, output: &mut Output) {
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
                output.push(&value.to_string());
            }
        }
        VmDebuggerValuePreview::BigIntBytes(bytes) => {
            output.push(&num_bigint::BigInt::from_signed_bytes_le(bytes).to_string());
            output.push("n");
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
                    render_preview(value, output);
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
                render_preview(value, output);
            }
            output.push("}");
        }
    }
}
