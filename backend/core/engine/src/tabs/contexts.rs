// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

//! Browser-context lifetime and organization. Context IDs fence membership;
//! they do not authorize a controller, navigation, or a human permission.
use super::*;
use unicode_normalization::UnicodeNormalization;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct BrowserContextId(u64);
impl BrowserContextId {
    pub fn from_u64(id: u64) -> Self {
        Self(id)
    }
    pub fn as_u64(self) -> u64 {
        self.0
    }
}

pub struct BrowserContext {
    pub(super) id: BrowserContextId,
    pub(super) name: String,
}
impl BrowserContext {
    pub fn id(&self) -> BrowserContextId {
        self.id
    }
    pub fn name(&self) -> &str {
        &self.name
    }
}

/// Runtime identities invalidated by an explicit context removal.
pub struct ClosedBrowserContext {
    pub windows: Vec<WindowId>,
    pub tabs: Vec<TabId>,
    pub groups: Vec<GroupId>,
}

impl TabManager {
    pub fn context(&self, id: BrowserContextId) -> Option<&BrowserContext> {
        self.contexts.get(&id)
    }
    pub fn contexts(&self) -> impl Iterator<Item = &BrowserContext> {
        self.contexts.values()
    }
    pub fn context_windows(&self, id: BrowserContextId) -> impl Iterator<Item = WindowId> + '_ {
        self.windows
            .iter()
            .filter_map(move |(&window, state)| (state.context_id == id).then_some(window))
    }
    pub fn context_groups(&self, id: BrowserContextId) -> impl Iterator<Item = &TabGroup> {
        self.groups().filter(move |group| group.context_id == id)
    }
    pub fn window_context(&self, id: WindowId) -> Option<BrowserContextId> {
        self.windows.get(&id).map(|window| window.context_id)
    }
    pub fn tab_context(&self, id: TabId) -> Option<BrowserContextId> {
        self.tab_window(id)
            .and_then(|window| self.window_context(window))
    }
    fn validate_context_name(
        &self,
        name: String,
        excluding: Option<BrowserContextId>,
    ) -> Result<String, String> {
        let name: String = name.trim().nfc().collect();
        if name.is_empty() || name.len() > 256 || name.chars().any(char::is_control) {
            return Err(
                "A context name must contain 1–256 UTF-8 bytes without control characters".into(),
            );
        }
        let folded = name.to_lowercase();
        if self
            .contexts
            .values()
            .any(|context| Some(context.id) != excluding && context.name.to_lowercase() == folded)
        {
            return Err("A browser context with this name already exists".into());
        }
        Ok(name)
    }
    pub fn create_context(&mut self, name: String) -> Result<BrowserContextId, String> {
        let name = self.validate_context_name(name, None)?;
        if self.contexts.len() >= 16 {
            return Err("Browser context limit reached".into());
        }
        let id = BrowserContextId(self.next_context_id);
        let next = self
            .next_context_id
            .checked_add(1)
            .ok_or("Context identities exhausted")?;
        self.contexts.insert(id, BrowserContext { id, name });
        self.next_context_id = next;
        Ok(id)
    }
    pub fn rename_context(&mut self, id: BrowserContextId, name: String) -> Result<(), String> {
        if !self.contexts.contains_key(&id) {
            return Err("Unknown browser context".into());
        }
        let name = self.validate_context_name(name, Some(id))?;
        self.contexts.get_mut(&id).expect("validated context").name = name;
        Ok(())
    }
    pub fn close_context(&mut self, id: BrowserContextId) -> Result<ClosedBrowserContext, String> {
        if id == BrowserContextId(1) {
            return Err("The default browser context cannot be removed".into());
        }
        if !self.contexts.contains_key(&id) {
            return Err("Unknown browser context".into());
        }
        let windows: Vec<_> = self.context_windows(id).collect();
        let groups: Vec<_> = self.context_groups(id).map(TabGroup::id).collect();
        let mut tabs = Vec::new();
        for &window in &windows {
            tabs.extend(self.close_window(window).expect("live context window"));
        }
        for &group in &groups {
            self.close_group(group);
        }
        self.contexts.remove(&id);
        Ok(ClosedBrowserContext {
            windows,
            tabs,
            groups,
        })
    }
    pub fn create_group_in_context(
        &mut self,
        context_id: BrowserContextId,
        name: String,
        color: String,
    ) -> Result<GroupId, String> {
        if !self.contexts.contains_key(&context_id) {
            return Err("Unknown browser context".into());
        }
        let id = GroupId(self.next_group_id);
        let next = self
            .next_group_id
            .checked_add(1)
            .ok_or("Group identities exhausted")?;
        self.groups.insert(
            id,
            TabGroup {
                id,
                name,
                color,
                collapsed: false,
                context_id,
            },
        );
        self.group_order.push(id);
        self.next_group_id = next;
        Ok(id)
    }
    pub fn assign_tab_group(&mut self, tab: TabId, group: Option<GroupId>) -> Result<(), String> {
        let context = self.tab_context(tab).ok_or("Unknown tab")?;
        if let Some(group) = group {
            let target = self.group(group).ok_or("Unknown tab group")?;
            if target.context_id != context {
                return Err("A tab group belongs to a different browser context".into());
            }
        }
        self.tabs.get_mut(&tab).expect("validated tab").group_id = group;
        Ok(())
    }
    pub(crate) fn enable_native_contexts(&mut self) {
        self.native_contexts = true;
    }
    pub(crate) fn native_contexts_enabled(&self) -> bool {
        self.native_contexts
    }
}
