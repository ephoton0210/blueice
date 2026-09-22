// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

//! Private, versioned transport between `blueice-launcher` and its isolated
//! BlueJS page host child.
//!
//! This is intentionally neither the public frontend protocol nor the narrow
//! script-to-DOM channel. A launcher creates one private Unix socket and a
//! one-time capability token for one child, then a core-owned page loader may
//! supply already-authorized source records through that connection. The
//! child never receives a filesystem path, URL to fetch, DOM handle, network
//! authority, or a resolver callback. It receives only complete source graphs
//! selected by its caller and reports only bounded, source-free outcomes.
//!
//! Version 2 deliberately covers document lifecycle plus classic and static
//! ESM graph execution for caller-classified standard JavaScript and explicit
//! BlueTS declarations. BlueTS stays a child-fixed, direct-lowering profile:
//! no compiler option, typing profile, resolver, or emitted JavaScript crosses
//! this channel. It does not expose a debugger, DOM operation, runtime value,
//! host callback, fetch/cache, or general client-facing API.

use serde::{Deserialize, Serialize};
use std::io::{self, Read, Write};

/// Independent version for the private launcher-to-BlueJS-host channel.
pub const PAGE_HOST_PROTOCOL_VERSION: u32 = 2;

/// Maximum private page-host request/reply frame. The child rejects a length
/// above this cap before allocating a payload buffer or deserializing source.
/// This is separate from per-document source budgets enforced by the child.
pub const PAGE_HOST_MAX_FRAME_BYTES: usize = 12 * 1024 * 1024;

/// A complete source record selected and fingerprinted by the caller-owned
/// page loader. `source_hash` is verified by the child against `source`; it
/// is never a client-supplied assertion that the child blindly trusts.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct PageHostSource {
    pub canonical_module_id: String,
    pub source: String,
    pub source_hash: String,
}

impl PageHostSource {
    /// Creates a source record with the v1 deterministic FNV-1a content
    /// fingerprint. This detects accidental record mismatches and gives the
    /// program registry a stable identity; it is not cryptographic integrity
    /// and does not replace the caller's integrity, CSP, cache, or origin
    /// policy.
    pub fn new(canonical_module_id: impl Into<String>, source: impl Into<String>) -> Self {
        let source = source.into();
        Self {
            canonical_module_id: canonical_module_id.into(),
            source_hash: source_hash(&source),
            source,
        }
    }
}

/// One caller-authorized static import/export resolution. The child validates
/// this exact record against every static request; it never performs relative
/// URL, import-map, filesystem, package-manager, or network resolution.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct PageHostStaticResolution {
    pub from_module: String,
    pub specifier: String,
    pub canonical_target: String,
}

/// A closed, caller-authorized source graph for one page declaration.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct PageHostModuleGraph {
    pub entry: String,
    pub modules: Vec<PageHostSource>,
    pub resolutions: Vec<PageHostStaticResolution>,
    /// Caller-owned identity for the origin/integrity/cache resolver policy.
    /// It is recorded only as an opaque non-empty fingerprint in this v1
    /// transport; the child does not interpret it or gain the resolver.
    pub resolver_fingerprint: String,
}

/// Script grammar selected by the trusted page pipeline, never by a filename
/// suffix or the child host's own MIME sniffing.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum PageHostScriptKind {
    Classic,
    Module,
}

/// The source language selected by the trusted page pipeline. An ordinary
/// JavaScript declaration never becomes BlueTS merely because its source is
/// syntactically accepted by the compiler.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum PageHostScriptLanguage {
    JavaScript,
    BlueTs,
}

/// One declaration in document order. Its graph is fully supplied before the
/// child parses or executes anything.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct PageHostScript {
    pub ordinal: u32,
    pub language: PageHostScriptLanguage,
    pub kind: PageHostScriptKind,
    pub graph: PageHostModuleGraph,
}

/// One caller-authorized document snapshot. `origin` is opaque to the child:
/// it must already be canonicalized and policy-approved by core/launcher.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct PageHostDocument {
    pub tab_id: u64,
    pub document_generation: u64,
    pub origin: String,
    pub scripts: Vec<PageHostScript>,
}

/// Source-free outcome for one attempted declaration. These records are safe
/// to return to the launcher/core but are not a runtime-value or diagnostic
/// transport.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct PageHostScriptReport {
    pub tab_id: u64,
    pub document_generation: u64,
    pub ordinal: u32,
    pub language: PageHostScriptLanguage,
    pub kind: PageHostScriptKind,
    pub outcome: PageHostScriptOutcome,
}

/// Bounded outcome category. The child owns these fixed labels; it never
/// forwards parser/compiler/runtime error text or page source.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum PageHostScriptOutcome {
    Executed,
    Rejected { category: String },
}

