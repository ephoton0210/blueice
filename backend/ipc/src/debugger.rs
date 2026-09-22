// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

//! Versioned native debugger IPC vocabulary.
//!
//! This channel is intentionally separate from page-script DOM calls and the
//! public automation protocol. It gives `core` and an out-of-process BlueJS
//! host one typed way to agree on a page realm, its generation, and executable
//! program locations. It establishes framing, handshake, capability discovery,
//! bounded opaque program-location operations, and exact breakpoint
//! configuration. A host must report every operation as
//! [`DebuggerCapabilityState::Available`] only after it implements the native
//! behavior; a configured breakpoint is not evidence that pause, stack,
//! scope, or value inspection already exists.

use serde::{Deserialize, Serialize};
use std::io::{self, Read, Write};

/// Independent protocol version for the private core-to-BlueJS debugger
/// channel. It does not share `crate::PROTOCOL_VERSION`, whose lifecycle is
/// the frontend control-plane protocol.
pub const DEBUGGER_PROTOCOL_VERSION: u32 = 3;

/// A core-owned page realm identity. The browser-context field is present from
/// from the first protocol revision even while the current core exposes only
/// its default context, so an old debugger client cannot silently retarget an
/// equally numbered tab in a future context.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct DebuggerPageRealm {
    pub browser_context_id: u64,
    pub tab_id: u64,
    pub realm_generation: u64,
}

impl DebuggerPageRealm {
    /// The wire format never treats zero as a valid owner-issued identity.
    pub fn is_well_formed(self) -> bool {
        self.browser_context_id != 0 && self.tab_id != 0 && self.realm_generation != 0
    }
}

/// An exact BlueJS program generation in one page realm. It is deliberately
/// separate from a source URL or an offset: a replacement program cannot
/// inherit a debugger handle merely because it has the same source identity.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct DebuggerProgram {
    pub realm: DebuggerPageRealm,
    pub program_handle: u64,
    pub program_generation: u64,
}

impl DebuggerProgram {
    pub fn is_well_formed(self) -> bool {
        self.realm.is_well_formed() && self.program_handle != 0 && self.program_generation != 0
    }
}

/// A compiler-recorded executable bytecode boundary for one exact program.
/// Hosts MUST validate this tuple against BlueJS's program registry rather
/// than translating a nearest source offset heuristically.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct DebuggerSafePoint {
    pub program: DebuggerProgram,
    pub code_unit_ordinal: u32,
    pub bytecode_offset: u32,
}

impl DebuggerSafePoint {
    pub fn is_well_formed(self) -> bool {
        self.program.is_well_formed()
    }
}

/// Native debugger features that a host may explicitly advertise.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum DebuggerCapability {
    /// Enumerate opaque live program identities and compiler-verified
    /// instruction boundaries, then revalidate an exact tuple. This does not
    /// pause or inspect a VM.
    ProgramLocations,
    /// Install, enumerate, and remove exact generation-bound breakpoint
    /// records. Configuration alone neither starts nor interrupts execution.
    BreakpointConfiguration,
    /// A breakpoint interrupt/pause hook. This remains distinct from
    /// [`Self::BreakpointConfiguration`] so a host cannot imply that a stored
    /// record has stopped a synchronous VM.
    Breakpoints,
    PauseResume,
    Stepping,
    Stack,
    Scopes,
    ExceptionPolicy,
    BoundedValues,
}

/// Availability is per target and protocol generation. `Planned` never grants
/// a caller permission to invoke a feature; it exists so clients can render a
/// truthful disabled state without guessing from another browser's debugger.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum DebuggerCapabilityState {
    Available,
    Planned,
    Unsupported,
}

/// One bounded, host-controlled capability report.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct DebuggerCapabilityReport {
    pub capability: DebuggerCapability,
    pub state: DebuggerCapabilityState,
    /// A stable, host-controlled reason or implementation label. It MUST NOT
    /// contain script source, runtime values, or an arbitrary thrown value.
    pub detail: String,
}

