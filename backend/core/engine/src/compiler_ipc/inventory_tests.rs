// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

// Focused coverage for the compiler service's opaque static-ID inventory.

use super::*;

#[test]
fn metadata_inventory_clamps_pages_and_rejects_malformed_or_replayed_cursors() {
    let mut adapter = CompilerServiceIpcAdapter::new(
        RegisteredProjectCompilerService::default(),
        CompilerServiceIpcLimits {
            max_static_metadata_page_entries: 1,
            ..CompilerServiceIpcLimits::default()
        },
    )
    .unwrap();
    let project = adapter
        .register_core_project(registration("export const value: number = answer;"))
        .unwrap();
    let CompilerReply::Check(check) = adapter.handle(CompilerRequest::Check { project }) else {
        panic!("registered project must check through adapter")
    };
    let CompilerReply::StaticMetadataPage(page) =
        adapter.handle(CompilerRequest::ListStaticMetadata {
            generation: check.generation,
            kind: CompilerStaticMetadataKind::Symbols,
            cursor: None,
            limit: Some(999),
        })
    else {
        panic!("bounded inventory must return a page")
    };
    assert_eq!(page.ids.len(), 1, "caller limit must not raise core cap");
    let cursor = page.next_cursor.expect("fixture needs a second page");
    assert!(matches!(
        adapter.handle(CompilerRequest::ListStaticMetadata {
            generation: check.generation,
            kind: CompilerStaticMetadataKind::Symbols,
            cursor: Some(CompilerStaticMetadataCursor { id: 0 }),
            limit: Some(1),
        }),
        CompilerReply::Error {
            code: CompilerErrorCode::InvalidMetadataCursor,
            ..
        }
    ));
    assert!(matches!(
        adapter.handle(CompilerRequest::ListStaticMetadata {
            generation: check.generation,
            kind: CompilerStaticMetadataKind::Symbols,
            cursor: Some(cursor),
            limit: Some(0),
        }),
        CompilerReply::Error {
            code: CompilerErrorCode::InvalidMetadataPage,
            ..
        }
    ));
    assert!(matches!(
        adapter.handle(CompilerRequest::ListStaticMetadata {
            generation: check.generation,
            kind: CompilerStaticMetadataKind::Symbols,
            cursor: Some(cursor),
            limit: Some(1),
        }),
        CompilerReply::StaticMetadataPage(_)
    ));
    assert!(matches!(
        adapter.handle(CompilerRequest::ListStaticMetadata {
            generation: check.generation,
            kind: CompilerStaticMetadataKind::Symbols,
            cursor: Some(cursor),
            limit: Some(1),
        }),
        CompilerReply::Error {
            code: CompilerErrorCode::InvalidMetadataCursor,
            ..
        }
    ));
}

