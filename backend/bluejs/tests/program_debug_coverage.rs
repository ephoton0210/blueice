// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

use blueice_bluejs::{
    parse, BlueJsAstNodeKind, BlueJsProgramDebugError, BlueJsProgramRegistry, BlueJsProgramV1,
    BlueJsSourceIdentity, CompileError,
};

#[test]
fn program_registry_errors_have_stable_host_facing_diagnostics() {
    use BlueJsProgramDebugError as Error;

    for (error, expected) in [
        (
            Error::EmptyCanonicalModuleId,
            "canonical module ID must not be empty",
        ),
        (Error::EmptySourceHash, "source hash must not be empty"),
        (
            Error::Compilation(CompileError::DuplicateBinding("x".into())),
            "cannot compile structured program: duplicate or conflicting binding: x",
        ),
        (
            Error::GenerationExhausted,
            "BlueJS program generation space is exhausted",
        ),
        (
            Error::CodeUnitLimitExceeded,
            "BlueJS program has too many code units",
        ),
        (
            Error::AstNodeLimitExceeded,
            "BlueJS program has too many AST nodes",
        ),
        (
            Error::UnknownProgram,
            "BlueJS program handle is stale or unknown",
        ),
        (
            Error::StaleSafePoint,
            "BlueJS safe point belongs to another generation",
        ),
        (
            Error::UnknownCodeUnit,
            "BlueJS code unit is unknown for this generation",
        ),
        (
            Error::InvalidInstructionBoundary,
            "BlueJS offset is not an instruction boundary",
        ),
        (
            Error::StaleAstNode,
            "BlueJS AST node belongs to another generation",
        ),
        (
            Error::UnknownAstNode,
            "BlueJS AST node is unknown for this generation",
        ),
        (
            Error::AstNodeUnbound,
            "BlueJS AST node has no executable root safe point",
        ),
        (
            Error::AstBytecodeShapeMismatch,
            "BlueJS AST and bytecode top-level provenance disagree",
        ),
    ] {
        assert_eq!(error.to_string(), expected);
    }
}

#[test]
fn failed_initial_install_does_not_consume_a_program_generation() {
    let source = || BlueJsSourceIdentity::new("page:///debug.js", "sha256:debug").unwrap();
    let mut registry = BlueJsProgramRegistry::default();
    let invalid = BlueJsProgramV1::Script(parse("let duplicate; let duplicate;").unwrap());
    assert!(matches!(
        registry.install(source(), &invalid),
        Err(BlueJsProgramDebugError::Compilation(
            CompileError::DuplicateBinding(_)
        ))
    ));

    let valid = BlueJsProgramV1::Script(parse("42;").unwrap());
    let handle = registry.install(source(), &valid).unwrap();
    assert_eq!(handle.generation().as_u64(), 1);
}

#[test]
fn invalidated_programs_reject_resolution_and_replacement() {
    let source = || BlueJsSourceIdentity::new("page:///debug.js", "sha256:debug").unwrap();
    let program = BlueJsProgramV1::Script(parse("42;").unwrap());
    let mut registry = BlueJsProgramRegistry::default();
    let handle = registry.install(source(), &program).unwrap();
    let node = registry.get(handle).unwrap().ast_nodes()[0].id();
    assert!(registry.invalidate(handle));
    assert_eq!(
        registry.safe_point_for_ast_node(handle, node),
        Err(BlueJsProgramDebugError::UnknownProgram)
    );
    assert_eq!(
        registry.replace(handle, source(), &program),
        Err(BlueJsProgramDebugError::UnknownProgram)
    );
}

#[test]
fn a_live_program_exposes_its_generation_source_and_executable_inventory() {
    let source = BlueJsSourceIdentity::new("page:///debug.js", "sha256:debug").unwrap();
    let program = BlueJsProgramV1::Script(parse("let answer = 42; answer;").unwrap());
    let mut registry = BlueJsProgramRegistry::default();
    let handle = registry.install(source.clone(), &program).unwrap();
    let compiled = registry.get(handle).unwrap();

    assert_eq!(compiled.handle(), handle);
    assert_eq!(handle.generation().as_u64(), 1);
    assert_eq!(compiled.source(), &source);
    assert_eq!(compiled.source().source_hash(), "sha256:debug");
    assert_eq!(compiled.source().canonical_module_id(), "page:///debug.js");

    let root = compiled.ast_nodes()[0];
    assert_eq!(root.id().generation(), handle.generation());
    assert_eq!(root.id().ordinal(), 0);
    assert_eq!(root.kind(), BlueJsAstNodeKind::Script);
    assert!(!root.is_top_level_statement());
    let statements: Vec<_> = compiled
        .ast_nodes()
        .iter()
        .filter(|node| node.is_top_level_statement())
        .collect();
    assert_eq!(statements.len(), 2);

    let code_unit = &compiled.code_units()[0];
    assert_eq!(code_unit.id().generation(), handle.generation());
    assert_eq!(code_unit.id().ordinal(), 0);
    assert!(!code_unit.instruction_offsets().is_empty());
    let point = compiled
        .safe_point_for_ast_node(statements[0].id())
        .unwrap();
    assert_eq!(point.code_unit, code_unit.id());
    assert!(code_unit
        .instruction_offsets()
        .contains(&point.bytecode_offset));
    assert_eq!(
        registry.validate_ast_node(handle, statements[0].id()),
        Ok(())
    );
    assert_eq!(registry.validate_safe_point(handle, point), Ok(()));
}

#[test]
fn replacing_a_program_invalidates_its_old_generation_and_ast_nodes() {
    let mut registry = BlueJsProgramRegistry::default();
    let first = registry
        .install(
            BlueJsSourceIdentity::new("page:///first.js", "sha256:first").unwrap(),
            &BlueJsProgramV1::Script(parse("1;").unwrap()),
        )
        .unwrap();
    let old_node = registry.get(first).unwrap().ast_nodes()[0].id();
    let replacement = registry
        .replace(
            first,
            BlueJsSourceIdentity::new("page:///second.js", "sha256:second").unwrap(),
            &BlueJsProgramV1::Script(parse("2;").unwrap()),
        )
        .unwrap();

    assert_eq!(replacement.generation().as_u64(), 2);
    assert!(matches!(
        registry.get(first),
        Err(BlueJsProgramDebugError::UnknownProgram)
    ));
    assert_eq!(
        registry.validate_ast_node(replacement, old_node),
        Err(BlueJsProgramDebugError::StaleAstNode)
    );
    assert_eq!(
        registry.safe_point_for_ast_node(replacement, old_node),
        Err(BlueJsProgramDebugError::StaleAstNode)
    );
    assert!(!registry.invalidate(first));
    assert!(registry.invalidate(replacement));
}