/// The discovery document for one exact realm generation.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct DebuggerCapabilities {
    pub protocol_version: u32,
    pub realm: DebuggerPageRealm,
    pub reports: Vec<DebuggerCapabilityReport>,
    pub max_stack_frames: u32,
    pub max_scope_bindings: u32,
    pub max_value_preview_bytes: u32,
    /// Maximum instruction boundaries returned by one `ListSafePoints`
    /// operation. This limit does not grant a caller source or bytecode.
    pub max_safe_points_per_program: u32,
    /// Maximum exact breakpoint records retained for one page realm. This
    /// does not grant a caller pause, execution, or runtime-value authority.
    pub max_breakpoints_per_realm: u32,
}

/// Core's requests to the out-of-process BlueJS debugger host.
///
/// Command families are deliberately added only with real native behavior.
/// The first non-discovery family resolves opaque programs and exact
/// compiler-verified instruction boundaries. The second is exact, bounded
/// breakpoint configuration; it remains distinct from a later
/// breakpoint-interrupt/pause operation.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum DebuggerRequest {
    Hello {
        protocol_version: u32,
    },
    /// Lists bounded, currently loaded page realm identities. The reply carries
    /// no URL, source, program, bytecode, or runtime object; clients use the
    /// returned generation only to make a subsequent target-bound discovery
    /// request.
    ListPageRealms,
    DescribeCapabilities {
        realm: DebuggerPageRealm,
    },
    /// Lists opaque program identities currently retained by one exact page
    /// realm. It never returns a source identity, bytecode, VM object, or
    /// completion value.
    ListPrograms {
        realm: DebuggerPageRealm,
    },
    /// Lists bounded, compiler-verified instruction boundaries for one exact
    /// live program generation. A caller must not infer or substitute offsets.
    ListSafePoints {
        program: DebuggerProgram,
    },
    /// Checks one supplied location against the exact current BlueJS program
    /// generation. This operation does not execute or pause the program.
    ValidateSafePoint {
        safe_point: DebuggerSafePoint,
    },
    /// Stores one exact, already compiler-verified breakpoint record for the
    /// current program generation. It does not execute or interrupt a VM.
    SetBreakpoint {
        safe_point: DebuggerSafePoint,
    },
    /// Lists only exact breakpoint records currently retained by one live
    /// realm. The records carry no source, bytecode, VM object, or value.
    ListBreakpoints {
        realm: DebuggerPageRealm,
    },
    /// Removes one exact current breakpoint record. Removal is idempotent,
    /// but its target must still name a live compiler-verified boundary.
    ClearBreakpoint {
        safe_point: DebuggerSafePoint,
    },
    /// Catch-all for a newer request variant. Like the frontend protocol, a
    /// receiver preserves connection framing and replies with `Unsupported`
    /// rather than deserializing an unknown command as an unrelated request.
    #[serde(other)]
    Unknown,
}

/// BlueJS host replies on the debugger channel.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum DebuggerReply {
    HelloAck {
        protocol_version: u32,
    },
    /// Reply to [`DebuggerRequest::ListPageRealms`].
    PageRealms(Vec<DebuggerPageRealm>),
    Capabilities(DebuggerCapabilities),
    Programs(Vec<DebuggerProgram>),
    SafePoints(Vec<DebuggerSafePoint>),
    SafePointValidated {
        safe_point: DebuggerSafePoint,
    },
    BreakpointSet {
        safe_point: DebuggerSafePoint,
    },
    Breakpoints(Vec<DebuggerSafePoint>),
    BreakpointCleared {
        safe_point: DebuggerSafePoint,
        was_present: bool,
    },
    Unsupported {
        operation: String,
        reason: String,
    },
    Error {
        code: DebuggerErrorCode,
        message: String,
    },
}

/// Stable error categories for native debugger implementations and their
/// adapters. `message` remains diagnostic prose; clients branch on this code.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum DebuggerErrorCode {
    ProtocolVersion,
    InvalidTarget,
    StaleRealm,
    StaleProgram,
    InvalidSafePoint,
    CapabilityUnavailable,
    ResourceLimit,
}

