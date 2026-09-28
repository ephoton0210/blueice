// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

//! Existing registry and debugger location tests.

use super::*;
use crate::{parse, BlueJsProgramV1, Value, Vm};

fn source(name: &str, hash: &str) -> BlueJsSourceIdentity {
    BlueJsSourceIdentity::new(name, hash).unwrap()
}

fn script(text: &str) -> BlueJsProgramV1 {
    BlueJsProgramV1::Script(parse(text).unwrap())
}

#[test]
fn registry_exposes_and_validates_exact_instruction_boundaries() {
    let mut registry = BlueJsProgramRegistry::default();
    let handle = registry
        .install(source("page:///main.js", "sha256:one"), &script("1 + 2"))
        .unwrap();
    let compiled = registry.get(handle).unwrap();
    assert_eq!(compiled.source().canonical_module_id(), "page:///main.js");
    assert_eq!(compiled.source().source_hash(), "sha256:one");
    assert_eq!(
        Vm::default().execute(compiled.bytecode()).unwrap(),
        Value::Number(3.0)
    );
    let safe_point = compiled.safe_points().next().unwrap();
    registry.validate_safe_point(handle, safe_point).unwrap();
    assert_eq!(safe_point.code_unit.generation(), handle.generation());
    assert_eq!(safe_point.code_unit.ordinal(), 0);

    let malformed = BlueJsSafePoint {
        code_unit: safe_point.code_unit,
        bytecode_offset: safe_point.bytecode_offset + 1,
    };
    assert_eq!(
        registry.validate_safe_point(handle, malformed),
        Err(BlueJsProgramDebugError::InvalidInstructionBoundary)
    );
}

#[test]
fn nested_function_code_units_are_named_in_preorder() {
    let mut registry = BlueJsProgramRegistry::default();
    let handle = registry
        .install(
            source("page:///nested.js", "sha256:nested"),
            &script("function answer(){return 42;} answer();"),
        )
        .unwrap();
    let compiled = registry.get(handle).unwrap();
    assert_eq!(compiled.code_units().len(), 2);
    assert_eq!(compiled.code_units()[0].id().ordinal(), 0);
    assert_eq!(compiled.code_units()[1].id().ordinal(), 1);
    let nested_safe_point = BlueJsSafePoint {
        code_unit: compiled.code_units()[1].id(),
        bytecode_offset: compiled.code_units()[1].instruction_offsets()[0],
    };
    registry
        .validate_safe_point(handle, nested_safe_point)
        .unwrap();
}

#[test]
fn installed_bytecode_carries_exact_ordinals_for_duplicate_shaped_closures() {
    let program =
        script("function first(){return 1;} function second(){return 1;} first() + second();");
    let bare = program.compile().unwrap();
    assert_eq!(bare.debugger_code_unit_ordinal, None);
    assert_eq!(bare.debugger_program_generation, None);
    assert!(bare
        .child_code_units()
        .all(|child| child.debugger_code_unit_ordinal.is_none()));
    let mut registry = BlueJsProgramRegistry::default();
    let precompiled = registry
        .install_precompiled(source("page:///duplicate.js", "sha256:precompiled"), bare)
        .unwrap();
    let recompiled = registry
        .install(
            source("page:///duplicate.js", "sha256:recompiled"),
            &program,
        )
        .unwrap();
    assert_ne!(precompiled.generation(), recompiled.generation());
    for handle in [precompiled, recompiled] {
        let compiled = registry.get(handle).unwrap();
        let code = compiled.bytecode();
        let children = code.child_code_units().collect::<Vec<_>>();
        assert_eq!(compiled.code_units().len(), 3);
        assert_eq!(code.debugger_code_unit_ordinal, Some(0));
        assert_eq!(
            code.debugger_program_generation,
            Some(handle.generation().0)
        );
        assert_eq!(children[0].bytes(), children[1].bytes());
        assert_eq!(children[0].debugger_code_unit_ordinal, Some(1));
        assert_eq!(children[1].debugger_code_unit_ordinal, Some(2));
        assert_eq!(
            children[0].debugger_program_generation,
            Some(handle.generation().0)
        );
        assert_eq!(
            children[1].debugger_program_generation,
            Some(handle.generation().0)
        );
        for (ordinal, unit) in compiled.code_units().iter().enumerate() {
            assert_eq!(unit.id().generation(), handle.generation());
            assert_eq!(unit.id().ordinal(), ordinal as u32);
        }
        assert_eq!(
            code.clone().functions[1].debugger_code_unit_ordinal,
            Some(2)
        );
        assert_eq!(Vm::default().execute(code).unwrap(), Value::Number(2.0));
    }
    let deep = registry
        .install(
            source("page:///deep.js", "sha256:deep"),
            &script("function outer(){function inner(){return 3;} return inner();} outer();"),
        )
        .unwrap();
    let deep_code = registry.get(deep).unwrap().bytecode();
    assert_eq!(deep_code.debugger_code_unit_ordinal, Some(0));
    let outer = deep_code.child_code_units().next().unwrap();
    assert_eq!(outer.debugger_code_unit_ordinal, Some(1));
    let inner = outer.child_code_units().next().unwrap();
    assert_eq!(inner.debugger_code_unit_ordinal, Some(2));
    assert_eq!(
        Vm::default().execute(deep_code).unwrap(),
        Value::Number(3.0)
    );
}

