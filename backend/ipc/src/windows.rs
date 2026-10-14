// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

//! Additive native-window registry. Canonical snapshots contain window-local
//! ordered tabs; the ordinary tab and navigation protocol retains its shape.
use crate::{viewport::DisplayViewport, TabSummary};
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub enum WindowAction {
    List,
    /// A native tab command is fenced to its originating window. The ID is a
    /// lifecycle check, not a human gesture or permission grant.
    Command {
        window_id: u64,
        message: Box<crate::ClientMessage>,
    },
    Create {
        viewport: DisplayViewport,
    },
    CreateInContext {
        context_id: u64,
        viewport: DisplayViewport,
    },
    Resize {
        window_id: u64,
        viewport: DisplayViewport,
    },
    Close {
        window_id: u64,
    },
    MoveTab {
        window_id: u64,
    },
    /// Atomic native placement. The source window fences a stale drag; neither
    /// a tab identity nor this command authorizes document or OS access.
    PlaceTab {
        source_window_id: u64,
        window_id: u64,
        before_tab_id: Option<u64>,
        group_id: Option<u64>,
    },
    OpenTab {
        window_id: u64,
        url: Option<String>,
    },
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct WindowSummary {
    pub id: u64,
    pub viewport: DisplayViewport,
    pub tabs: Vec<TabSummary>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub enum WindowEvent {
    Snapshot,
    Created {
        window_id: u64,
    },
    Resized {
        window_id: u64,
    },
    Closed {
        window_id: u64,
    },
    TabMoved {
        tab_id: u64,
        from_window: u64,
        to_window: u64,
    },
    TabPlaced {
        tab_id: u64,
        from_window: u64,
        to_window: u64,
    },
    TabOpened {
        tab_id: u64,
        window_id: u64,
    },
    TabClosed {
        tab_id: u64,
        window_id: u64,
    },
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct WindowState {
    pub windows: Vec<WindowSummary>,
    pub event: WindowEvent,
    /// Older cores omit this flag. Frontends must not send PlaceTab until it
    /// is advertised by the owned native-window registry.
    #[serde(default)]
    pub tab_placement_v1: bool,
}
