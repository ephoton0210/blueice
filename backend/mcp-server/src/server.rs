// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

//! The MCP-facing tool definitions -- thin wrappers over
//! [`crate::CoreConnection`]'s methods, bridged into `rmcp`'s async
//! `ServerHandler` world via `tokio::task::spawn_blocking` (the
//! connection itself does synchronous, blocking socket I/O, matching
//! `blueice_engine::session`'s own style rather than introducing an
//! async rewrite of `blueice-ipc` for this one caller).
//!
//! Tool outputs are plain JSON text content (`serde_json::to_string`),
//! not `rmcp`'s `Json<T>` structured-output wrapper -- that wrapper
//! requires `T: schemars::JsonSchema`, which would mean adding
//! `schemars` as a dependency of `blueice-ipc` (a core protocol crate)
//! just to satisfy one MCP-specific caller. Plain text content needs
//! only `Serialize`, which `blueice-ipc`'s wire types already derive.

use crate::{CoreConnection, CoreProcess};
use base64::Engine;
use blueice_ipc::NodeAction;
use rmcp::handler::server::wrapper::Parameters;
use rmcp::model::{CallToolResult, ContentBlock as Content, Implementation, ServerCapabilities, ServerInfo};
use rmcp::schemars;
use rmcp::{ErrorData, ServerHandler, tool, tool_handler, tool_router};
use serde::Deserialize;
use std::io;
use std::os::unix::net::UnixStream;
use std::sync::{Arc, Mutex};

#[derive(Deserialize, schemars::JsonSchema)]
struct NavigateParams {
    /// The URL to load, replacing the current page.
    url: String,
}

#[derive(Deserialize, schemars::JsonSchema)]
struct NodeIdParams {
    /// A stable node ID from a prior `get_page_representation` call.
    node_id: u64,
}

#[derive(Deserialize, schemars::JsonSchema)]
struct TypeTextParams {
    /// A stable node ID from a prior `get_page_representation` call.
    node_id: u64,
    /// The text to set as the element's value.
    text: String,
}

#[derive(Deserialize, schemars::JsonSchema)]
struct HighlightParams {
    /// The node to highlight, or omit/`null` to clear any current highlight.
    node_id: Option<u64>,
}

async fn blocking<T, F>(conn: Arc<Mutex<CoreConnection<UnixStream>>>, f: F) -> Result<T, ErrorData>
where
    F: FnOnce(&mut CoreConnection<UnixStream>) -> io::Result<T> + Send + 'static,
    T: Send + 'static,
{
    tokio::task::spawn_blocking(move || {
        let mut guard = conn.lock().unwrap_or_else(|poisoned| poisoned.into_inner());
        f(&mut guard)
    })
    .await
    .map_err(|e| ErrorData::internal_error(format!("mcp-server task join error: {e}"), None))?
    .map_err(|e| ErrorData::internal_error(format!("blueice-core IPC error: {e}"), None))
}

fn outcome_to_result(outcome: crate::ToolOutcome) -> CallToolResult {
    let json = serde_json::json!({ "error": outcome.error, "snapshot": outcome.snapshot });
    let text = serde_json::to_string_pretty(&json).unwrap_or_else(|_| "{}".to_string());
    if outcome.error.is_some() {
        CallToolResult::error(vec![Content::text(text)])
    } else {
        CallToolResult::success(vec![Content::text(text)])
    }
}

/// The MCP server itself -- owns its `core` connection for its whole
/// lifetime. If a `blueice-launcher` rendezvous socket is reachable
/// (see [`CoreProcess::connect`]), that shared `core`/`Page` is left
/// running when the MCP client disconnects; otherwise (no launcher
/// running) this privately spawned its own `core`, which *is* torn
/// down with it.
pub struct BlueIceMcpServer {
    core: CoreProcess,
}

impl BlueIceMcpServer {
    pub fn spawn(width: u32, height: u32) -> io::Result<Self> {
        Ok(BlueIceMcpServer { core: CoreProcess::connect(width, height)? })
    }

    fn conn(&self) -> Arc<Mutex<CoreConnection<UnixStream>>> {
        self.core.conn.clone()
    }
}

#[tool_router]
impl BlueIceMcpServer {
    #[tool(description = "Navigate to a URL and return the resulting page representation (an accessibility-tree-shaped snapshot, per phase-1-ai-representation-layer/PLAN.md)")]
    async fn navigate(&self, Parameters(NavigateParams { url }): Parameters<NavigateParams>) -> Result<CallToolResult, ErrorData> {
        let outcome = blocking(self.conn(), move |conn| conn.navigate(&url)).await?;
        Ok(outcome_to_result(outcome))
    }

