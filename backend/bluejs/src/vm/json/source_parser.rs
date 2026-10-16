// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

//! Shared JSON grammar; module parses lease their AST and omit reviver sources.

use super::*;
use crate::vm::host_scheduling::SharedSchedulingCharge;

const MAX_MODULE_JSON_DEPTH: usize = 128;

#[cfg(any(test, coverage))]
#[path = "../../../tests/fixtures/json_parser_ownership.rs"]
mod ownership_tests;

pub(super) struct ParsedJson {
    node: JsonNode,
    pub(super) depth: usize,
    _charge: Option<SharedSchedulingCharge>,
}

impl std::ops::Deref for ParsedJson {
    type Target = JsonNode;
    fn deref(&self) -> &JsonNode {
        &self.node
    }
}

pub(super) struct JsonSourceParser<'a> {
    input: &'a str,
    index: usize,
    retain_source: bool,
    depth: usize,
    max_depth: usize,
    depth_limit: Option<usize>,
    owned_bytes: usize,
    charge: Option<SharedSchedulingCharge>,
}

impl<'a> JsonSourceParser<'a> {
    pub(super) fn new(input: &'a str) -> Self {
        Self {
            input,
            index: 0,
            retain_source: true,
            depth: 0,
            max_depth: 0,
            depth_limit: None,
            owned_bytes: 0,
            charge: None,
        }
    }

    pub(super) fn for_module(
        input: &'a str,
        scope: Option<&SchedulingScope>,
    ) -> Result<Self, RuntimeError> {
        Ok(Self {
            retain_source: false,
            depth_limit: scope.map(|_| MAX_MODULE_JSON_DEPTH),
            charge: scope.map(|scope| scope.shared_charge(0)).transpose()?,
            ..Self::new(input)
        })
    }

    fn syntax() -> RuntimeError {
        RuntimeError::SyntaxError("invalid JSON text".into())
    }

    fn resize(&mut self, bytes: usize) -> Result<(), RuntimeError> {
        if let Some(charge) = &mut self.charge {
            charge.resize(bytes)?;
        }
        Ok(())
    }

    fn reserve<T>(&mut self, values: &mut Vec<T>, capacity: usize) -> Result<(), RuntimeError> {
        if capacity <= values.capacity() {
            return Ok(());
        }
        let old = values.capacity().saturating_mul(std::mem::size_of::<T>());
        let planned = capacity.saturating_mul(std::mem::size_of::<T>());
        // Reserve a complete replacement while the prior allocation exists.
        self.resize(self.owned_bytes.saturating_add(planned))?;
        values
            .try_reserve_exact(capacity.saturating_sub(values.len()))
            .map_err(|_| {
                self.charge
                    .as_ref()
                    .map_or(RuntimeError::SchedulingLimit, SharedSchedulingCharge::fail)
            })?;
        let actual = values.capacity().saturating_mul(std::mem::size_of::<T>());
        self.resize(self.owned_bytes.saturating_add(actual))?;
        self.owned_bytes = self.owned_bytes.saturating_sub(old).saturating_add(actual);
        self.resize(self.owned_bytes)
    }

    fn append<T>(&mut self, values: &mut Vec<T>, value: T) -> Result<(), RuntimeError> {
        if values.len() == values.capacity() {
            self.reserve(
                values,
                values
                    .len()
                    .saturating_add(1)
                    .max(values.capacity().saturating_mul(2))
                    .max(4),
            )?;
        }
        values.push(value);
        Ok(())
    }

    fn source(&mut self, start: usize) -> Result<JsString, RuntimeError> {
        if !self.retain_source {
            return Ok(JsString::default());
        }
        let text = &self.input[start..self.index];
        let mut units = Vec::new();
        self.reserve(&mut units, text.encode_utf16().count())?;
        units.extend(text.encode_utf16());
        Ok(JsString::from_code_units(units))
    }

    pub(super) fn parse(mut self) -> Result<ParsedJson, RuntimeError> {
        self.skip_whitespace();
        let node = self.value()?;
        self.skip_whitespace();
        if self.index != self.input.len() {
            return Err(Self::syntax());
        }
        Ok(ParsedJson {
            node,
            depth: self.max_depth,
            _charge: self.charge,
        })
    }

    fn value(&mut self) -> Result<JsonNode, RuntimeError> {
        self.skip_whitespace();
        let start = self.index;
        match self.peek() {
            Some(b'{' | b'[') => {
                if self.depth_limit.is_some_and(|limit| self.depth >= limit) {
                    return Err(RuntimeError::InstructionLimit);
                }
                self.depth += 1;
                self.max_depth = self.max_depth.max(self.depth);
                let result = if self.peek() == Some(b'{') {
                    self.object()
                } else {
                    self.array()
                };
                self.depth -= 1;
                result
            }
            Some(b'"') => {
                let value = Value::String(self.string()?);
                Ok(JsonNode::Primitive {
                    value,
                    source: self.source(start)?,
                })
            }
            Some(b't') => self.keyword("true", Value::Bool(true), start),
            Some(b'f') => self.keyword("false", Value::Bool(false), start),
            Some(b'n') => self.keyword("null", Value::Null, start),
            Some(b'-' | b'0'..=b'9') => self.number(start),
            _ => Err(Self::syntax()),
        }
    }

    fn array(&mut self) -> Result<JsonNode, RuntimeError> {
        self.index += 1;
        self.skip_whitespace();
        if self.take(b']') {
            return Ok(JsonNode::Array(Vec::new()));
        }
        let mut values = Vec::new();
        loop {
            let value = self.value()?;
            self.append(&mut values, value)?;
            self.skip_whitespace();
            if self.take(b']') {
                return Ok(JsonNode::Array(values));
            }
            self.expect(b',')?;
            self.skip_whitespace();
        }
    }

