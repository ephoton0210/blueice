// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

//! MCP-side client for the independently granted compiler output socket.

use blueice_ipc::compiler::CompilerProject;
use blueice_ipc::compiler_output::{
    self, CompilerOutputReply, CompilerOutputRequest, CompilerOutputSessionReceipt,
    COMPILER_OUTPUT_PROTOCOL_VERSION,
};
use std::collections::BTreeSet;
use std::io::{self, Read, Write};
use std::os::unix::net::UnixStream;
use std::sync::{Arc, Mutex};

pub struct CompilerOutputConnection<S> {
    stream: S,
    receipt: Option<CompilerOutputSessionReceipt>,
}

impl<S: Read + Write> CompilerOutputConnection<S> {
    pub fn new(stream: S) -> Self {
        Self {
            stream,
            receipt: None,
        }
    }

    pub fn handshake(&mut self) -> io::Result<()> {
        self.receipt = None;
        compiler_output::write_compiler_output_request(
            &mut self.stream,
            &CompilerOutputRequest::Hello {
                protocol_version: COMPILER_OUTPUT_PROTOCOL_VERSION,
            },
        )?;
        match compiler_output::read_compiler_output_reply(&mut self.stream)? {
            CompilerOutputReply::HelloAck {
                protocol_version,
                receipt,
            } if protocol_version == COMPILER_OUTPUT_PROTOCOL_VERSION
                && receipt.is_well_formed() =>
            {
                self.receipt = Some(receipt);
                Ok(())
            }
            CompilerOutputReply::Error { message, .. } => Err(io::Error::other(message)),
            _ => Err(io::Error::new(
                io::ErrorKind::InvalidData,
                "core returned an invalid compiler output handshake",
            )),
        }
    }

    pub fn receipt(&self) -> Option<&CompilerOutputSessionReceipt> {
        self.receipt.as_ref()
    }

    fn request(&mut self, request: CompilerOutputRequest) -> io::Result<CompilerOutputReply> {
        if self.receipt.is_none() {
            return Err(io::Error::new(
                io::ErrorKind::NotConnected,
                "compiler output handshake is required",
            ));
        }
        compiler_output::write_compiler_output_request(&mut self.stream, &request)?;
        compiler_output::read_compiler_output_reply(&mut self.stream)
    }

    pub fn list_projects(&mut self) -> io::Result<CompilerOutputReply> {
        self.request(CompilerOutputRequest::ListProjects {
            receipt: self.receipt.clone().ok_or_else(|| {
                io::Error::new(
                    io::ErrorKind::NotConnected,
                    "compiler output is not connected",
                )
            })?,
        })
    }

    pub fn build(&mut self, project: CompilerProject) -> io::Result<CompilerOutputReply> {
        self.request(CompilerOutputRequest::Build {
            receipt: self.receipt.clone().ok_or_else(|| {
                io::Error::new(
                    io::ErrorKind::NotConnected,
                    "compiler output is not connected",
                )
            })?,
            project,
        })
    }
}

struct OutputMcpSession {
    connection: CompilerOutputConnection<UnixStream>,
    observed_projects: BTreeSet<u64>,
}

#[derive(Clone)]
pub(super) struct OutputMcpAdapter {
    session: Arc<Mutex<OutputMcpSession>>,
    pub(super) receipt: CompilerOutputSessionReceipt,
}

impl OutputMcpAdapter {
    pub(super) fn new(connection: CompilerOutputConnection<UnixStream>) -> io::Result<Self> {
        let receipt = connection.receipt().cloned().ok_or_else(|| {
            io::Error::new(
                io::ErrorKind::NotConnected,
                "compiler output connection has no accepted receipt",
            )
        })?;
        Ok(Self {
            session: Arc::new(Mutex::new(OutputMcpSession {
                connection,
                observed_projects: BTreeSet::new(),
            })),
            receipt,
        })
    }

    pub(super) fn accepts_session(&self, id: Option<&str>) -> bool {
        id == Some(self.receipt.id.as_str())
    }

    pub(super) fn list_projects(&self) -> io::Result<CompilerOutputReply> {
        let mut session = self
            .session
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        session.observed_projects.clear();
        let reply = session.connection.list_projects()?;
        match &reply {
            CompilerOutputReply::Projects(inventory) if inventory.is_well_formed() => {
                session.observed_projects = inventory
                    .projects
                    .iter()
                    .map(|project| project.id)
                    .collect();
                Ok(reply)
            }
            CompilerOutputReply::Projects(_) => Err(io::Error::new(
                io::ErrorKind::InvalidData,
                "core returned a malformed output project inventory",
            )),
            CompilerOutputReply::Error { .. } => Ok(reply),
            _ => Err(io::Error::new(
                io::ErrorKind::InvalidData,
                "core returned a non-inventory output reply",
            )),
        }
    }

    pub(super) fn build(&self, project: CompilerProject) -> io::Result<CompilerOutputReply> {
        let mut session = self
            .session
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        if !session.observed_projects.contains(&project.id) {
            return Ok(CompilerOutputReply::Error {
                code: compiler_output::CompilerOutputErrorCode::UnobservedProject,
                message: "compiler output project was not inventoried on this MCP session".into(),
            });
        }
        let reply = session.connection.build(project)?;
        match &reply {
            CompilerOutputReply::Build(result) if result.is_well_formed_for_project(project) => {
                Ok(reply)
            }
            CompilerOutputReply::Build(_) => Err(io::Error::new(
                io::ErrorKind::InvalidData,
                "core returned a malformed output build result",
            )),
            CompilerOutputReply::Error { .. } => Ok(reply),
            _ => Err(io::Error::new(
                io::ErrorKind::InvalidData,
                "core returned a non-build output reply",
            )),
        }
    }
}