/// Builds the only valid reply to the connection's first request. A caller
/// must still reject any non-`Hello` first request before it dispatches the
/// connection to a realm owner.
pub fn negotiate(request: &DebuggerRequest) -> DebuggerReply {
    match request {
        DebuggerRequest::Hello { protocol_version }
            if *protocol_version == DEBUGGER_PROTOCOL_VERSION =>
        {
            DebuggerReply::HelloAck {
                protocol_version: DEBUGGER_PROTOCOL_VERSION,
            }
        }
        DebuggerRequest::Hello { .. } => DebuggerReply::Error {
            code: DebuggerErrorCode::ProtocolVersion,
            message: "unsupported debugger protocol version".to_string(),
        },
        DebuggerRequest::ListPageRealms
        | DebuggerRequest::DescribeCapabilities { .. }
        | DebuggerRequest::ListPrograms { .. }
        | DebuggerRequest::ListSafePoints { .. }
        | DebuggerRequest::ValidateSafePoint { .. }
        | DebuggerRequest::SetBreakpoint { .. }
        | DebuggerRequest::ListBreakpoints { .. }
        | DebuggerRequest::ClearBreakpoint { .. }
        | DebuggerRequest::Unknown => DebuggerReply::Error {
            code: DebuggerErrorCode::ProtocolVersion,
            message: "debugger protocol requires Hello as its first request".to_string(),
        },
    }
}

pub fn write_debugger_request<W: Write>(
    writer: &mut W,
    request: &DebuggerRequest,
) -> io::Result<()> {
    crate::write_framed(writer, request)
}

pub fn read_debugger_request<R: Read>(reader: &mut R) -> io::Result<DebuggerRequest> {
    let bytes = crate::read_frame_bytes(reader)?;
    serde_json::from_slice(&bytes).map_err(io::Error::other)
}

pub fn write_debugger_reply<W: Write>(writer: &mut W, reply: &DebuggerReply) -> io::Result<()> {
    crate::write_framed(writer, reply)
}

pub fn read_debugger_reply<R: Read>(reader: &mut R) -> io::Result<DebuggerReply> {
    let bytes = crate::read_frame_bytes(reader)?;
    serde_json::from_slice(&bytes).map_err(io::Error::other)
}

#[cfg(all(test, unix))]
mod tests {
    use super::*;
    use std::os::unix::net::UnixStream;

    fn realm() -> DebuggerPageRealm {
        DebuggerPageRealm {
            browser_context_id: 1,
            tab_id: 7,
            realm_generation: 3,
        }
    }

    #[test]
    fn request_and_reply_round_trip_on_a_real_socket() {
        for request in [
            DebuggerRequest::Hello {
                protocol_version: DEBUGGER_PROTOCOL_VERSION,
            },
            DebuggerRequest::ListPageRealms,
            DebuggerRequest::DescribeCapabilities { realm: realm() },
            DebuggerRequest::ListPrograms { realm: realm() },
            DebuggerRequest::ListSafePoints {
                program: DebuggerProgram {
                    realm: realm(),
                    program_handle: 12,
                    program_generation: 5,
                },
            },
            DebuggerRequest::ValidateSafePoint {
                safe_point: DebuggerSafePoint {
                    program: DebuggerProgram {
                        realm: realm(),
                        program_handle: 12,
                        program_generation: 5,
                    },
                    code_unit_ordinal: 0,
                    bytecode_offset: 0,
                },
            },
            DebuggerRequest::SetBreakpoint {
                safe_point: DebuggerSafePoint {
                    program: DebuggerProgram {
                        realm: realm(),
                        program_handle: 12,
                        program_generation: 5,
                    },
                    code_unit_ordinal: 0,
                    bytecode_offset: 0,
                },
            },
            DebuggerRequest::ListBreakpoints { realm: realm() },
            DebuggerRequest::ClearBreakpoint {
                safe_point: DebuggerSafePoint {
                    program: DebuggerProgram {
                        realm: realm(),
                        program_handle: 12,
                        program_generation: 5,
                    },
                    code_unit_ordinal: 0,
                    bytecode_offset: 0,
                },
            },
            DebuggerRequest::Unknown,
        ] {
            let (mut sender, mut receiver) = UnixStream::pair().unwrap();
            write_debugger_request(&mut sender, &request).unwrap();
            assert_eq!(read_debugger_request(&mut receiver).unwrap(), request);
        }

        let reply = DebuggerReply::Capabilities(DebuggerCapabilities {
            protocol_version: DEBUGGER_PROTOCOL_VERSION,
            realm: realm(),
            reports: vec![DebuggerCapabilityReport {
                capability: DebuggerCapability::BreakpointConfiguration,
                state: DebuggerCapabilityState::Available,
                detail: "exact breakpoint configuration is installed".to_string(),
            }],
            max_stack_frames: 64,
            max_scope_bindings: 256,
            max_value_preview_bytes: 4_096,
            max_safe_points_per_program: 4_096,
            max_breakpoints_per_realm: 256,
        });
        let (mut sender, mut receiver) = UnixStream::pair().unwrap();
        write_debugger_reply(&mut sender, &reply).unwrap();
        assert_eq!(read_debugger_reply(&mut receiver).unwrap(), reply);

        let reply = DebuggerReply::PageRealms(vec![realm()]);
        let (mut sender, mut receiver) = UnixStream::pair().unwrap();
        write_debugger_reply(&mut sender, &reply).unwrap();
        assert_eq!(read_debugger_reply(&mut receiver).unwrap(), reply);

        let program = DebuggerProgram {
            realm: realm(),
            program_handle: 12,
            program_generation: 5,
        };
        let safe_point = DebuggerSafePoint {
            program,
            code_unit_ordinal: 0,
            bytecode_offset: 0,
        };
        for reply in [
            DebuggerReply::Programs(vec![program]),
            DebuggerReply::SafePoints(vec![safe_point]),
            DebuggerReply::SafePointValidated { safe_point },
            DebuggerReply::BreakpointSet { safe_point },
            DebuggerReply::Breakpoints(vec![safe_point]),
            DebuggerReply::BreakpointCleared {
                safe_point,
                was_present: true,
            },
        ] {
            let (mut sender, mut receiver) = UnixStream::pair().unwrap();
            write_debugger_reply(&mut sender, &reply).unwrap();
            assert_eq!(read_debugger_reply(&mut receiver).unwrap(), reply);
        }
    }

