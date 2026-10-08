// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

use super::native_interaction::{input_type, tag};
use super::*;
use blueice_ipc::file_input::{
    FileData, FileInputContext, FileInputState, MAX_DOCUMENT_SELECTED_BYTES,
    MAX_DOCUMENT_SELECTED_FILES, MAX_SELECTED_BYTES, MAX_SELECTED_FILES,
};

impl Page {
    pub(crate) fn script_file_value(&self, handle: u64) -> Result<Option<String>, String> {
        let node = self.script_node(handle)?;
        if tag(&self.doc, node) != "input" || input_type(&self.doc, node) != "file" {
            return Ok(None);
        }
        Ok(Some(
            self.native_files
                .get(&node)
                .and_then(|files| files.first())
                .map_or_else(String::new, |file| format!("C:\\fakepath\\{}", file.name)),
        ))
    }
    pub(crate) fn script_clear_input_files(&mut self, handle: u64) -> Result<(), String> {
        let node = self.script_node(handle)?;
        if tag(&self.doc, node) != "input" || input_type(&self.doc, node) != "file" {
            return Err("Not a file input".into());
        }
        self.native_files.remove(&node);
        self.doc.set_file_control_names(node, vec![]);
        self.native_file_revision = self.native_file_revision.wrapping_add(1);
        self.relayout();
        Ok(())
    }
    pub(crate) fn script_reset_form(&mut self, handle: u64) -> Result<(), String> {
        let node = self.script_node(handle)?;
        if tag(&self.doc, node) != "form" {
            return Err("Not a form".into());
        }
        self.reset_native_form(node);
        self.relayout();
        Ok(())
    }
    pub(crate) fn script_document_handle(&mut self) -> u64 {
        self.script_handle_for_node(self.doc.root())
    }
    pub fn validate_file_input(
        &self,
        context: &FileInputContext,
        source: u64,
    ) -> Result<FileInputState, String> {
        let mut current =
            self.file_input_state(context.node_id, source, context.document_generation)?;
        current.context.tab_id = context.tab_id;
        if current.context != *context {
            return Err("File selection is stale".into());
        }
        Ok(current)
    }

    #[cfg(unix)]
    pub(crate) fn script_file_event_path(&mut self, node: u64) -> Option<Vec<u64>> {
        let mut node = NodeId::from_u64(node);
        if !self.doc.contains(node) {
            return None;
        }
        let mut raw = vec![node];
        while let Some(parent) = self.doc.parent(node) {
            if raw.len() >= 256 {
                return None;
            }
            raw.push(parent);
            node = parent;
        }
        Some(
            raw.into_iter()
                .map(|node| self.script_handle_for_node(node))
                .collect(),
        )
    }
    pub(crate) fn script_input_files(
        &self,
        handle: u64,
    ) -> Result<(u64, Option<Vec<blueice_ipc::script::ScriptFileMetadata>>), String> {
        let node = self.script_node(handle)?;
        let files = if tag(&self.doc, node) == "input" && input_type(&self.doc, node) == "file" {
            Some(
                self.native_files
                    .get(&node)
                    .into_iter()
                    .flatten()
                    .map(|file| blueice_ipc::script::ScriptFileMetadata {
                        name: file.name.clone(),
                        media_type: file.media_type.clone(),
                        last_modified: file.last_modified,
                        size: file.bytes.len(),
                    })
                    .collect(),
            )
        } else {
            None
        };
        Ok((self.native_file_revision, files))
    }

    pub(crate) fn script_read_input_file(
        &self,
        handle: u64,
        revision: u64,
        index: usize,
        offset: usize,
        length: usize,
    ) -> Result<Vec<u8>, String> {
        let node = self.script_node(handle)?;
        if revision != self.native_file_revision
            || tag(&self.doc, node) != "input"
            || input_type(&self.doc, node) != "file"
            || length > blueice_ipc::script::SCRIPT_MAX_FILE_CHUNK_BYTES
        {
            return Err("Selected file revision or range is unavailable".into());
        }
        let file = self
            .native_files
            .get(&node)
            .and_then(|files| files.get(index))
            .ok_or("Selected file is unavailable")?;
        let end = offset
            .checked_add(length)
            .filter(|end| *end <= file.bytes.len())
            .ok_or("Selected file range is unavailable")?;
        Ok(file.bytes[offset..end].to_vec())
    }

    pub fn file_input_state(
        &self,
        node: u64,
        source: u64,
        document: u64,
    ) -> Result<FileInputState, String> {
        let id = NodeId::from_u64(node);
        if document != self.document_generation
            || !self.doc.contains(id)
            || tag(&self.doc, id) != "input"
            || input_type(&self.doc, id) != "file"
            || !self.native_activation_available(id)
        {
            return Err("File control is stale or unavailable".into());
        }
        let accept = element_attribute(&self.doc, id, "accept").unwrap_or("");
        if accept.len() > 4096 {
            return Err("File type hints exceed the supported limit".into());
        }
        Ok(FileInputState {
            context: FileInputContext {
                tab_id: 0, // The session attaches the actual tab owner.
                frame_source: source,
                document_generation: document,
                node_id: node,
                revision: self.native_file_revision,
            },
            multiple: element_attribute(&self.doc, id, "multiple").is_some(),
            accept: accept.into(),
            names: self.doc.file_control_names(id).to_vec(),
        })
    }
    pub fn set_file_input(
        &mut self,
        context: &FileInputContext,
        source: u64,
        files: Vec<FileData>,
    ) -> Result<FileInputState, String> {
        let current = self.validate_file_input(context, source)?;
        let id = NodeId::from_u64(context.node_id);
        let total = files
            .iter()
            .fold(0usize, |n, f| n.saturating_add(f.bytes.len()));
        let retained = self
            .native_files
            .iter()
            .filter(|(node, _)| **node != id)
            .flat_map(|(_, files)| files)
            .fold(0usize, |n, f| n.saturating_add(f.bytes.len()));
        let retained_count = self
            .native_files
            .iter()
            .filter(|(node, _)| **node != id)
            .map(|(_, files)| files.len())
            .sum::<usize>();
        if files.len() > MAX_SELECTED_FILES
            || !current.multiple && files.len() > 1
            || files.iter().any(|f| !f.valid())
            || total > MAX_SELECTED_BYTES
            || retained.saturating_add(total) > MAX_DOCUMENT_SELECTED_BYTES
            || retained_count.saturating_add(files.len()) > MAX_DOCUMENT_SELECTED_FILES
        {
            return Err("Selected files exceed supported names, count or content limits".into());
        }
        self.doc
            .set_file_control_names(id, files.iter().map(|f| f.name.clone()).collect());
        if files.is_empty() {
            self.native_files.remove(&id);
        } else {
            self.native_files.insert(id, files);
        }
        self.native_file_revision = self.native_file_revision.wrapping_add(1);
        self.relayout();
        self.file_input_state(context.node_id, source, context.document_generation)
    }
    pub(super) fn prune_native_files(&mut self) {
        let removed: Vec<_> = self
            .native_files
            .keys()
            .filter(|id| {
                !self.doc.contains(**id)
                    || tag(&self.doc, **id) != "input"
                    || input_type(&self.doc, **id) != "file"
            })
            .copied()
            .collect();
        if !removed.is_empty() {
            self.native_file_revision = self.native_file_revision.wrapping_add(1);
        }
        for id in removed {
            self.native_files.remove(&id);
            self.doc.set_file_control_names(id, vec![]);
        }
    }
}
