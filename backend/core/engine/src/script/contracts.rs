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

use super::host_typings::{HostBindingRoleV1, HostRuntimeBindingV1};
use blueice_bluets::{
    ContractPlan, ContractValue, SourceSpan, Type, ValidationError, ValidationLimits,
};
use blueice_ipc::page_host::{
    PAGE_HOST_DOCUMENT_ORIGIN_MAX_BYTES, PAGE_HOST_DOCUMENT_TEXT_MAX_BYTES,
};
use std::collections::BTreeMap;

pub const CORE_SCRIPT_DOCUMENT_TEXT_RESULT_CONTRACT_V1: &str =
    "core-script-document-text-result-v1";
pub const CORE_SCRIPT_DOCUMENT_ORIGIN_RESULT_CONTRACT_V1: &str =
    "core-script-document-origin-result-v1";

/// The direction in which one core binding's data crosses into a BlueJS realm.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum HostBindingContractDirection {
    HostToScript,
    ScriptToHost,
}

/// The owner that selects a boundary's contract before page code executes.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum HostBindingBoundaryOwner {
    CorePageRealm,
}

/// Whether the installed runtime path validates a value before realm capture.
/// A new boundary starts unchecked until its crossing is implemented and
/// tested; a reviewed type alone does not authorize an unchecked value.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum HostBindingBoundaryValidation {
    Unchecked,
    CheckedBeforeRealmCapture,
}

/// One reviewed ingress/egress inventory record. Source coordinates are
/// optional because the two immutable host snapshots are selected by core
/// before any source-level call; future call-site boundaries can retain their
/// original module and half-open byte range here. This core-only record is
/// never serialized into a source-free page report.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct HostBindingBoundaryRecordV1 {
    pub stable_binding_id: &'static str,
    pub runtime_binding_id: &'static str,
    pub owner: HostBindingBoundaryOwner,
    pub source_position: Option<SourceSpan>,
    pub direction: HostBindingContractDirection,
    pub contract_id: &'static str,
    pub declared_type: Type,
    pub validation_limits: ValidationLimits,
    pub validation: HostBindingBoundaryValidation,
    pub failure_category: &'static str,
    pub capability: &'static str,
}

/// Stable, distinct strict-runtime refusals for a live host value crossing.
/// Public script reports map them to fixed source-free categories.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum StrictRuntimeBoundaryDiagnostic {
    MissingContract { stable_binding_id: String },
    UnreifiableType { stable_binding_id: String },
    UncheckedBoundary { stable_binding_id: String },
}

/// Rejects a strict-runtime profile before the host installs or compiles any
/// value boundary that lacks a matching reviewed, reifiable, actively checked
/// contract. The record and installed profile are both core-selected; page
/// source cannot add a record or change its validation status.
pub fn validate_strict_runtime_boundary_inventory(
    bindings: &[HostRuntimeBindingV1],
    records: &[HostBindingBoundaryRecordV1],
) -> Result<(), StrictRuntimeBoundaryDiagnostic> {
    if let Err(error) = validate_host_binding_boundary_inventory(bindings, records) {
        let stable_binding_id = match error {
            HostBindingBoundaryInventoryError::MissingRecord(id)
            | HostBindingBoundaryInventoryError::DuplicateRecord(id)
            | HostBindingBoundaryInventoryError::MismatchedRecord(id) => id,
        };
        return Err(StrictRuntimeBoundaryDiagnostic::MissingContract { stable_binding_id });
    }
    for binding in bindings
        .iter()
        .filter(|binding| binding.role == HostBindingRoleV1::Value)
    {
        let Some(record) = records
            .iter()
            .find(|record| record.stable_binding_id == binding.stable_id)
        else {
            return Err(StrictRuntimeBoundaryDiagnostic::MissingContract {
                stable_binding_id: binding.stable_id.clone(),
            });
        };
        if record.runtime_binding_id != binding.runtime_binding_id
            || record.capability != binding.capability
            || record.contract_id.is_empty()
        {
            return Err(StrictRuntimeBoundaryDiagnostic::MissingContract {
                stable_binding_id: binding.stable_id.clone(),
            });
        }
        if ContractPlan::from_type(record.contract_id, &record.declared_type, &BTreeMap::new())
            .is_err()
        {
            return Err(StrictRuntimeBoundaryDiagnostic::UnreifiableType {
                stable_binding_id: binding.stable_id.clone(),
            });
        }
        if record.validation != HostBindingBoundaryValidation::CheckedBeforeRealmCapture {
            return Err(StrictRuntimeBoundaryDiagnostic::UncheckedBoundary {
                stable_binding_id: binding.stable_id.clone(),
            });
        }
    }
    Ok(())
}

/// An installed runtime value may not silently acquire an inferred contract.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum HostBindingBoundaryInventoryError {
    MissingRecord(String),
    DuplicateRecord(String),
    MismatchedRecord(String),
}

