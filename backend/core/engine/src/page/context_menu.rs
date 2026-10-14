// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

use super::*;
use blueice_ipc::context_menu::{ContextMenuContext, ContextMenuState};

impl Page {
    pub(crate) fn validate_menu_point(&self, x: f64, y: f64) -> bool {
        x.is_finite()
            && y.is_finite()
            && x >= 0.0
            && y >= 0.0
            && x < self.viewport_width
            && y < self.viewport_height
    }

    fn menu_target(&self, x: f64, y: f64) -> Option<NodeId> {
        if !self.validate_menu_point(x, y) {
            return None;
        }
        let target = self.click_target(x, y)?;
        if self
            .event_element_target(target)
            .is_some_and(|id| self.native_control_disabled(id))
        {
            return None;
        }
        let mut cursor = Some(target);
        while let Some(id) = cursor {
            if element_attribute(&self.doc, id, "hidden").is_some()
                || element_attribute(&self.doc, id, "inert").is_some()
                || self
                    .styles
                    .get(&id)
                    .is_some_and(|s| s.display == "none" || s.opacity() == 0.0)
            {
                return None;
            }
            cursor = self.doc.parent(id);
        }
        Some(target)
    }

    pub(crate) fn menu_link(&self, x: f64, y: f64) -> Option<String> {
        let target = self.menu_target(x, y)?;
        let href = self.native_activation_link(target)?;
        let url = Url::parse(&href).ok()?;
        (matches!(url.scheme(), "http" | "https" | "about") && href.len() <= 8192).then_some(href)
    }

    pub(crate) fn prepare_context_menu(&mut self, x: f64, y: f64) -> bool {
        let target = self.menu_target(x, y);
        let editor = self
            .native_pointer_focus(target)
            .filter(|id| native_editing::supports_native_selection(&self.doc, *id));
        // Right-clicking a link/button/blank area never executes its default
        // action or changes an unrelated editor's selection/composition.
        editor.is_some() && self.focus_native_editor_at(editor)
    }

    pub(crate) fn context_menu_state(
        &self,
        source: u64,
        tab: u64,
        x: f64,
        y: f64,
    ) -> ContextMenuState {
        let editor = self
            .native_pointer_focus(self.menu_target(x, y))
            .filter(|id| native_editing::supports_native_selection(&self.doc, *id));
        let input = editor.filter(|id| Some(*id) == self.focused).and_then(|_| {
            let mut state = self.native_text_input_state(source);
            state.tab_id = tab;
            state.focused.as_ref()?;
            Some(state)
        });
        let document = (input.is_none()
            && self.validate_menu_point(x, y)
            && self
                .click_target(x, y)
                .is_none_or(|node| self.document_text_is_public(node)))
        .then(|| self.document_selection_state())
        .filter(|state| state.text_length > 0)
        .map(Box::new);
        ContextMenuState {
            context: ContextMenuContext {
                tab_id: tab,
                frame_source: source,
                document_generation: self.document_generation,
                frame_generation: self.frame_generation,
                x,
                y,
            },
            link_url: self.menu_link(x, y),
            input,
            document,
        }
    }
}