    fn object(&mut self) -> Result<JsonNode, RuntimeError> {
        self.index += 1;
        self.skip_whitespace();
        if self.take(b'}') {
            return Ok(JsonNode::Object(Vec::new()));
        }
        let mut values = Vec::new();
        loop {
            self.skip_whitespace();
            let key = self.string()?;
            self.skip_whitespace();
            self.expect(b':')?;
            let value = self.value()?;
            self.append(&mut values, (key, value))?;
            self.skip_whitespace();
            if self.take(b'}') {
                return Ok(JsonNode::Object(values));
            }
            self.expect(b',')?;
            self.skip_whitespace();
        }
    }

    fn keyword(
        &mut self,
        keyword: &str,
        value: Value,
        start: usize,
    ) -> Result<JsonNode, RuntimeError> {
        if !self.input[self.index..].starts_with(keyword) {
            return Err(Self::syntax());
        }
        self.index += keyword.len();
        Ok(JsonNode::Primitive {
            value,
            source: self.source(start)?,
        })
    }

    fn number(&mut self, start: usize) -> Result<JsonNode, RuntimeError> {
        self.take(b'-');
        match self.peek() {
            Some(b'0') => self.index += 1,
            Some(b'1'..=b'9') => {
                self.index += 1;
                while matches!(self.peek(), Some(b'0'..=b'9')) {
                    self.index += 1;
                }
            }
            _ => return Err(Self::syntax()),
        }
        if self.take(b'.') {
            let decimal_start = self.index;
            while matches!(self.peek(), Some(b'0'..=b'9')) {
                self.index += 1;
            }
            if self.index == decimal_start {
                return Err(Self::syntax());
            }
        }
        if matches!(self.peek(), Some(b'e' | b'E')) {
            self.index += 1;
            self.take(b'+');
            self.take(b'-');
            let exponent_start = self.index;
            while matches!(self.peek(), Some(b'0'..=b'9')) {
                self.index += 1;
            }
            if self.index == exponent_start {
                return Err(Self::syntax());
            }
        }
        let value = self.input[start..self.index]
            .parse::<f64>()
            .map_err(|_| Self::syntax())?;
        Ok(JsonNode::Primitive {
            value: Value::Number(value),
            source: self.source(start)?,
        })
    }

    fn string(&mut self) -> Result<JsString, RuntimeError> {
        let start = self.index;
        self.expect(b'"')?;
        loop {
            match self.peek() {
                Some(b'"') => {
                    let body = &self.input[start + 1..self.index];
                    self.index += 1;
                    let mut length = 0;
                    decode_string(body, |_| length += 1);
                    let mut units = Vec::new();
                    self.reserve(&mut units, length)?;
                    decode_string(body, |unit| units.push(unit));
                    return Ok(JsString::from_code_units(units));
                }
                Some(b'\\') => {
                    self.index += 1;
                    match self.peek() {
                        Some(b'"' | b'\\' | b'/' | b'b' | b'f' | b'n' | b'r' | b't') => {
                            self.index += 1
                        }
                        Some(b'u') => {
                            self.index += 1;
                            for _ in 0..4 {
                                if !matches!(
                                    self.peek(),
                                    Some(b'0'..=b'9' | b'a'..=b'f' | b'A'..=b'F')
                                ) {
                                    return Err(Self::syntax());
                                }
                                self.index += 1;
                            }
                        }
                        _ => return Err(Self::syntax()),
                    }
                }
                Some(byte) if byte < 0x20 => return Err(Self::syntax()),
                Some(_) => {
                    self.index += self.input[self.index..]
                        .chars()
                        .next()
                        .expect("UTF-8 character boundary")
                        .len_utf8()
                }
                None => return Err(Self::syntax()),
            }
        }
    }

    fn skip_whitespace(&mut self) {
        while matches!(self.peek(), Some(b' ' | b'\n' | b'\r' | b'\t')) {
            self.index += 1;
        }
    }
    fn expect(&mut self, byte: u8) -> Result<(), RuntimeError> {
        self.take(byte).then_some(()).ok_or_else(Self::syntax)
    }
    fn take(&mut self, byte: u8) -> bool {
        if self.peek() == Some(byte) {
            self.index += 1;
            true
        } else {
            false
        }
    }
    fn peek(&self) -> Option<u8> {
        self.input.as_bytes().get(self.index).copied()
    }
}

/// Decode an already validated token, preserving lone surrogate code units.
fn decode_string(body: &str, mut emit: impl FnMut(u16)) {
    let bytes = body.as_bytes();
    let mut index = 0;
    while index < bytes.len() {
        if bytes[index] == b'\\' {
            let unit = match bytes[index + 1] {
                b'"' => u16::from(b'"'),
                b'\\' => u16::from(b'\\'),
                b'/' => u16::from(b'/'),
                b'b' => 0x08,
                b'f' => 0x0c,
                b'n' => u16::from(b'\n'),
                b'r' => u16::from(b'\r'),
                b't' => u16::from(b'\t'),
                _ => {
                    let unit = u16::from_str_radix(&body[index + 2..index + 6], 16)
                        .expect("validated hex escape");
                    index += 6;
                    emit(unit);
                    continue;
                }
            };
            emit(unit);
            index += 2;
        } else {
            let character = body[index..]
                .chars()
                .next()
                .expect("UTF-8 character boundary");
            let mut units = [0; 2];
            for &unit in character.encode_utf16(&mut units).iter() {
                emit(unit);
            }
            index += character.len_utf8();
        }
    }
}