/// Requires an exact reviewed record for every installed runtime value in a
/// selected host profile. Type-only declarations are not runtime crossings.
/// The caller supplies the profile's actual generated runtime bindings, so a
/// newly installed value fails until the boundary inventory is extended.
pub fn validate_host_binding_boundary_inventory(
    bindings: &[HostRuntimeBindingV1],
    records: &[HostBindingBoundaryRecordV1],
) -> Result<(), HostBindingBoundaryInventoryError> {
    let mut by_id = BTreeMap::new();
    for record in records {
        if by_id.insert(record.stable_binding_id, record).is_some() {
            return Err(HostBindingBoundaryInventoryError::DuplicateRecord(
                record.stable_binding_id.to_string(),
            ));
        }
    }
    for binding in bindings
        .iter()
        .filter(|binding| binding.role == HostBindingRoleV1::Value)
    {
        let Some(record) = by_id.get(binding.stable_id.as_str()) else {
            return Err(HostBindingBoundaryInventoryError::MissingRecord(
                binding.stable_id.clone(),
            ));
        };
        if record.runtime_binding_id != binding.runtime_binding_id
            || record.capability != binding.capability
            || record.contract_id.is_empty()
            || record.failure_category.is_empty()
        {
            return Err(HostBindingBoundaryInventoryError::MismatchedRecord(
                binding.stable_id.clone(),
            ));
        }
    }
    Ok(())
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
            document_text: ValidationLimits {
                max_string_bytes: PAGE_HOST_DOCUMENT_TEXT_MAX_BYTES,
                ..ValidationLimits::default()
            },
            document_origin: ValidationLimits {
                max_string_bytes: PAGE_HOST_DOCUMENT_ORIGIN_MAX_BYTES,
                ..ValidationLimits::default()
            },
        }
    }
}

/// Builds the current core-owned boundary records using the owner's actual
/// validation budgets. Only installed bindings are listed; a future boundary
/// must supply its own reviewed record before E1.2.2 can admit it.
pub fn core_script_binding_boundary_records(
    limits: CoreScriptBindingContractLimits,
) -> [HostBindingBoundaryRecordV1; 2] {
    let [origin, text] = core_script_binding_contracts();
    let record = |boundary: HostBindingContractV1, validation_limits| HostBindingBoundaryRecordV1 {
        stable_binding_id: boundary.stable_binding_id,
        runtime_binding_id: boundary.runtime_binding_id,
        owner: HostBindingBoundaryOwner::CorePageRealm,
        source_position: None,
        direction: boundary.direction,
        contract_id: boundary.contract_id,
        declared_type: Type::String,
        validation_limits,
        validation: HostBindingBoundaryValidation::CheckedBeforeRealmCapture,
        failure_category: "host binding contract rejected the page script",
        capability: boundary.capability,
    };
    [
        record(origin, limits.document_origin),
        record(text, limits.document_text),
    ]
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
    use crate::script::host_typings::{
        core_script_host_type_catalog, CORE_SCRIPT_DOCUMENT_CONTEXT_PROFILE_V1,
        CORE_SCRIPT_DOCUMENT_TEXT_PROFILE_V1,
    };

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

    #[test]
    fn inventory_records_name_owner_position_policy_and_actual_limits() {
        let mut limits = CoreScriptBindingContractLimits::default();
        limits.document_text.max_string_bytes = 17;
        let [origin, text] = core_script_binding_boundary_records(limits);
        assert_eq!(origin.stable_binding_id, "dom.document-origin");
        assert_eq!(
            origin.contract_id,
            CORE_SCRIPT_DOCUMENT_ORIGIN_RESULT_CONTRACT_V1
        );
        assert_eq!(origin.validation_limits, limits.document_origin);
        assert_eq!(text.stable_binding_id, "dom.document-text");
        assert_eq!(text.runtime_binding_id, "global.blueiceDocumentText");
        assert_eq!(
            text.contract_id,
            CORE_SCRIPT_DOCUMENT_TEXT_RESULT_CONTRACT_V1
        );
        assert_eq!(text.validation_limits.max_string_bytes, 17);
        for record in [origin, text] {
            assert_eq!(record.owner, HostBindingBoundaryOwner::CorePageRealm);
            assert_eq!(record.source_position, None);
            assert_eq!(record.direction, HostBindingContractDirection::HostToScript);
            assert_eq!(
                record.failure_category,
                "host binding contract rejected the page script"
            );
            assert_eq!(record.capability, "dom-read");
        }
    }

    #[test]
    fn installed_snapshot_value_without_a_reviewed_record_fails_inventory_gate() {
        let catalog = core_script_host_type_catalog();
        let records =
            core_script_binding_boundary_records(CoreScriptBindingContractLimits::default());
        for profile in [
            CORE_SCRIPT_DOCUMENT_TEXT_PROFILE_V1,
            CORE_SCRIPT_DOCUMENT_CONTEXT_PROFILE_V1,
        ] {
            let artifact = catalog.generate(profile).unwrap();
            assert_eq!(
                validate_host_binding_boundary_inventory(&artifact.runtime_bindings, &records),
                Ok(())
            );
        }
        let context = catalog
            .generate(CORE_SCRIPT_DOCUMENT_CONTEXT_PROFILE_V1)
            .unwrap();
        assert_eq!(
            validate_host_binding_boundary_inventory(&context.runtime_bindings, &records[1..]),
            Err(HostBindingBoundaryInventoryError::MissingRecord(
                "dom.document-origin".into()
            ))
        );
        let mut new_value = context.runtime_bindings.clone();
        new_value.push(HostRuntimeBindingV1 {
            stable_id: "dom.future-value".into(),
            role: HostBindingRoleV1::Value,
            runtime_binding_id: "global.futureValue".into(),
            capability: "dom-read".into(),
            feature_flag: "future-value".into(),
            first_host_api_version: "blueice-core-script-v1".into(),
        });
        assert_eq!(
            validate_host_binding_boundary_inventory(&new_value, &records),
            Err(HostBindingBoundaryInventoryError::MissingRecord(
                "dom.future-value".into()
            ))
        );
    }
}
