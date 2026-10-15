// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

//! Handle-free, document-bound page console output. Text is untrusted data.
use serde::{Deserialize, Serialize};

pub const MAX_CONSOLE_RECORDS: usize = 256;
pub const MAX_CONSOLE_BYTES: usize = 256 * 1024;
pub const MAX_CONSOLE_MESSAGE_BYTES: usize = 8 * 1024;
pub const MAX_CONSOLE_SOURCE_NAME_BYTES: usize = 8 * 1024;
pub const MAX_CONSOLE_GROUP_DEPTH: u32 = 32;

/// Copied diagnostic data. A filename is untrusted text, not an authority or
/// a request to load a URL. Coordinates are one-based; columns count original
/// UTF-16 units.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ConsoleSourceLocation {
    pub filename: String,
    pub line: u32,
    pub column: u32,
}

impl ConsoleSourceLocation {
    pub fn is_well_formed(&self) -> bool {
        !self.filename.is_empty()
            && self.filename.len() <= MAX_CONSOLE_SOURCE_NAME_BYTES
            && self.line != 0
            && self.column != 0
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ConsoleRecord {
    pub sequence: u64,
    pub timestamp_ms: u64,
    pub method: String,
    pub level: String,
    pub group_depth: u32,
    pub message: String,
    pub truncated: bool,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub source: Option<ConsoleSourceLocation>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ConsoleMessages {
    pub tab_id: u64,
    pub document_generation: u64,
    pub records: Vec<ConsoleRecord>,
    pub dropped_records: u64,
}

impl ConsoleMessages {
    pub fn is_well_formed(&self) -> bool {
        self.tab_id != 0
            && self.document_generation != 0
            && self.records.len() <= MAX_CONSOLE_RECORDS
            && self.records.iter().fold(0usize, |bytes, record| {
                bytes.saturating_add(record.message.len()).saturating_add(
                    record
                        .source
                        .as_ref()
                        .map_or(0, |source| source.filename.len()),
                )
            }) <= MAX_CONSOLE_BYTES
            && self.records.iter().all(|record| {
                record.sequence != 0
                    && record
                        .source
                        .as_ref()
                        .is_none_or(ConsoleSourceLocation::is_well_formed)
                    && record.message.len() <= MAX_CONSOLE_MESSAGE_BYTES
                    && record.group_depth <= MAX_CONSOLE_GROUP_DEPTH
                    && matches!(
                        record.level.as_str(),
                        "debug" | "info" | "log" | "warn" | "error"
                    )
                    && matches!(
                        record.method.as_str(),
                        "assert"
                            | "clear"
                            | "debug"
                            | "error"
                            | "info"
                            | "log"
                            | "table"
                            | "trace"
                            | "warn"
                            | "dir"
                            | "dirxml"
                            | "count"
                            | "countReset"
                            | "group"
                            | "groupCollapsed"
                            | "groupEnd"
                            | "time"
                            | "timeLog"
                            | "timeEnd"
                    )
            })
            && self.records.windows(2).all(|pair| {
                pair[0].sequence < pair[1].sequence && pair[0].timestamp_ms <= pair[1].timestamp_ms
            })
    }
}