    #[tool(description = "Get the current page's representation without performing any action")]
    async fn get_page_representation(&self) -> Result<CallToolResult, ErrorData> {
        let snapshot = blocking(self.conn(), |conn| conn.representation()).await?;
        let text = serde_json::to_string_pretty(&snapshot).unwrap_or_else(|_| "{}".to_string());
        Ok(CallToolResult::success(vec![Content::text(text)]))
    }

    #[tool(
        description = "Get the full DOM tree as a canonical text dump, unfiltered by the AI representation's semantic-role/display:none exclusion -- useful for structural comparison against another browser's DOM"
    )]
    async fn get_dom(&self) -> Result<CallToolResult, ErrorData> {
        let dump = blocking(self.conn(), |conn| conn.dom()).await?;
        Ok(CallToolResult::success(vec![Content::text(dump)]))
    }

    #[tool(description = "Click the element with this node ID (follows a link's href if it is or is inside one, same as a human click)")]
    async fn click(&self, Parameters(NodeIdParams { node_id }): Parameters<NodeIdParams>) -> Result<CallToolResult, ErrorData> {
        let outcome = blocking(self.conn(), move |conn| conn.act(node_id, NodeAction::Click)).await?;
        Ok(outcome_to_result(outcome))
    }

    #[tool(description = "Set the value of an input/textarea/select element identified by node ID")]
    async fn type_text(&self, Parameters(TypeTextParams { node_id, text }): Parameters<TypeTextParams>) -> Result<CallToolResult, ErrorData> {
        let outcome = blocking(self.conn(), move |conn| conn.act(node_id, NodeAction::SetValue(text))).await?;
        Ok(outcome_to_result(outcome))
    }

    #[tool(description = "Move keyboard focus to the element with this node ID")]
    async fn focus(&self, Parameters(NodeIdParams { node_id }): Parameters<NodeIdParams>) -> Result<CallToolResult, ErrorData> {
        let outcome = blocking(self.conn(), move |conn| conn.act(node_id, NodeAction::Focus)).await?;
        Ok(outcome_to_result(outcome))
    }

    #[tool(description = "Scroll the page so the element with this node ID is aligned to the top of the viewport")]
    async fn scroll_into_view(&self, Parameters(NodeIdParams { node_id }): Parameters<NodeIdParams>) -> Result<CallToolResult, ErrorData> {
        let outcome = blocking(self.conn(), move |conn| conn.act(node_id, NodeAction::ScrollIntoView)).await?;
        Ok(outcome_to_result(outcome))
    }

    #[tool(description = "Highlight an element for the human-visible window (an outline drawn around its current bounds), or clear the highlight by omitting node_id")]
    async fn highlight(&self, Parameters(HighlightParams { node_id }): Parameters<HighlightParams>) -> Result<CallToolResult, ErrorData> {
        let outcome = blocking(self.conn(), move |conn| conn.highlight(node_id)).await?;
        Ok(outcome_to_result(outcome))
    }

    #[tool(description = "Take a PNG screenshot of the most recently rendered frame (call navigate first; there is nothing to screenshot before that)")]
    async fn screenshot(&self) -> Result<CallToolResult, ErrorData> {
        let png = blocking(self.conn(), |conn| {
            let Some(frame) = conn.last_frame().cloned() else { return Ok(None) };
            let mapped = blueice_ipc::shm::map_frame(std::path::Path::new(&frame.shm_path))?;
            Ok(Some(crate::frame_to_png_bytes(&mapped, frame.width, frame.height)?))
        })
        .await?;

        match png {
            Some(bytes) => {
                let b64 = base64::engine::general_purpose::STANDARD.encode(bytes);
                Ok(CallToolResult::success(vec![Content::image(b64, "image/png")]))
            }
            None => Ok(CallToolResult::error(vec![Content::text("no frame has been rendered yet -- call navigate first")])),
        }
    }
}

#[tool_handler]
impl ServerHandler for BlueIceMcpServer {
    fn get_info(&self) -> ServerInfo {
        ServerInfo::new(ServerCapabilities::builder().enable_tools().build())
            .with_server_info(Implementation::new("blueice", env!("CARGO_PKG_VERSION")))
            .with_instructions(
                "Drive the BlueIce browser engine: navigate to pages, read the accessibility-tree-shaped \
                 representation, and act on elements by their stable node ID (click/type/focus/scroll-into-view). \
                 This is BlueIce's own render pass, not a driven Chromium instance -- what these tools report is \
                 exactly what a human would see in the reference frontend at the same moment.",
            )
    }
}
