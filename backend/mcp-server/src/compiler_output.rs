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

#[cfg_attr(test, derive(Debug))]
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

#[cfg_attr(test, derive(Debug))]
struct OutputMcpSession {
    connection: CompilerOutputConnection<UnixStream>,
    observed_projects: BTreeSet<u64>,
}

#[cfg_attr(test, derive(Debug))]
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

#[cfg(test)]
mod tests {
    use super::*;
    use blueice_ipc::compiler::{CompilerGeneration, CompilerProjectInventory};
    use blueice_ipc::compiler_output::{CompilerOutputBuildResult, CompilerOutputErrorCode};
    use std::thread;

    fn receipt() -> CompilerOutputSessionReceipt {
        CompilerOutputSessionReceipt {
            id: format!("ow-{}", "a".repeat(64)),
        }
    }

    /// Serves each queued reply, in order, to one request each, over a real
    /// paired socket so `CompilerOutputConnection`'s own wire I/O runs for
    /// real -- only the far side's *content* is scripted.
    fn fake_server(replies: Vec<CompilerOutputReply>) -> CompilerOutputConnection<UnixStream> {
        let (client, mut server) = UnixStream::pair().unwrap();
        thread::spawn(move || {
            for reply in replies {
                let Ok(_request) = compiler_output::read_compiler_output_request(&mut server)
                else {
                    return;
                };
                if compiler_output::write_compiler_output_reply(&mut server, &reply).is_err() {
                    return;
                }
            }
        });
        CompilerOutputConnection::new(client)
    }

    #[test]
    fn handshake_succeeds_and_stores_the_well_formed_receipt() {
        let mut connection = fake_server(vec![CompilerOutputReply::HelloAck {
            protocol_version: COMPILER_OUTPUT_PROTOCOL_VERSION,
            receipt: receipt(),
        }]);
        connection.handshake().unwrap();
        assert_eq!(connection.receipt(), Some(&receipt()));
    }

    #[test]
    fn handshake_surfaces_an_error_reply_as_an_io_error() {
        let mut connection = fake_server(vec![CompilerOutputReply::Error {
            code: CompilerOutputErrorCode::InvalidReceipt,
            message: "denied".to_string(),
        }]);
        let error = connection.handshake().unwrap_err();
        assert_eq!(error.to_string(), "denied");
        assert!(connection.receipt().is_none());
    }

    #[test]
    fn handshake_rejects_any_other_reply_shape() {
        let mut connection = fake_server(vec![CompilerOutputReply::Projects(
            CompilerProjectInventory {
                projects: Vec::new(),
            },
        )]);
        let error = connection.handshake().unwrap_err();
        assert_eq!(error.kind(), io::ErrorKind::InvalidData);
        assert!(connection.receipt().is_none());
    }

    #[test]
    fn request_before_a_successful_handshake_is_rejected() {
        let mut connection = fake_server(vec![]);
        let error = connection
            .request(CompilerOutputRequest::Hello {
                protocol_version: COMPILER_OUTPUT_PROTOCOL_VERSION,
            })
            .unwrap_err();
        assert_eq!(error.kind(), io::ErrorKind::NotConnected);
    }

    fn handshaken_connection(
        mut later_replies: Vec<CompilerOutputReply>,
    ) -> CompilerOutputConnection<UnixStream> {
        let mut replies = vec![CompilerOutputReply::HelloAck {
            protocol_version: COMPILER_OUTPUT_PROTOCOL_VERSION,
            receipt: receipt(),
        }];
        replies.append(&mut later_replies);
        let mut connection = fake_server(replies);
        connection.handshake().unwrap();
        connection
    }

    fn well_formed_inventory() -> CompilerProjectInventory {
        CompilerProjectInventory {
            projects: vec![CompilerProject { id: 1 }, CompilerProject { id: 2 }],
        }
    }

    fn well_formed_build(project: CompilerProject) -> CompilerOutputBuildResult {
        CompilerOutputBuildResult {
            generation: CompilerGeneration {
                project,
                sequence: 1,
            },
            project_fingerprint: "fingerprint".to_string(),
            has_errors: false,
            published: true,
        }
    }

    fn adapter(mut later_replies: Vec<CompilerOutputReply>) -> OutputMcpAdapter {
        let mut replies = vec![CompilerOutputReply::HelloAck {
            protocol_version: COMPILER_OUTPUT_PROTOCOL_VERSION,
            receipt: receipt(),
        }];
        replies.append(&mut later_replies);
        let mut connection = fake_server(replies);
        connection.handshake().unwrap();
        OutputMcpAdapter::new(connection).unwrap()
    }