#[test]
fn replacing_or_invalidating_a_program_fails_closed_for_old_ids() {
    let mut registry = BlueJsProgramRegistry::default();
    let first = registry
        .install(source("page:///main.js", "sha256:old"), &script("1"))
        .unwrap();
    let stale_safe_point = registry.get(first).unwrap().safe_points().next().unwrap();
    let stale_ast_node = registry.get(first).unwrap().ast_nodes()[0].id();
    let second = registry
        .replace(first, source("page:///main.js", "sha256:new"), &script("2"))
        .unwrap();
    assert_ne!(first.generation(), second.generation());
    assert!(matches!(
        registry.get(first),
        Err(BlueJsProgramDebugError::UnknownProgram)
    ));
    assert_eq!(
        registry.validate_safe_point(first, stale_safe_point),
        Err(BlueJsProgramDebugError::UnknownProgram)
    );
    assert_eq!(
        registry.validate_safe_point(second, stale_safe_point),
        Err(BlueJsProgramDebugError::StaleSafePoint)
    );
    assert_eq!(
        registry.validate_ast_node(first, stale_ast_node),
        Err(BlueJsProgramDebugError::UnknownProgram)
    );
    assert_eq!(
        registry.validate_ast_node(second, stale_ast_node),
        Err(BlueJsProgramDebugError::StaleAstNode)
    );
    assert!(registry.invalidate(second));
    assert!(!registry.invalidate(second));
}

#[test]
fn failed_replacement_preserves_the_previous_generation() {
    let mut registry = BlueJsProgramRegistry::default();
    let handle = registry
        .install(source("page:///main.js", "sha256:old"), &script("1"))
        .unwrap();
    let invalid = script("let duplicate; let duplicate;");
    assert!(matches!(
        registry.replace(handle, source("page:///main.js", "sha256:bad"), &invalid),
        Err(BlueJsProgramDebugError::Compilation(
            CompileError::DuplicateBinding(_)
        ))
    ));
    assert_eq!(
        Vm::default()
            .execute(registry.get(handle).unwrap().bytecode())
            .unwrap(),
        Value::Number(1.0)
    );
}

#[test]
fn source_identity_rejects_missing_required_fields() {
    assert_eq!(
        BlueJsSourceIdentity::new("", "sha256:value"),
        Err(BlueJsProgramDebugError::EmptyCanonicalModuleId)
    );
    assert_eq!(
        BlueJsSourceIdentity::new("page:///main.js", ""),
        Err(BlueJsProgramDebugError::EmptySourceHash)
    );
}

#[test]
fn structured_programs_expose_and_validate_executable_ast_nodes() {
    let mut registry = BlueJsProgramRegistry::default();
    let handle = registry
        .install(
            source("page:///main.js", "sha256:ast"),
            &script("let value=1+2; value;"),
        )
        .unwrap();
    let compiled = registry.get(handle).unwrap();
    assert_eq!(compiled.ast_nodes()[0].kind(), BlueJsAstNodeKind::Script);
    assert_eq!(compiled.ast_nodes()[0].id().ordinal(), 0);
    let expression = compiled
        .ast_nodes()
        .iter()
        .find(|node| node.kind() == BlueJsAstNodeKind::Expression)
        .copied()
        .unwrap();
    registry.validate_ast_node(handle, expression.id()).unwrap();
    let statement = compiled
        .ast_nodes()
        .iter()
        .find(|node| node.is_top_level_statement())
        .copied()
        .unwrap();
    let safe_point = registry
        .safe_point_for_ast_node(handle, statement.id())
        .unwrap();
    registry.validate_safe_point(handle, safe_point).unwrap();

    let malformed = BlueJsAstNodeId {
        generation: handle.generation(),
        ordinal: u32::MAX,
    };
    assert_eq!(
        registry.validate_ast_node(handle, malformed),
        Err(BlueJsProgramDebugError::UnknownAstNode)
    );
}

#[test]
fn empty_root_statement_is_explicitly_unbound() {
    let mut registry = BlueJsProgramRegistry::default();
    let handle = registry
        .install(source("page:///empty.js", "sha256:empty"), &script("; 42;"))
        .unwrap();
    let compiled = registry.get(handle).unwrap();
    let top_level_statements = compiled
        .ast_nodes()
        .iter()
        .filter(|node| node.is_top_level_statement())
        .copied()
        .collect::<Vec<_>>();
    assert_eq!(top_level_statements.len(), 2);
    assert_eq!(
        registry.safe_point_for_ast_node(handle, top_level_statements[0].id()),
        Err(BlueJsProgramDebugError::AstNodeUnbound)
    );
    let safe_point = registry
        .safe_point_for_ast_node(handle, top_level_statements[1].id())
        .unwrap();
    registry.validate_safe_point(handle, safe_point).unwrap();
}

#[test]
fn precompiled_bytecode_uses_the_same_generation_bound_validation() {
    let program = script("40 + 2");
    let bytecode = program.compile().unwrap();
    let mut registry = BlueJsProgramRegistry::default();
    let handle = registry
        .install_precompiled(source("page:///main.js", "sha256:compiled"), bytecode)
        .unwrap();
    let compiled = registry.get(handle).unwrap();
    assert_eq!(
        Vm::default().execute(compiled.bytecode()).unwrap(),
        Value::Number(42.0)
    );
    registry
        .validate_safe_point(handle, compiled.safe_points().next().unwrap())
        .unwrap();
}
