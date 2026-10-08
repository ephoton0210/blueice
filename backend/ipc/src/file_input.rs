// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

//! Selected content, never a filesystem path or a request to open an OS
//! picker. Native frontends read only files explicitly chosen by their user.
use serde::{Deserialize, Serialize};

pub const MAX_SELECTED_FILES: usize = 16;
pub const MAX_SELECTED_BYTES: usize = 1_048_576;
pub const MAX_DOCUMENT_SELECTED_BYTES: usize = 4 * MAX_SELECTED_BYTES;
pub const MAX_DOCUMENT_SELECTED_FILES: usize = 64;

#[derive(Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct FileData {
    pub name: String,
    pub media_type: String,
    /// Milliseconds since the Unix epoch, read from the selected descriptor.
    /// Missing legacy metadata is represented by zero.
    #[serde(default)]
    pub last_modified: i64,
    pub bytes: Vec<u8>,
}
impl std::fmt::Debug for FileData {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("FileData")
            .field("bytes", &self.bytes.len())
            .finish_non_exhaustive()
    }
}
impl FileData {
    pub fn valid(&self) -> bool {
        !self.name.is_empty()
            && self.name.len() <= 255
            && !matches!(self.name.as_str(), "." | "..")
            && !self
                .name
                .chars()
                .any(|c| c.is_control() || matches!(c, '/' | '\\' | ':'))
            && self.media_type.len() <= 127
            && self.media_type.split('/').count() == 2
            && self.media_type.split('/').all(|s| {
                !s.is_empty()
                    && s.bytes()
                        .all(|b| b.is_ascii_alphanumeric() || b"!#$&^_.+-".contains(&b))
            })
            && self.bytes.len() <= MAX_SELECTED_BYTES
    }
}
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct FileInputContext {
    pub tab_id: u64,
    pub frame_source: u64,
    pub document_generation: u64,
    pub node_id: u64,
    pub revision: u64,
}
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct FileInputState {
    pub context: FileInputContext,
    pub multiple: bool,
    pub accept: String,
    pub names: Vec<String>,
}
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum FileInputAction {
    /// Checks the context before a native frontend starts reading its selected
    /// descriptors. It does not deliver another activation/click event.
    Validate { context: FileInputContext },
    /// Dismissal preserves selected content and delivers the cancel event.
    Cancel { context: FileInputContext },
    /// Validates and applies the page's ordinary click default, then returns
    /// hints. Receiving this reply must never itself open a native picker.
    Prepare {
        frame_source: u64,
        document_generation: u64,
        node_id: u64,
    },
    /// Caller-provided bytes cannot cause core to open any OS file. This
    /// operation is not a human permission grant or a filesystem capability.
    Set {
        context: FileInputContext,
        files: Vec<FileData>,
    },
}