/// Safe, aggregate accounting for one live child-owned realm. Heap object
/// identities and bytecode/source remain private to the child.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct PageHostRealmStats {
    pub tab_id: u64,
    pub document_generation: u64,
    pub program_count: u32,
    pub bytecode_bytes: u64,
    pub heap_bytes: u64,
}

/// Launcher/core requests to the private host. `Hello` carries the per-spawn
/// secret capability so a same-user process that guesses a socket pathname
/// cannot claim the child before its launcher does.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum PageHostRequest {
    Hello {
        protocol_version: u32,
        session_token: String,
    },
    /// Applies a new document only if its generation succeeds the tab's
    /// current child-owned generation. Repeating the current generation is
    /// idempotent and never re-runs page code.
    SynchronizeDocument { document: PageHostDocument },
    /// Releases one exact live realm. A stale generation cannot close its
    /// successor after navigation.
    CloseRealm {
        tab_id: u64,
        document_generation: u64,
    },
    /// Returns only bounded, aggregate accounting for one exact live realm.
    GetRealmStats {
        tab_id: u64,
        document_generation: u64,
    },
    /// Ends the child process after its acknowledgement.
    Shutdown,
    /// A newer request must not be interpreted as an existing operation.
    #[serde(other)]
    Unknown,
}

/// Child replies for [`PageHostRequest`].
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum PageHostReply {
    HelloAck {
        protocol_version: u32,
    },
    Synchronized {
        tab_id: u64,
        document_generation: u64,
        /// `true` when the child already owned this exact document and did
        /// not execute any declaration again.
        already_current: bool,
        reports: Vec<PageHostScriptReport>,
    },
    RealmClosed {
        tab_id: u64,
        document_generation: u64,
    },
    RealmStats(PageHostRealmStats),
    ShutdownAck,
    Error {
        code: PageHostErrorCode,
        /// Fixed host-owned prose only; callers branch on `code`.
        message: String,
    },
}

/// Stable transport/lifecycle failure categories.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum PageHostErrorCode {
    ProtocolVersion,
    Authentication,
    InvalidRequest,
    StaleDocument,
    UnknownRealm,
    ResourceLimit,
    HostFailure,
}

/// Builds the only valid first reply. The caller must check this before it
/// dispatches a request to a live realm owner.
pub fn negotiate(request: &PageHostRequest, expected_session_token: &str) -> PageHostReply {
    match request {
        PageHostRequest::Hello {
            protocol_version,
            session_token,
        } if *protocol_version != PAGE_HOST_PROTOCOL_VERSION => PageHostReply::Error {
            code: PageHostErrorCode::ProtocolVersion,
            message: "unsupported BlueJS page-host protocol version".to_string(),
        },
        PageHostRequest::Hello { session_token, .. } if session_token != expected_session_token => {
            PageHostReply::Error {
                code: PageHostErrorCode::Authentication,
                message: "BlueJS page-host session capability was rejected".to_string(),
            }
        }
        PageHostRequest::Hello { .. } => PageHostReply::HelloAck {
            protocol_version: PAGE_HOST_PROTOCOL_VERSION,
        },
        _ => PageHostReply::Error {
            code: PageHostErrorCode::ProtocolVersion,
            message: "BlueJS page-host protocol requires Hello as its first request".to_string(),
        },
    }
}

/// Writes one length-prefixed request frame.
pub fn write_page_host_request<W: Write>(
    writer: &mut W,
    request: &PageHostRequest,
) -> io::Result<()> {
    crate::write_framed(writer, request)
}

/// Reads one length-prefixed request frame.
pub fn read_page_host_request<R: Read>(reader: &mut R) -> io::Result<PageHostRequest> {
    let bytes = crate::read_frame_bytes_with_limit(reader, PAGE_HOST_MAX_FRAME_BYTES)?;
    serde_json::from_slice(&bytes).map_err(io::Error::other)
}

/// Writes one length-prefixed child reply frame.
pub fn write_page_host_reply<W: Write>(writer: &mut W, reply: &PageHostReply) -> io::Result<()> {
    crate::write_framed(writer, reply)
}

/// Reads one length-prefixed child reply frame.
pub fn read_page_host_reply<R: Read>(reader: &mut R) -> io::Result<PageHostReply> {
    let bytes = crate::read_frame_bytes_with_limit(reader, PAGE_HOST_MAX_FRAME_BYTES)?;
    serde_json::from_slice(&bytes).map_err(io::Error::other)
}

