// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

//! `DebuggerMetadataCapability::debugger_capability` is only reached
//! indirectly through `authorize_metadata`, whose existing tests exercise
//! just a few capability variants. Check the full mapping directly.

use super::*;

#[test]
fn debugger_capability_maps_every_metadata_capability_to_its_own_public_capability() {
    let pairs = [
        (
            DebuggerMetadataCapability::OpaqueInventory,
            Some(DebuggerCapability::StaticMetadataInventory),
        ),
        (
            DebuggerMetadataCapability::OpaqueSummary,
            Some(DebuggerCapability::StaticMetadataSummary),
        ),
        (
            DebuggerMetadataCapability::OpaqueSourceInventory,
            Some(DebuggerCapability::StaticMetadataSourceInventory),
        ),
        (
            DebuggerMetadataCapability::OpaqueSourceProvenance,
            Some(DebuggerCapability::StaticMetadataSourceProvenance),
        ),
        (
            DebuggerMetadataCapability::OpaqueTypeInventory,
            Some(DebuggerCapability::StaticMetadataTypeInventory),
        ),
        (
            DebuggerMetadataCapability::OpaqueTypeDisplay,
            Some(DebuggerCapability::StaticMetadataTypeDisplay),
        ),
        (
            DebuggerMetadataCapability::OpaqueSymbolInventory,
            Some(DebuggerCapability::StaticMetadataSymbolInventory),
        ),
        (
            DebuggerMetadataCapability::OpaqueContractInventory,
            Some(DebuggerCapability::StaticMetadataContractInventory),
        ),
        (
            DebuggerMetadataCapability::OpaqueSymbolDisplay,
            Some(DebuggerCapability::StaticMetadataSymbolDisplay),
        ),
        (
            DebuggerMetadataCapability::OpaqueContractDisplay,
            Some(DebuggerCapability::StaticMetadataContractDisplay),
        ),
        (
            DebuggerMetadataCapability::OpaqueContractValidation,
            Some(DebuggerCapability::StaticMetadataContractValidation),
        ),
        (
            DebuggerMetadataCapability::OpaqueLoweringSummary,
            Some(DebuggerCapability::StaticMetadataLoweringSummary),
        ),
        (
            DebuggerMetadataCapability::OpaqueSymbolLocation,
            Some(DebuggerCapability::StaticMetadataSymbolLocation),
        ),
        (
            DebuggerMetadataCapability::OpaqueContractLocation,
            Some(DebuggerCapability::StaticMetadataContractLocation),
        ),
        (
            DebuggerMetadataCapability::OpaqueSymbolType,
            Some(DebuggerCapability::StaticMetadataSymbolType),
        ),
        (
            DebuggerMetadataCapability::OpaqueSymbolContract,
            Some(DebuggerCapability::StaticMetadataSymbolContract),
        ),
        (
            DebuggerMetadataCapability::OpaqueSafePointSpan,
            Some(DebuggerCapability::StaticMetadataSafePointSpan),
        ),
        (
            DebuggerMetadataCapability::OpaqueSourceBreakpoint,
            Some(DebuggerCapability::StaticMetadataSourceBreakpoint),
        ),
        (
            DebuggerMetadataCapability::OpaqueSourceSpanStep,
            Some(DebuggerCapability::StaticMetadataSourceSpanStep),
        ),
        (
            DebuggerMetadataCapability::OpaqueStaticScopeRelation,
            Some(DebuggerCapability::StaticScopeRelation),
        ),
        (DebuggerMetadataCapability::Unknown, None),
    ];
    for (capability, expected) in pairs {
        assert_eq!(
            capability.debugger_capability(),
            expected,
            "{capability:?} mapped unexpectedly"
        );
    }
}
