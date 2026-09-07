// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

//! `blueice-mcp-server`'s process entry point: spawn `core`, serve MCP
//! over stdio (the transport local MCP clients like Claude Code/
//! Claude Desktop expect). All the logic worth unit-testing lives in
//! `lib.rs`, driven here through a real subprocess and a real stdio
//! transport that headless CI has no MCP client to exercise the far
//! end of -- excluded from the coverage gate for the same reason
//! `frontend-reference/src/main.rs` is (see `CLAUDE.md`).

use blueice_mcp_server::BlueIceMcpServer;
use rmcp::{ServiceExt, transport::stdio};

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    let server = BlueIceMcpServer::spawn(800, 600)?;
    let service = server.serve(stdio()).await?;
    service.waiting().await?;
    Ok(())
}
