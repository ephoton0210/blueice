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
    session_attestation: Option<blueice_ipc::compiler::CompilerSessionAttestation>,
    capability_manifest: Option<blueice_ipc::compiler::CompilerSessionCapabilityManifest>,
}

impl<S: Read + Write> CompilerConnection<S> {
    pub fn new(stream: S) -> Self {
        Self {
            stream,
            session_attestation: None,
            capability_manifest: None,
        }
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

        // A retry must not leave prior core evidence usable when this new
        // negotiation fails. The connection is query-capable only after the
        // exact acknowledgement below repopulates both values.
        self.session_attestation = None;
        self.capability_manifest = None;
        write_compiler_request(
            &mut self.stream,
            &CompilerRequest::Hello {
                protocol_version: COMPILER_PROTOCOL_VERSION,
            },
        )?;
        match read_compiler_reply(&mut self.stream)? {
            CompilerReply::HelloAck {
                protocol_version,
                session_attestation,
                capability_manifest,
            } if protocol_version == COMPILER_PROTOCOL_VERSION
                && session_attestation.is_well_formed()
                && capability_manifest.is_well_formed() =>
            {
                self.session_attestation = Some(session_attestation);
                self.capability_manifest = Some(capability_manifest);
                Ok(())
            }
            CompilerReply::Error { message, .. } => Err(io::Error::other(message)),
            reply => Err(io::Error::other(format!(
                "expected compiler Hello handshake reply, got {reply:?}"
            ))),
        }
    }

    /// Returns the source-free core evidence minted for this exact accepted
    /// transport stream. It is available only after a successful v4
    /// handshake, and it grants no authority beyond the stream itself.
    pub fn session_attestation(
        &self,
    ) -> Option<&blueice_ipc::compiler::CompilerSessionAttestation> {
        self.session_attestation.as_ref()
    }

