// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

//! Additive browser-context organization and lifetime. Runtime IDs are neither
//! durable profile keys nor grants; all page operations retain normal policy.
use crate::{ClientMessage, TabGroupSummary};
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub enum ContextAction {
    List,
    Create {
        name: String,
    },
    Rename {
        context_id: u64,
        name: String,
    },
    Close {
        context_id: u64,
    },
    Command {
        context_id: u64,
        message: Box<ClientMessage>,
    },
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ContextSummary {
    pub id: u64,
    pub name: String,
    pub windows: Vec<u64>,
    pub groups: Vec<TabGroupSummary>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub enum ContextEvent {
    Snapshot,
    Created { context_id: u64 },
    Renamed { context_id: u64 },
    Closed { context_id: u64 },
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ContextState {
    pub contexts: Vec<ContextSummary>,
    pub event: ContextEvent,
}
