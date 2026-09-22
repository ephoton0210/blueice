// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

//! MCP-side client for the core-owned registered-project compiler protocol.
//!
//! This module deliberately knows only the query-only IPC vocabulary. It
//! receives opaque handles after a core owner selected and registered a closed
//! project; no connection method can create a registration, pass a path or
//! source graph, configure a resolver/plugin/compiler option, read source, or
//! request an artifact/output write.

use std::io::{self, Read, Write};

/// A synchronous client for the separately-versioned, query-only
/// registered-project compiler channel. It is deliberately distinct from the
/// browser-control connection: compiler queries neither navigate a page nor
/// share the browser protocol version.
pub struct CompilerConnection<S> {
    stream: S,
}

impl<S: Read + Write> CompilerConnection<S> {
    pub fn new(stream: S) -> Self {
        Self { stream }
    }

    /// Negotiates the compiler protocol before any query. A peer's structured
    /// rejection remains an I/O failure to this client; query-level errors
    /// from later replies remain structured compiler replies so the MCP layer
    /// can report them without substituting another action.
    pub fn handshake(&mut self) -> io::Result<()> {
        use blueice_ipc::compiler::{
            read_compiler_reply, write_compiler_request, CompilerReply, CompilerRequest,
            COMPILER_PROTOCOL_VERSION,
        };

        write_compiler_request(
            &mut self.stream,
            &CompilerRequest::Hello {
                protocol_version: COMPILER_PROTOCOL_VERSION,
            },
        )?;
        match read_compiler_reply(&mut self.stream)? {
            CompilerReply::HelloAck { protocol_version }
                if protocol_version == COMPILER_PROTOCOL_VERSION =>
            {
                Ok(())
            }
            CompilerReply::Error { message, .. } => Err(io::Error::other(message)),
            reply => Err(io::Error::other(format!(
                "expected compiler Hello handshake reply, got {reply:?}"
            ))),
        }
    }

    /// Returns the source-text-free description selected by the core for an
    /// already registered project.
    pub fn describe_project(
        &mut self,
        project_id: u64,
    ) -> io::Result<blueice_ipc::compiler::CompilerReply> {
        self.request(blueice_ipc::compiler::CompilerRequest::DescribeProject {
            project: blueice_ipc::compiler::CompilerProject { id: project_id },
        })
    }

    /// Runs a read-only check for one core-registered project. The reply is
    /// source-text-free and may carry explicit work-set/diagnostic truncation;
    /// it never contains emitted artifacts or writes output.
    pub fn check(&mut self, project_id: u64) -> io::Result<blueice_ipc::compiler::CompilerReply> {
        self.request(blueice_ipc::compiler::CompilerRequest::Check {
            project: blueice_ipc::compiler::CompilerProject { id: project_id },
        })
    }

    /// Looks up one static type in exactly one retained compiler generation.
    /// Stale and unknown handles remain structured replies from the core; this
    /// method never guesses a replacement generation.
    pub fn static_type(
        &mut self,
        project_id: u64,
        generation: u64,
        type_id: u32,
    ) -> io::Result<blueice_ipc::compiler::CompilerReply> {
        self.request(blueice_ipc::compiler::CompilerRequest::GetStaticType {
            generation: compiler_generation(project_id, generation),
            type_id,
        })
    }

    /// Looks up one static symbol in exactly one retained compiler generation.
    /// It is static compiler metadata, not a BlueJS value or page-object
    /// inspection request.
    pub fn static_symbol(
        &mut self,
        project_id: u64,
        generation: u64,
        symbol_id: u32,
    ) -> io::Result<blueice_ipc::compiler::CompilerReply> {
        self.request(blueice_ipc::compiler::CompilerRequest::GetStaticSymbol {
            generation: compiler_generation(project_id, generation),
            symbol_id,
        })
    }

    fn request(
        &mut self,
        request: blueice_ipc::compiler::CompilerRequest,
    ) -> io::Result<blueice_ipc::compiler::CompilerReply> {
        blueice_ipc::compiler::write_compiler_request(&mut self.stream, &request)?;
        blueice_ipc::compiler::read_compiler_reply(&mut self.stream)
    }
}