    /// Returns the complete fixed query-only capability manifest selected by
    /// the core for this accepted stream. It is never constructed from MCP
    /// tool definitions or caller input, and remains absent after any failed
    /// handshake.
    pub fn capability_manifest(
        &self,
    ) -> Option<&blueice_ipc::compiler::CompilerSessionCapabilityManifest> {
        self.capability_manifest.as_ref()
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

    /// Lists one capped, source-free page of compiler-minted metadata IDs for
    /// an exact retained generation. A continuation cursor is opaque,
    /// generation-bound, and one-shot; this client forwards it verbatim and
    /// never guesses IDs or substitutes another generation.
    pub fn static_metadata_page(
        &mut self,
        project_id: u64,
        generation: u64,
        kind: blueice_ipc::compiler::CompilerStaticMetadataKind,
        cursor: Option<u64>,
        limit: Option<u32>,
    ) -> io::Result<blueice_ipc::compiler::CompilerReply> {
        self.request(blueice_ipc::compiler::CompilerRequest::ListStaticMetadata {
            generation: compiler_generation(project_id, generation),
            kind,
            cursor: cursor.map(|id| blueice_ipc::compiler::CompilerStaticMetadataCursor { id }),
            limit,
        })
    }

    /// Looks up one source-text-free static provenance record from exactly one
    /// retained compiler generation. `source_id` is compiler-minted metadata,
    /// never a path or a source-read capability.
    pub fn static_provenance(
        &mut self,
        project_id: u64,
        generation: u64,
        source_id: u32,
    ) -> io::Result<blueice_ipc::compiler::CompilerReply> {
        self.request(
            blueice_ipc::compiler::CompilerRequest::GetStaticProvenance {
                generation: compiler_generation(project_id, generation),
                source_id,
            },
        )
    }

    /// Returns the bounded static summary of one reifiable contract retained
    /// by the exact compiler generation. The summary cannot retrieve source
    /// or inspect a live JavaScript value.
    pub fn static_contract(
        &mut self,
        project_id: u64,
        generation: u64,
        contract_id: u32,
    ) -> io::Result<blueice_ipc::compiler::CompilerReply> {
        self.request(blueice_ipc::compiler::CompilerRequest::GetStaticContract {
            generation: compiler_generation(project_id, generation),
            contract_id,
        })
    }

    /// Validates one data-only snapshot against a compiler-retained exact
    /// static contract. The request does not execute JavaScript and does not
    /// echo the snapshot in its reply.
    pub fn validate_static_contract(
        &mut self,
        project_id: u64,
        generation: u64,
        contract_id: u32,
        value: blueice_ipc::compiler::CompilerContractValue,
    ) -> io::Result<blueice_ipc::compiler::CompilerReply> {
        self.request(
            blueice_ipc::compiler::CompilerRequest::ValidateStaticContract {
                generation: compiler_generation(project_id, generation),
                contract_id,
                value,
            },
        )
    }

    fn request(
        &mut self,
        request: blueice_ipc::compiler::CompilerRequest,
    ) -> io::Result<blueice_ipc::compiler::CompilerReply> {
        if self.session_attestation.is_none() || self.capability_manifest.is_none() {
            return Err(io::Error::new(
                io::ErrorKind::NotConnected,
                "compiler query requires a completed core-attested capability handshake",
            ));
        }
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

    fn session_attestation() -> blueice_ipc::compiler::CompilerSessionAttestation {
        blueice_ipc::compiler::CompilerSessionAttestation {
            id: "b2".repeat(32),
        }
    }

    fn session_evidence() -> blueice_ipc::compiler::CompilerSessionHelloEvidence {
        blueice_ipc::compiler::CompilerSessionHelloEvidence {
            session_attestation: session_attestation(),
            capability_manifest:
                blueice_ipc::compiler::CompilerSessionCapabilityManifest::fixed_query_only(),
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
                &blueice_ipc::compiler::negotiate(&hello, Some(session_evidence())),
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
    fn compiler_connection_discovers_symbol_ids_and_round_trips_exact_contract_and_provenance_queries(
    ) {
        const ENTRY: &str = "project:///mcp/contracts.ts";
        let registration = RegisteredProjectRegistration {
            canonical_project_root: "project:///mcp".to_string(),
            canonical_config_root: "project:///mcp/blue-ts.json".to_string(),
            canonical_output_root: "project:///mcp-dist".to_string(),
            entry_module: ENTRY.to_string(),
            loader: AuthorizedModuleLoader::new(
                [AuthorizedModule::new(
                    ENTRY,
                    "interface Settings { enabled: boolean; } \
                     export const settings: Settings = { enabled: true };",
                )],
                [],
            )
            .unwrap(),
            compiler_options: CompilerOptions {
                resolver_fingerprint: "mcp-contract-fixture-v1".to_string(),
                runtime_policy: RuntimePolicy::Checked,
                ..CompilerOptions::default()
            },
        };
        let mut catalog = CoreCompilerProjectCatalog::default();
        let project = catalog.register_startup_project(registration).unwrap();
        let mut core_session = catalog.seal();
        let (request_sender, request_receiver) = compiler_service_ipc_request_channel();
        let (client, mut server) = UnixStream::pair().unwrap();
        let listener = thread::spawn(move || {
            let hello = blueice_ipc::compiler::read_compiler_request(&mut server).unwrap();
            blueice_ipc::compiler::write_compiler_reply(
                &mut server,
                &blueice_ipc::compiler::negotiate(&hello, Some(session_evidence())),
            )
            .unwrap();
            for _ in 0..6 {
                let request = blueice_ipc::compiler::read_compiler_request(&mut server).unwrap();
                let reply = request_sender.request(request).unwrap();
                blueice_ipc::compiler::write_compiler_reply(&mut server, &reply).unwrap();
            }
        });

        let mcp_client = thread::spawn(move || {
            let mut connection = CompilerConnection::new(client);
            connection.handshake().unwrap();
            let blueice_ipc::compiler::CompilerReply::Check(check) =
                connection.check(project.id).unwrap()
            else {
                panic!("check must return an exact generation")
            };
            let blueice_ipc::compiler::CompilerReply::StaticMetadataPage(symbols) = connection
                .static_metadata_page(
                    project.id,
                    check.generation.sequence,
                    blueice_ipc::compiler::CompilerStaticMetadataKind::Symbols,
                    None,
                    Some(128),
                )
                .unwrap()
            else {
                panic!("check metadata must discover bounded static symbol IDs")
            };
            assert!(symbols.next_cursor.is_none());
            assert_eq!(
                u32::try_from(symbols.ids.len()).unwrap(),
                check.static_metadata.as_ref().unwrap().symbol_count
            );
            let blueice_ipc::compiler::CompilerReply::StaticSymbol(symbol) = connection
                .static_symbol(
                    project.id,
                    check.generation.sequence,
                    *symbols
                        .ids
                        .first()
                        .expect("fixture interface must be returned by inventory"),
                )
                .unwrap()
            else {
                panic!("interface declaration must return a static symbol")
            };
            let contract_id = symbol
                .contract_id
                .expect("local interface must have a contract");
            let provenance = connection
                .static_provenance(project.id, check.generation.sequence, symbol.source_id)
                .unwrap();
            let contract = connection
                .static_contract(project.id, check.generation.sequence, contract_id)
                .unwrap();
            let validation = connection
                .validate_static_contract(
                    project.id,
                    check.generation.sequence,
                    contract_id,
                    blueice_ipc::compiler::CompilerContractValue::Object(
                        std::collections::BTreeMap::from([(
                            "enabled".to_string(),
                            blueice_ipc::compiler::CompilerContractValue::Boolean(true),
                        )]),
                    ),
                )
                .unwrap();
            (provenance, contract, validation)
        });
        while !mcp_client.is_finished() {
            if core_session.dispatch_pending(&request_receiver) == 0 {
                thread::yield_now();
            }
        }
        let (provenance, contract, validation) = mcp_client.join().unwrap();
        assert!(matches!(
            provenance,
            blueice_ipc::compiler::CompilerReply::StaticProvenance(_)
        ));
        assert!(matches!(
            contract,
            blueice_ipc::compiler::CompilerReply::StaticContract(_)
        ));
        assert!(matches!(
            validation,
            blueice_ipc::compiler::CompilerReply::ContractValidation(
                blueice_ipc::compiler::CompilerContractValidation { valid: true, .. }
            )
        ));
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
                &blueice_ipc::compiler::negotiate(&hello, Some(session_evidence())),
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
        assert_eq!(
            connection.session_attestation(),
            Some(&session_attestation())
        );
        assert_eq!(
            connection.capability_manifest(),
            Some(&blueice_ipc::compiler::CompilerSessionCapabilityManifest::fixed_query_only())
        );
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

    #[test]
    fn compiler_connection_rejects_a_malformed_core_attestation_before_queries() {
        let (client, mut server) = UnixStream::pair().unwrap();
        let worker = thread::spawn(move || {
            let _hello = blueice_ipc::compiler::read_compiler_request(&mut server).unwrap();
            blueice_ipc::compiler::write_compiler_reply(
                &mut server,
                &blueice_ipc::compiler::CompilerReply::HelloAck {
                    protocol_version: blueice_ipc::compiler::COMPILER_PROTOCOL_VERSION,
                    session_attestation: blueice_ipc::compiler::CompilerSessionAttestation {
                        id: "not-hex".to_string(),
                    },
                    capability_manifest:
                        blueice_ipc::compiler::CompilerSessionCapabilityManifest::fixed_query_only(),
                },
            )
            .unwrap();
        });

        let mut connection = CompilerConnection::new(client);
        let error = connection.handshake().unwrap_err();
        assert!(
            error.to_string().contains("expected compiler Hello"),
            "malformed core evidence must not create a usable compiler session: {error}"
        );
        assert_eq!(connection.session_attestation(), None);
        assert_eq!(connection.capability_manifest(), None);
        worker.join().unwrap();
    }

    #[test]
    fn compiler_connection_rejects_a_malformed_core_capability_manifest_before_queries() {
        let (client, mut server) = UnixStream::pair().unwrap();
        let worker = thread::spawn(move || {
            let _hello = blueice_ipc::compiler::read_compiler_request(&mut server).unwrap();
            blueice_ipc::compiler::write_compiler_reply(
                &mut server,
                &blueice_ipc::compiler::CompilerReply::HelloAck {
                    protocol_version: blueice_ipc::compiler::COMPILER_PROTOCOL_VERSION,
                    session_attestation: session_attestation(),
                    capability_manifest: blueice_ipc::compiler::CompilerSessionCapabilityManifest {
                        version: 0,
                        operation_ids: Vec::new(),
                    },
                },
            )
            .unwrap();
        });

        let mut connection = CompilerConnection::new(client);
        let error = connection.handshake().unwrap_err();
        assert!(
            error.to_string().contains("expected compiler Hello"),
            "malformed core manifest must not create a usable compiler session: {error}"
        );
        assert_eq!(connection.session_attestation(), None);
        assert_eq!(connection.capability_manifest(), None);
        worker.join().unwrap();
    }

    #[test]
    fn compiler_connection_rejects_queries_before_a_core_capability_handshake() {
        let (client, _server) = UnixStream::pair().unwrap();
        let mut connection = CompilerConnection::new(client);
        let error = connection.check(1).unwrap_err();
        assert_eq!(error.kind(), io::ErrorKind::NotConnected);
        assert!(error
            .to_string()
            .contains("core-attested capability handshake"));
    }
}
