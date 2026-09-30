// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

//! `generic_routes.rs`'s fixed `unavailable_*` reply constructors and
//! `debugger_program_error`'s full error-code mapping are pure, but several
//! of them were never reached by any existing test because no test happened
//! to drive a real dispatch into that exact unavailable-capability or
//! child-error branch. Assert their exact, fixed output directly.

use super::*;

fn capability_unavailable(message: &str) -> DebuggerReply {
    DebuggerReply::Error {
        code: DebuggerErrorCode::CapabilityUnavailable,
        message: message.to_string(),
    }
}

#[test]
fn fixed_capability_unavailable_replies_carry_their_documented_message() {
    assert_eq!(
        unavailable_stepping(),
        capability_unavailable(
            "native debugger root stepping is not installed for this page-host route"
        )
    );
    assert_eq!(
        unavailable_nested_frames(),
        capability_unavailable(
            "native debugger nested-frame control is not installed for this page-host route"
        )
    );
    assert_eq!(
        unavailable_linked_modules(),
        capability_unavailable("linked module debugger control is unavailable")
    );
    assert_eq!(
        unavailable_stack(),
        capability_unavailable(
            "bounded debugger stack inspection is not installed for this page-host route"
        )
    );
    assert_eq!(
        unavailable_scopes(),
        capability_unavailable(
            "bounded debugger scope inspection is not installed for this page-host route"
        )
    );
    assert_eq!(
        unavailable_static_scope_relation(),
        capability_unavailable(
            "static scope relations require an independent owner/client grant and live child route"
        )
    );
}

#[test]
fn debugger_program_error_maps_every_remaining_child_error_to_its_own_code_and_message() {
    assert_eq!(
        debugger_program_error(JavaScriptPageDebuggerError::ResourceLimit),
        DebuggerReply::Error {
            code: DebuggerErrorCode::ResourceLimit,
            message: "too many verified debugger safe points for one program".to_string(),
        }
    );
    assert_eq!(
        debugger_program_error(JavaScriptPageDebuggerError::BreakpointLimit),
        DebuggerReply::Error {
            code: DebuggerErrorCode::ResourceLimit,
            message: "too many native breakpoint records for one page realm".to_string(),
        }
    );
    assert_eq!(
        debugger_program_error(JavaScriptPageDebuggerError::NotExecutableEntry),
        DebuggerReply::Error {
            code: DebuggerErrorCode::InvalidSafePoint,
            message: "debugger entry pause accepts only a pending root instruction boundary"
                .to_string(),
        }
    );
    assert_eq!(
        debugger_program_error(JavaScriptPageDebuggerError::NotResumableRootSafePoint),
        DebuggerReply::Error {
            code: DebuggerErrorCode::InvalidSafePoint,
            message:
                "debugger root continuation accepts only a pending classic-script root code-unit boundary"
                    .to_string(),
        }
    );
}
