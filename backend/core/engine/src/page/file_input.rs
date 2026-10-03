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
            || !self.native_focusable(id)
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
        let mut current =
            self.file_input_state(context.node_id, source, context.document_generation)?;
        current.context.tab_id = context.tab_id;
        if current.context != *context {
            return Err("File selection is stale".into());
        }
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
