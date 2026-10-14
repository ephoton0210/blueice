// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

//! Internal validation cases for the generation-bound debugger registry.

use super::*;
use crate::parse;

fn source(name: &str, hash: &str) -> BlueJsSourceIdentity {
    BlueJsSourceIdentity::new(name, hash).unwrap()
}

fn script(text: &str) -> BlueJsProgramV1 {
    BlueJsProgramV1::Script(parse(text).unwrap())
}

#[test]
fn compiled_program_rejects_stale_and_unknown_locations_directly() {
    let mut registry = BlueJsProgramRegistry::default();
    let handle = registry
        .install(
            source("page:///validation.js", "sha256:validation"),
            &script("1;"),
        )
        .unwrap();
    let compiled = registry.get(handle).unwrap();
    let safe_point = compiled.safe_points().next().unwrap();
    let other_generation = BlueJsProgramGeneration(handle.generation().0 + 1);
    assert_eq!(
        compiled.validate_safe_point(BlueJsSafePoint {
            code_unit: BlueJsCodeUnitId {
                generation: other_generation,
                ordinal: safe_point.code_unit.ordinal,
            },
            ..safe_point
        }),
        Err(BlueJsProgramDebugError::StaleSafePoint)
    );
    assert_eq!(
        compiled.validate_safe_point(BlueJsSafePoint {
            code_unit: BlueJsCodeUnitId {
                generation: handle.generation(),
                ordinal: u32::MAX,
            },
            ..safe_point
        }),
        Err(BlueJsProgramDebugError::UnknownCodeUnit)
    );
    let node = compiled.ast_nodes()[0].id();
    let stale = BlueJsAstNodeId {
        generation: other_generation,
        ordinal: node.ordinal,
    };
    assert_eq!(
        compiled.validate_ast_node(stale),
        Err(BlueJsProgramDebugError::StaleAstNode)
    );
    assert_eq!(
        compiled.safe_point_for_ast_node(stale),
        Err(BlueJsProgramDebugError::StaleAstNode)
    );
}

#[test]
fn malformed_precompiled_ast_mapping_and_generation_exhaustion_fail_closed() {
    let mut registry = BlueJsProgramRegistry::default();
    let bytecode = script("1;").compile().unwrap();
    let node = AstNodeDescriptor {
        kind: BlueJsAstNodeKind::Statement,
        top_level_statement: true,
    };
    assert_eq!(
        registry.install_with_ast_nodes(
            source("page:///missing.js", "sha256:missing"),
            bytecode.clone(),
            vec![AstNodeDescriptor {
                top_level_statement: false,
                ..node
            }],
        ),
        Err(BlueJsProgramDebugError::AstBytecodeShapeMismatch)
    );
    let mut invalid_offset = bytecode;
    invalid_offset.root_statement_offsets[0] = Some(u32::MAX);
    assert_eq!(
        registry.install_with_ast_nodes(
            source("page:///offset.js", "sha256:offset"),
            invalid_offset,
            vec![node],
        ),
        Err(BlueJsProgramDebugError::AstBytecodeShapeMismatch)
    );
    assert!(registry.programs.is_empty());
    registry.next_generation = u64::MAX;
    assert_eq!(
        registry.install_precompiled(
            source("page:///overflow.js", "sha256:overflow"),
            script("2;").compile().unwrap(),
        ),
        Err(BlueJsProgramDebugError::GenerationExhausted)
    );

    let mut replacing = BlueJsProgramRegistry::default();
    let previous = replacing
        .install(source("page:///prior.js", "sha256:prior"), &script("1;"))
        .unwrap();
    replacing.next_generation = u64::MAX;
    assert_eq!(
        replacing.replace(
            previous,
            source("page:///replacement.js", "sha256:replacement"),
            &script("2;"),
        ),
        Err(BlueJsProgramDebugError::GenerationExhausted)
    );
    assert!(replacing.get(previous).is_ok());
}