    #[test]
    fn handshake_rejects_wrong_or_missing_versions_before_dispatch() {
        assert_eq!(
            negotiate(&DebuggerRequest::Hello {
                protocol_version: DEBUGGER_PROTOCOL_VERSION,
            }),
            DebuggerReply::HelloAck {
                protocol_version: DEBUGGER_PROTOCOL_VERSION,
            }
        );
        for unsupported_version in [1, 2, DEBUGGER_PROTOCOL_VERSION + 1] {
            assert!(matches!(
                negotiate(&DebuggerRequest::Hello {
                    protocol_version: unsupported_version,
                }),
                DebuggerReply::Error {
                    code: DebuggerErrorCode::ProtocolVersion,
                    ..
                }
            ));
        }
        assert!(matches!(
            negotiate(&DebuggerRequest::ListPageRealms),
            DebuggerReply::Error {
                code: DebuggerErrorCode::ProtocolVersion,
                ..
            }
        ));
        assert!(matches!(
            negotiate(&DebuggerRequest::DescribeCapabilities { realm: realm() }),
            DebuggerReply::Error {
                code: DebuggerErrorCode::ProtocolVersion,
                ..
            }
        ));
    }

    #[test]
    fn generation_bound_target_and_safe_point_ids_reject_zero_placeholders() {
        let valid_program = DebuggerProgram {
            realm: realm(),
            program_handle: 12,
            program_generation: 5,
        };
        assert!(valid_program.is_well_formed());
        assert!(DebuggerSafePoint {
            program: valid_program,
            code_unit_ordinal: 0,
            bytecode_offset: 0,
        }
        .is_well_formed());
        assert!(!DebuggerPageRealm {
            browser_context_id: 1,
            tab_id: 7,
            realm_generation: 0,
        }
        .is_well_formed());
        assert!(!DebuggerProgram {
            realm: realm(),
            program_handle: 0,
            program_generation: 5,
        }
        .is_well_formed());
    }

    #[test]
    fn malformed_debugger_frames_fail_without_a_panic() {
        let mut bytes = Vec::new();
        let malformed = b"not json";
        bytes.extend_from_slice(&(malformed.len() as u32).to_le_bytes());
        bytes.extend_from_slice(malformed);
        assert!(read_debugger_request(&mut std::io::Cursor::new(bytes)).is_err());
    }
}
