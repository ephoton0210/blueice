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
use rmcp::{transport::stdio, ServiceExt};
use std::path::PathBuf;

/// `(rendezvous socket, control socket)`, each optional.
fn parse_args(
    args: impl Iterator<Item = String>,
) -> Result<(Option<PathBuf>, Option<PathBuf>), String> {
    let mut launcher_socket = None;
    let mut control_socket = None;
    let mut args = args;
    while let Some(flag) = args.next() {
        match flag.as_str() {
            "--launcher-socket" => {
                let value = args
                    .next()
                    .ok_or_else(|| "--launcher-socket requires a path".to_string())?;
                if launcher_socket.replace(PathBuf::from(value)).is_some() {
                    return Err("--launcher-socket may be supplied only once".to_string());
                }
            }
            "--launcher-control-socket" => {
                let value = args
                    .next()
                    .ok_or_else(|| "--launcher-control-socket requires a path".to_string())?;
                if control_socket.replace(PathBuf::from(value)).is_some() {
                    return Err("--launcher-control-socket may be supplied only once".to_string());
                }
            }
            "--help" | "-h" => {
                return Err(
                    "usage: blueice-mcp-server [--launcher-socket <rendezvous.sock>] [--launcher-control-socket <control.sock>]".to_string(),
                );
            }
            _ => return Err(format!("unknown argument {flag:?}")),
        }
    }
    Ok((launcher_socket, control_socket))
}

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    let (launcher_socket, control_socket) = parse_args(std::env::args().skip(1))?;
    let server = match launcher_socket {
        Some(socket) => BlueIceMcpServer::attach_to_launcher(socket, 800, 600),
        None => BlueIceMcpServer::spawn(800, 600),
    };
    let server = match control_socket {
        Some(socket) => server.with_control_socket(socket),
        None => server,
    };
    let service = server.serve(stdio()).await?;
    service.waiting().await?;
    Ok(())
}
