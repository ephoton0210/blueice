// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

//! Core-owned window membership and display environments. Moving a tab
//! transfers its existing page/history instead of loading a second document.
use super::*;
use blueice_ipc::viewport::DisplayViewport;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct WindowId(u64);
impl WindowId {
    pub fn from_u64(id: u64) -> Self {
        Self(id)
    }
    pub fn as_u64(self) -> u64 {
        self.0
    }
}

pub(super) struct CoreWindow {
    pub(super) context_id: BrowserContextId,
    pub(super) viewport: DisplayViewport,
    pub(super) native: bool,
    pub(super) tabs: Vec<TabId>,
}

impl TabManager {
    pub fn window_ids(&self) -> impl Iterator<Item = WindowId> + '_ {
        self.windows.keys().copied()
    }
    pub fn window_tabs(&self, id: WindowId) -> impl Iterator<Item = TabId> + '_ {
        self.windows
            .get(&id)
            .into_iter()
            .flat_map(|w| w.tabs.iter().copied())
    }
    pub fn window_viewport(&self, id: WindowId) -> Option<DisplayViewport> {
        self.windows.get(&id).map(|w| w.viewport)
    }
    pub fn tab_window(&self, id: TabId) -> Option<WindowId> {
        self.tabs.get(&id).map(|t| t.window_id)
    }
    pub fn create_window(&mut self, viewport: DisplayViewport) -> Result<WindowId, String> {
        self.create_window_in_context(BrowserContextId::from_u64(1), viewport)
    }
    pub fn create_window_in_context(
        &mut self,
        context_id: BrowserContextId,
        viewport: DisplayViewport,
    ) -> Result<WindowId, String> {
        if self.context(context_id).is_none() {
            return Err("Unknown browser context".into());
        }
        viewport.validate()?;
        if self.windows.len() >= 64 {
            return Err("Browser window limit reached".into());
        }
        let id = WindowId(self.next_window_id);
        let next = self
            .next_window_id
            .checked_add(1)
            .ok_or("Window identities exhausted")?;
        self.windows.insert(
            id,
            CoreWindow {
                context_id,
                viewport,
                native: true,
                tabs: Vec::new(),
            },
        );
        self.next_window_id = next;
        Ok(id)
    }
    pub fn configure_window_viewport(
        &mut self,
        id: WindowId,
        viewport: DisplayViewport,
    ) -> Result<(), String> {
        viewport.validate()?;
        let window = self.windows.get_mut(&id).ok_or("Unknown browser window")?;
        window.viewport = viewport;
        window.native = true;
        for tab in self.tabs.values_mut().filter(|t| t.window_id == id) {
            tab.page.configure_display(viewport);
            for entry in tab.back.iter_mut().chain(tab.forward.iter_mut()) {
                if let Some(page) = entry.snapshot_mut() {
                    page.configure_display(viewport);
                }
            }
        }
        Ok(())
    }
    pub(crate) fn resize_window(&mut self, id: WindowId, width: f64, height: f64) {
        let Some(window) = self.windows.get_mut(&id) else {
            return;
        };
        if window.native {
            let _ = self.configure_window_viewport(
                id,
                DisplayViewport {
                    width,
                    height,
                    device_scale: 1.0,
                    backing_scale: None,
                },
            );
            return;
        }
        window.viewport.width = width;
        window.viewport.height = height;
        for tab in self.tabs.values_mut().filter(|t| t.window_id == id) {
            tab.page.resize(width, height);
            for entry in tab.back.iter_mut().chain(tab.forward.iter_mut()) {
                if let Some(page) = entry.snapshot_mut() {
                    page.resize(width, height);
                }
            }
        }
    }
    pub fn move_tab_to_window(&mut self, id: TabId, destination: WindowId) -> Result<(), String> {
        let viewport = self
            .window_viewport(destination)
            .ok_or("Unknown browser window")?;
        let source = self.tab_window(id).ok_or("Unknown tab")?;
        if self.window_context(source) != self.window_context(destination) {
            return Err("A tab cannot move to a different browser context".into());
        }
        if source == destination {
            return Ok(());
        }
        self.windows
            .get_mut(&source)
            .expect("live source window")
            .tabs
            .retain(|&t| t != id);
        self.windows
            .get_mut(&destination)
            .expect("validated destination")
            .tabs
            .push(id);
        let tab = self.tabs.get_mut(&id).expect("validated tab");
        tab.window_id = destination;
        tab.page.transfer_native_editor();
        tab.page.configure_display(viewport);
        for entry in tab.back.iter_mut().chain(tab.forward.iter_mut()) {
            if let Some(page) = entry.snapshot_mut() {
                page.configure_display(viewport);
            }
        }
        Ok(())
    }
    pub fn close_window(&mut self, id: WindowId) -> Result<Vec<TabId>, String> {
        let window = self.windows.remove(&id).ok_or("Unknown browser window")?;
        for tab in &window.tabs {
            self.tabs.remove(tab);
        }
        self.order.retain(|tab| !window.tabs.contains(tab));
        Ok(window.tabs)
    }
    pub(crate) fn enable_native_windows(&mut self) {
        self.native_windows = true;
    }
    pub(crate) fn native_windows_enabled(&self) -> bool {
        self.native_windows
    }
}