#[test]
fn adapter_exposes_only_exact_generation_contracts_and_source_hashes() {
    let mut adapter = CompilerServiceIpcAdapter::default();
    let project = adapter
        .register_core_project(registration(
            "interface Settings { enabled: boolean; } \
             export const settings: Settings = { enabled: true };",
        ))
        .unwrap();
    let CompilerReply::Check(check) = adapter.handle(CompilerRequest::Check { project }) else {
        panic!("registered project must check through adapter")
    };
    assert_eq!(check.static_metadata.as_ref().unwrap().contract_count, 1);

    // Discover the symbol IDs from the check generation instead of assuming
    // an ordinal. A one-shot cursor cannot switch collections; the rejected
    // request leaves the valid symbols cursor usable.
    let CompilerReply::StaticMetadataPage(first_page) =
        adapter.handle(CompilerRequest::ListStaticMetadata {
            generation: check.generation,
            kind: CompilerStaticMetadataKind::Symbols,
            cursor: None,
            limit: Some(1),
        })
    else {
        panic!("successful metadata must expose a bounded symbols inventory")
    };
    assert_eq!(first_page.ids.len(), 1);
    let first_cursor = first_page
        .next_cursor
        .expect("fixture must require a symbols continuation");
    assert!(matches!(
        adapter.handle(CompilerRequest::ListStaticMetadata {
            generation: check.generation,
            kind: CompilerStaticMetadataKind::Contracts,
            cursor: Some(first_cursor),
            limit: Some(1),
        }),
        CompilerReply::Error {
            code: CompilerErrorCode::InvalidMetadataCursor,
            ..
        }
    ));
    let mut symbol_ids = first_page.ids;
    let mut cursor = Some(first_cursor);
    while let Some(next_cursor) = cursor {
        let CompilerReply::StaticMetadataPage(page) =
            adapter.handle(CompilerRequest::ListStaticMetadata {
                generation: check.generation,
                kind: CompilerStaticMetadataKind::Symbols,
                cursor: Some(next_cursor),
                limit: Some(1),
            })
        else {
            panic!("valid symbol inventory cursor must continue exactly once")
        };
        symbol_ids.extend(page.ids);
        cursor = page.next_cursor;
    }
    assert_eq!(
        u32::try_from(symbol_ids.len()).unwrap(),
        check.static_metadata.as_ref().unwrap().symbol_count
    );
    let symbol = symbol_ids
        .into_iter()
        .find_map(|symbol_id| {
            match adapter.handle(CompilerRequest::GetStaticSymbol {
                generation: check.generation,
                symbol_id,
            }) {
                CompilerReply::StaticSymbol(symbol) if symbol.contract_id.is_some() => Some(symbol),
                _ => None,
            }
        })
        .expect("a discovered local interface must carry a contract ID");
    let contract_id = symbol
        .contract_id
        .expect("local interface must carry contract id");
    assert_ne!(symbol.source_id, u32::MAX);

    let CompilerReply::StaticProvenance(provenance) =
        adapter.handle(CompilerRequest::GetStaticProvenance {
            generation: check.generation,
            source_id: symbol.source_id,
        })
    else {
        panic!("symbol source ID must resolve to source-free provenance")
    };
    assert_eq!(provenance.source_id, symbol.source_id);
    assert_ne!(provenance.content_hash, "Settings");
    assert!(provenance.content_hash.starts_with("bts-sha256:"));
    assert_eq!(provenance.content_hash.len(), "bts-sha256:".len() + 64);

    let CompilerReply::StaticContract(contract) =
        adapter.handle(CompilerRequest::GetStaticContract {
            generation: check.generation,
            contract_id,
        })
    else {
        panic!("symbol contract ID must resolve exactly")
    };
    assert_eq!(contract.contract_id, contract_id);
    assert!(contract.root.contains("Reference"));

    let secret = "caller-supplied-secret-never-echoed";
    let CompilerReply::ContractValidation(validation) =
        adapter.handle(CompilerRequest::ValidateStaticContract {
            generation: check.generation,
            contract_id,
            value: CompilerContractValue::Object(std::collections::BTreeMap::from([(
                "enabled".to_string(),
                CompilerContractValue::String(secret.to_string()),
            )])),
        })
    else {
        panic!("invalid data-only snapshot is a validation result")
    };
    assert!(!validation.valid);
    let reply_text = format!("{validation:?}");
    assert!(!reply_text.contains(secret));
    assert!(validation.failure.is_some());

    let CompilerReply::Check(next) = adapter.handle(CompilerRequest::Check { project }) else {
        panic!("later check must succeed")
    };
    assert!(matches!(
        adapter.handle(CompilerRequest::GetStaticContract {
            generation: check.generation,
            contract_id,
        }),
        CompilerReply::Error {
            code: CompilerErrorCode::StaleGeneration,
            ..
        }
    ));
    assert!(matches!(
        adapter.handle(CompilerRequest::GetStaticProvenance {
            generation: next.generation,
            source_id: u32::MAX,
        }),
        CompilerReply::Error {
            code: CompilerErrorCode::UnknownSource,
            ..
        }
    ));
    assert!(matches!(
        adapter.handle(CompilerRequest::GetStaticContract {
            generation: next.generation,
            contract_id: u32::MAX,
        }),
        CompilerReply::Error {
            code: CompilerErrorCode::UnknownContract,
            ..
        }
    ));
}