fn compiler_generation(
    project_id: u64,
    sequence: u64,
) -> blueice_ipc::compiler::CompilerGeneration {
    blueice_ipc::compiler::CompilerGeneration {
        project: blueice_ipc::compiler::CompilerProject { id: project_id },
        sequence,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use blueice_bluets::{
        AuthorizedModule, AuthorizedModuleLoader, CompilerOptions, RuntimePolicy,
    };
    use blueice_engine::{
        compiler_ipc::{compiler_service_ipc_request_channel, CoreCompilerProjectCatalog},
        compiler_service::RegisteredProjectRegistration,
    };
    use std::os::unix::net::UnixStream;
    use std::thread;

    fn compiler_registration() -> RegisteredProjectRegistration {
        const ENTRY: &str = "project:///mcp/main.ts";
        RegisteredProjectRegistration {
            canonical_project_root: "project:///mcp".to_string(),
            canonical_config_root: "project:///mcp/blue-ts.json".to_string(),
            canonical_output_root: "project:///mcp-dist".to_string(),
            entry_module: ENTRY.to_string(),
            loader: AuthorizedModuleLoader::new(
                [AuthorizedModule::new(
                    ENTRY,
                    "export const checked: number = 42;",
                )],
                [],
            )
            .unwrap(),
            compiler_options: CompilerOptions {
                resolver_fingerprint: "mcp-adapter-fixture-v1".to_string(),
                runtime_policy: RuntimePolicy::Checked,
                ..CompilerOptions::default()
            },
        }
    }

    #[test]
    fn compiler_connection_checks_a_sealed_core_catalog_through_the_session_owner() {
        // The MCP client gets only the owner-minted opaque project ID. The
        // registration happens before `seal`, and the core session later owns
        // the mutable adapter/cache while a listener worker has framing only.
        let mut catalog = CoreCompilerProjectCatalog::default();
        let project = catalog
            .register_startup_project(compiler_registration())
            .unwrap();
        let mut core_session = catalog.seal();
        let (request_sender, request_receiver) = compiler_service_ipc_request_channel();
        let (client, mut server) = UnixStream::pair().unwrap();
        let listener = thread::spawn(move || {
            let hello = blueice_ipc::compiler::read_compiler_request(&mut server).unwrap();
            blueice_ipc::compiler::write_compiler_reply(
                &mut server,
                &blueice_ipc::compiler::negotiate(&hello),
            )
            .unwrap();
            let request = blueice_ipc::compiler::read_compiler_request(&mut server).unwrap();
            let reply = request_sender.request(request).unwrap();
            blueice_ipc::compiler::write_compiler_reply(&mut server, &reply).unwrap();
        });

        let mcp_client = thread::spawn(move || {
            let mut connection = CompilerConnection::new(client);
            connection.handshake().unwrap();
            connection.check(project.id).unwrap()
        });
        while core_session.dispatch_pending(&request_receiver) == 0 {
            thread::yield_now();
        }
        let blueice_ipc::compiler::CompilerReply::Check(check) = mcp_client.join().unwrap() else {
            panic!("the MCP client must receive a source-free core check reply")
        };
        assert_eq!(check.generation.project, project);
        assert!(!check.has_errors, "{check:#?}");
        assert!(check.static_metadata.is_some());
        assert!(check.artifact_fingerprint.is_some());
        assert_eq!(core_session.registered_project_count(), 1);
        listener.join().unwrap();
    }

    #[test]
    fn compiler_connection_negotiates_and_sends_only_opaque_query_handles() {
        let (client, mut server) = UnixStream::pair().unwrap();
        let worker = thread::spawn(move || {
            let hello = blueice_ipc::compiler::read_compiler_request(&mut server).unwrap();
            assert!(matches!(
                hello,
                blueice_ipc::compiler::CompilerRequest::Hello {
                    protocol_version: blueice_ipc::compiler::COMPILER_PROTOCOL_VERSION,
                }
            ));
            blueice_ipc::compiler::write_compiler_reply(
                &mut server,
                &blueice_ipc::compiler::negotiate(&hello),
            )
            .unwrap();

            assert!(matches!(
                blueice_ipc::compiler::read_compiler_request(&mut server).unwrap(),
                blueice_ipc::compiler::CompilerRequest::Check {
                    project: blueice_ipc::compiler::CompilerProject { id: 41 },
                }
            ));
            blueice_ipc::compiler::write_compiler_reply(
                &mut server,
                &blueice_ipc::compiler::CompilerReply::Error {
                    code: blueice_ipc::compiler::CompilerErrorCode::InvalidProject,
                    message: "unknown registered compiler project".to_string(),
                },
            )
            .unwrap();

            assert!(matches!(
                blueice_ipc::compiler::read_compiler_request(&mut server).unwrap(),
                blueice_ipc::compiler::CompilerRequest::GetStaticType {
                    generation: blueice_ipc::compiler::CompilerGeneration {
                        project: blueice_ipc::compiler::CompilerProject { id: 41 },
                        sequence: 9,
                    },
                    type_id: 3,
                }
            ));
            blueice_ipc::compiler::write_compiler_reply(
                &mut server,
                &blueice_ipc::compiler::CompilerReply::Error {
                    code: blueice_ipc::compiler::CompilerErrorCode::StaleGeneration,
                    message: "stale compiler generation".to_string(),
                },
            )
            .unwrap();

            assert!(matches!(
                blueice_ipc::compiler::read_compiler_request(&mut server).unwrap(),
                blueice_ipc::compiler::CompilerRequest::GetStaticSymbol {
                    generation: blueice_ipc::compiler::CompilerGeneration {
                        project: blueice_ipc::compiler::CompilerProject { id: 41 },
                        sequence: 9,
                    },
                    symbol_id: 4,
                }
            ));
            blueice_ipc::compiler::write_compiler_reply(
                &mut server,
                &blueice_ipc::compiler::CompilerReply::Error {
                    code: blueice_ipc::compiler::CompilerErrorCode::UnknownSymbol,
                    message: "unknown static symbol".to_string(),
                },
            )
            .unwrap();
        });

        let mut connection = CompilerConnection::new(client);
        connection.handshake().unwrap();
        assert!(matches!(
            connection.check(41).unwrap(),
            blueice_ipc::compiler::CompilerReply::Error {
                code: blueice_ipc::compiler::CompilerErrorCode::InvalidProject,
                ..
            }
        ));
        assert!(matches!(
            connection.static_type(41, 9, 3).unwrap(),
            blueice_ipc::compiler::CompilerReply::Error {
                code: blueice_ipc::compiler::CompilerErrorCode::StaleGeneration,
                ..
            }
        ));
        assert!(matches!(
            connection.static_symbol(41, 9, 4).unwrap(),
            blueice_ipc::compiler::CompilerReply::Error {
                code: blueice_ipc::compiler::CompilerErrorCode::UnknownSymbol,
                ..
            }
        ));
        worker.join().unwrap();
    }
}