/// The deterministic v1 source-content fingerprint shared by loader and
/// child. It detects accidental transport substitution but is not a
/// cryptographic integrity check; the caller's loader must enforce any
/// cryptographic integrity policy before it authorizes a record.
pub fn source_hash(source: &str) -> String {
    let mut hash = 0xcbf2_9ce4_8422_2325u64;
    for byte in source.bytes() {
        hash ^= u64::from(byte);
        hash = hash.wrapping_mul(0x0000_0100_0000_01b3);
    }
    format!("fnv1a64:{hash:016x}")
}

#[cfg(all(test, unix))]
mod tests {
    use super::*;
    use std::os::unix::net::UnixStream;

    fn source() -> PageHostSource {
        PageHostSource::new("blueice://page/main.js", "globalThis.answer = 42;")
    }

    fn document() -> PageHostDocument {
        PageHostDocument {
            tab_id: 7,
            document_generation: 3,
            origin: "https://example.test".to_string(),
            scripts: vec![PageHostScript {
                ordinal: 0,
                language: PageHostScriptLanguage::JavaScript,
                kind: PageHostScriptKind::Classic,
                graph: PageHostModuleGraph {
                    entry: "blueice://page/main.js".to_string(),
                    modules: vec![source()],
                    resolutions: vec![],
                    resolver_fingerprint: "core-loader-v1".to_string(),
                },
            }],
        }
    }

    #[test]
    fn requests_and_replies_round_trip_over_a_real_socket() {
        let requests = [
            PageHostRequest::Hello {
                protocol_version: PAGE_HOST_PROTOCOL_VERSION,
                session_token: "not-a-real-token".to_string(),
            },
            PageHostRequest::SynchronizeDocument {
                document: document(),
            },
            PageHostRequest::CloseRealm {
                tab_id: 7,
                document_generation: 3,
            },
            PageHostRequest::GetRealmStats {
                tab_id: 7,
                document_generation: 3,
            },
            PageHostRequest::Shutdown,
            PageHostRequest::Unknown,
        ];
        for request in requests {
            let (mut writer, mut reader) = UnixStream::pair().unwrap();
            write_page_host_request(&mut writer, &request).unwrap();
            assert_eq!(read_page_host_request(&mut reader).unwrap(), request);
        }

        let reply = PageHostReply::Synchronized {
            tab_id: 7,
            document_generation: 3,
            already_current: false,
            reports: vec![PageHostScriptReport {
                tab_id: 7,
                document_generation: 3,
                ordinal: 0,
                language: PageHostScriptLanguage::JavaScript,
                kind: PageHostScriptKind::Classic,
                outcome: PageHostScriptOutcome::Executed,
            }],
        };
        let (mut writer, mut reader) = UnixStream::pair().unwrap();
        write_page_host_reply(&mut writer, &reply).unwrap();
        assert_eq!(read_page_host_reply(&mut reader).unwrap(), reply);
    }

    #[test]
    fn handshake_requires_the_exact_version_and_capability() {
        let token = "launcher-secret";
        assert_eq!(
            negotiate(
                &PageHostRequest::Hello {
                    protocol_version: PAGE_HOST_PROTOCOL_VERSION,
                    session_token: token.to_string(),
                },
                token,
            ),
            PageHostReply::HelloAck {
                protocol_version: PAGE_HOST_PROTOCOL_VERSION,
            }
        );
        assert!(matches!(
            negotiate(
                &PageHostRequest::Hello {
                    protocol_version: 1,
                    session_token: token.to_string(),
                },
                token,
            ),
            PageHostReply::Error {
                code: PageHostErrorCode::ProtocolVersion,
                ..
            }
        ));
        assert!(matches!(
            negotiate(
                &PageHostRequest::Hello {
                    protocol_version: PAGE_HOST_PROTOCOL_VERSION,
                    session_token: "wrong".to_string(),
                },
                token,
            ),
            PageHostReply::Error {
                code: PageHostErrorCode::Authentication,
                ..
            }
        ));
        assert!(matches!(
            negotiate(
                &PageHostRequest::SynchronizeDocument {
                    document: document(),
                },
                token,
            ),
            PageHostReply::Error {
                code: PageHostErrorCode::ProtocolVersion,
                ..
            }
        ));
    }

    #[test]
    fn source_constructor_fingerprints_exact_bytes() {
        let source = PageHostSource::new("blueice://page/main.js", "let answer = 42;");
        assert_eq!(source.source_hash, source_hash(&source.source));
        assert_ne!(source.source_hash, source_hash("let answer = 43;"));
    }

    #[test]
    fn page_host_rejects_an_oversized_frame_before_payload_allocation() {
        let oversized = u32::try_from(PAGE_HOST_MAX_FRAME_BYTES + 1).unwrap();
        let mut bytes = oversized.to_le_bytes().to_vec();
        assert!(read_page_host_request(&mut std::io::Cursor::new(&mut bytes)).is_err());
    }
}
