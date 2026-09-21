// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

//! First live contract inventory for core's direct BlueTS page bindings.
//!
//! The only currently installed BlueJS host callbacks copy one document text
//! snapshot and one canonical origin into a realm. This module names those
//! host-to-script boundaries and validates their primitive result before a
//! callback captures it. It does not claim contracts for absent DOM, Fetch,
//! storage, messaging, JSON, or extension APIs.

use blueice_bluets::{ContractPlan, ContractValue, Type, ValidationError, ValidationLimits};
use std::collections::BTreeMap;

pub const CORE_SCRIPT_DOCUMENT_TEXT_RESULT_CONTRACT_V1: &str =
    "core-script-document-text-result-v1";
pub const CORE_SCRIPT_DOCUMENT_ORIGIN_RESULT_CONTRACT_V1: &str =
    "core-script-document-origin-result-v1";

/// The direction in which one core binding's data crosses into a BlueJS realm.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum HostBindingContractDirection {
    HostToScript,
}

/// A named, reifiable live binding boundary. `stable_binding_id` must be the
/// same schema identity used by `HostTypeSurfaceV1`; this keeps a contract from
/// being attached to a similarly shaped but unrelated host function.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct HostBindingContractV1 {
    pub stable_binding_id: &'static str,
    pub contract_id: &'static str,
    pub runtime_binding_id: &'static str,
    pub direction: HostBindingContractDirection,
    pub capability: &'static str,
}

impl HostBindingContractV1 {
    /// Creates the exact immutable plan used for this primitive callback
    /// result. The contract is data-only and invokes no JavaScript callback,
    /// getter, proxy, host operation, or capability acquisition.
    pub fn plan(self) -> ContractPlan {
        ContractPlan::from_type(self.contract_id, &Type::String, &BTreeMap::new())
            .expect("a primitive string contract is always reifiable")
    }

    /// Validates a copied host result before it is captured by a realm-local
    /// callback. The caller retains the original snapshot only after this
    /// bounded, pure validation completes.
    pub fn validate_string(
        self,
        value: &str,
        limits: ValidationLimits,
    ) -> Result<(), ValidationError> {
        self.plan()
            .validate_with_limits(&ContractValue::String(value.to_string()), limits)
    }
}

/// Per-boundary validation budgets. A caller can select tighter budgets when
/// constructing a page host, but cannot loosen them from a page request.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct CoreScriptBindingContractLimits {
    pub document_text: ValidationLimits,
    pub document_origin: ValidationLimits,
}

impl Default for CoreScriptBindingContractLimits {
    fn default() -> Self {
        Self {
            document_text: ValidationLimits::default(),
            document_origin: ValidationLimits {
                max_string_bytes: 4 * 1_024,
                ..ValidationLimits::default()
            },
        }
    }
}

/// Returns the complete current inventory. The order is stable by binding ID
/// and contains no speculative future boundary.
pub fn core_script_binding_contracts() -> [HostBindingContractV1; 2] {
    [
        HostBindingContractV1 {
            stable_binding_id: "dom.document-origin",
            contract_id: CORE_SCRIPT_DOCUMENT_ORIGIN_RESULT_CONTRACT_V1,
            runtime_binding_id: "global.blueiceDocumentOrigin",
            direction: HostBindingContractDirection::HostToScript,
            capability: "dom-read",
        },
        HostBindingContractV1 {
            stable_binding_id: "dom.document-text",
            contract_id: CORE_SCRIPT_DOCUMENT_TEXT_RESULT_CONTRACT_V1,
            runtime_binding_id: "global.blueiceDocumentText",
            direction: HostBindingContractDirection::HostToScript,
            capability: "dom-read",
        },
    ]
}

/// Resolves one known core binding contract. An absent binding has no inferred
/// fallback contract and must not be admitted by a host caller.
pub fn core_script_binding_contract(binding_id: &str) -> Option<HostBindingContractV1> {
    core_script_binding_contracts()
        .into_iter()
        .find(|boundary| boundary.stable_binding_id == binding_id)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn inventory_names_only_installed_string_result_boundaries() {
        let boundaries = core_script_binding_contracts();
        assert_eq!(
            boundaries
                .iter()
                .map(|boundary| boundary.stable_binding_id)
                .collect::<Vec<_>>(),
            vec!["dom.document-origin", "dom.document-text"]
        );
        assert!(boundaries.iter().all(|boundary| {
            boundary.direction == HostBindingContractDirection::HostToScript
                && boundary.capability == "dom-read"
                && boundary.plan().root == blueice_bluets::Contract::String
        }));
    }

    #[test]
    fn string_result_validation_is_pure_and_bounded() {
        let boundary = core_script_binding_contract("dom.document-text").unwrap();
        assert!(boundary
            .validate_string("safe", ValidationLimits::default())
            .is_ok());
        let error = boundary
            .validate_string(
                "oversized",
                ValidationLimits {
                    max_string_bytes: 3,
                    ..ValidationLimits::default()
                },
            )
            .unwrap_err();
        assert_eq!(error.path, "$");
        assert_eq!(error.observed, "string of 9 bytes");
    }
}