    #[test]
    fn output_mcp_adapter_new_requires_an_accepted_receipt() {
        let connection = fake_server(vec![]);
        let error = OutputMcpAdapter::new(connection).unwrap_err();
        assert_eq!(error.kind(), io::ErrorKind::NotConnected);
    }

    #[test]
    fn connection_list_projects_and_build_require_a_prior_handshake() {
        let mut connection = fake_server(vec![]);
        assert_eq!(
            connection.list_projects().unwrap_err().kind(),
            io::ErrorKind::NotConnected
        );
        let mut connection = fake_server(vec![]);
        assert_eq!(
            connection
                .build(CompilerProject { id: 1 })
                .unwrap_err()
                .kind(),
            io::ErrorKind::NotConnected
        );
    }

    #[test]
    fn connection_list_projects_and_build_round_trip_once_handshaken() {
        let mut connection = handshaken_connection(vec![
            CompilerOutputReply::Projects(well_formed_inventory()),
            CompilerOutputReply::Build(well_formed_build(CompilerProject { id: 1 })),
        ]);
        assert!(matches!(
            connection.list_projects().unwrap(),
            CompilerOutputReply::Projects(_)
        ));
        assert!(matches!(
            connection.build(CompilerProject { id: 1 }).unwrap(),
            CompilerOutputReply::Build(_)
        ));
    }

    #[test]
    fn adapter_list_projects_rejects_a_malformed_inventory() {
        let session = adapter(vec![CompilerOutputReply::Projects(
            CompilerProjectInventory {
                projects: vec![CompilerProject { id: 2 }, CompilerProject { id: 1 }],
            },
        )]);
        assert!(session.list_projects().is_err());
    }

    #[test]
    fn adapter_list_projects_forwards_an_error_reply_as_ok() {
        let session = adapter(vec![CompilerOutputReply::Error {
            code: CompilerOutputErrorCode::NotGranted,
            message: "not granted".to_string(),
        }]);
        assert!(matches!(
            session.list_projects().unwrap(),
            CompilerOutputReply::Error { .. }
        ));
    }

    #[test]
    fn adapter_list_projects_rejects_a_non_inventory_reply() {
        let session = adapter(vec![CompilerOutputReply::HelloAck {
            protocol_version: COMPILER_OUTPUT_PROTOCOL_VERSION,
            receipt: receipt(),
        }]);
        assert!(session.list_projects().is_err());
    }

    #[test]
    fn adapter_build_rejects_an_unobserved_project_without_a_round_trip() {
        let session = adapter(vec![]);
        let reply = session.build(CompilerProject { id: 1 }).unwrap();
        assert!(matches!(
            reply,
            CompilerOutputReply::Error {
                code: CompilerOutputErrorCode::UnobservedProject,
                ..
            }
        ));
    }

    #[test]
    fn adapter_build_rejects_a_malformed_result() {
        let session = adapter(vec![
            CompilerOutputReply::Projects(well_formed_inventory()),
            CompilerOutputReply::Build(CompilerOutputBuildResult {
                generation: CompilerGeneration {
                    project: CompilerProject { id: 1 },
                    sequence: 1,
                },
                project_fingerprint: String::new(),
                has_errors: false,
                published: true,
            }),
        ]);
        session.list_projects().unwrap();
        assert!(session.build(CompilerProject { id: 1 }).is_err());
    }

    #[test]
    fn adapter_build_forwards_an_error_reply_as_ok() {
        let session = adapter(vec![
            CompilerOutputReply::Projects(well_formed_inventory()),
            CompilerOutputReply::Error {
                code: CompilerOutputErrorCode::BuildFailed,
                message: "build failed".to_string(),
            },
        ]);
        session.list_projects().unwrap();
        assert!(matches!(
            session.build(CompilerProject { id: 1 }).unwrap(),
            CompilerOutputReply::Error { .. }
        ));
    }

    #[test]
    fn adapter_build_rejects_a_non_build_reply() {
        let session = adapter(vec![
            CompilerOutputReply::Projects(well_formed_inventory()),
            CompilerOutputReply::HelloAck {
                protocol_version: COMPILER_OUTPUT_PROTOCOL_VERSION,
                receipt: receipt(),
            },
        ]);
        session.list_projects().unwrap();
        assert!(session.build(CompilerProject { id: 1 }).is_err());
    }

    #[test]
    fn adapter_build_succeeds_for_an_observed_project() {
        let session = adapter(vec![
            CompilerOutputReply::Projects(well_formed_inventory()),
            CompilerOutputReply::Build(well_formed_build(CompilerProject { id: 1 })),
        ]);
        session.list_projects().unwrap();
        assert!(matches!(
            session.build(CompilerProject { id: 1 }).unwrap(),
            CompilerOutputReply::Build(_)
        ));
    }
}
