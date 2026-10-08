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
fn root_declaration_slots_follow_structural_statements_not_shadowed_names() {
    let code = script("let value=1; function read(){return value;} { let value=2; } value;")
        .compile()
        .unwrap();
    let slots = code.root_declaration_binding_slots();
    assert_eq!(slots.len(), 4);
    let value = slots[0].expect("root variable has a slot");
    let read = slots[1].expect("root function has a slot");
    assert_ne!(value, read);
    assert_eq!(code.bindings[value as usize].name, "value");
    assert_eq!(code.bindings[read as usize].name, "read");
    assert_eq!(&slots[2..], &[None, None]);
    assert!(code.scopes[0].contains(&value));
    assert!(code.scopes[0].contains(&read));
}

#[test]
fn root_declaration_slots_cover_modules_and_leave_non_declarations_unbound() {
    let module = BlueJsProgramV1::Module(
        crate::parse_module("export const answer=42; function read(){return answer;} read();")
            .unwrap(),
    );
    let code = module.compile().unwrap();
    let slots = code.root_declaration_binding_slots();
    assert_eq!(slots.len(), 3);
    assert_eq!(code.bindings[slots[0].unwrap() as usize].name, "answer");
    assert_eq!(code.bindings[slots[1].unwrap() as usize].name, "read");
    assert_eq!(slots[2], None);
}

#[test]
fn compound_declarations_have_no_single_slot_and_duplicate_vars_share_one() {
    let code = script("let {x}=input; let first=1, second=2; var repeated=1; var repeated=2;")
        .compile()
        .unwrap();
    let slots = code.root_declaration_binding_slots();
    assert_eq!(&slots[..2], &[None, None]);
    assert!(slots[2].is_some());
    assert_eq!(slots[2], slots[3]);
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

#[test]
fn every_error_has_a_distinct_readable_message() {
    let cases = [
        (
            BlueJsProgramDebugError::EmptyCanonicalModuleId,
            "canonical module ID must not be empty",
        ),
        (
            BlueJsProgramDebugError::EmptySourceHash,
            "source hash must not be empty",
        ),
        (
            BlueJsProgramDebugError::Compilation(CompileError::ProgramTooLarge),
            "cannot compile structured program: BlueJS program exceeds the bytecode size limit or a metadata limit",
        ),
        (
            BlueJsProgramDebugError::GenerationExhausted,
            "BlueJS program generation space is exhausted",
        ),
        (
            BlueJsProgramDebugError::CodeUnitLimitExceeded,
            "BlueJS program has too many code units",
        ),
        (
            BlueJsProgramDebugError::AstNodeLimitExceeded,
            "BlueJS program has too many AST nodes",
        ),
        (
            BlueJsProgramDebugError::UnknownProgram,
            "BlueJS program handle is stale or unknown",
        ),
        (
            BlueJsProgramDebugError::StaleSafePoint,
            "BlueJS safe point belongs to another generation",
        ),
        (
            BlueJsProgramDebugError::UnknownCodeUnit,
            "BlueJS code unit is unknown for this generation",
        ),
        (
            BlueJsProgramDebugError::InvalidInstructionBoundary,
            "BlueJS offset is not an instruction boundary",
        ),
        (
            BlueJsProgramDebugError::StaleAstNode,
            "BlueJS AST node belongs to another generation",
        ),
        (
            BlueJsProgramDebugError::UnknownAstNode,
            "BlueJS AST node is unknown for this generation",
        ),
        (
            BlueJsProgramDebugError::AstNodeUnbound,
            "BlueJS AST node has no executable root safe point",
        ),
        (
            BlueJsProgramDebugError::AstBytecodeShapeMismatch,
            "BlueJS AST and bytecode top-level provenance disagree",
        ),
    ];
    for (error, message) in cases {
        assert_eq!(error.to_string(), message);
    }
    let converted: BlueJsProgramDebugError = CompileError::ProgramTooLarge.into();
    assert_eq!(
        converted,
        BlueJsProgramDebugError::Compilation(CompileError::ProgramTooLarge)
    );
}

#[test]
fn handles_and_generations_expose_their_opaque_values() {
    let mut registry = BlueJsProgramRegistry::default();
    let first = registry
        .install(source("page:///a.js", "sha256:a"), &script("1"))
        .unwrap();
    let second = registry
        .install(source("page:///b.js", "sha256:b"), &script("2"))
        .unwrap();
    assert_eq!(first.generation().as_u64(), 1);
    assert_eq!(second.generation().as_u64(), 2);
    let compiled = registry.get(second).unwrap();
    assert_eq!(compiled.handle(), second);
    let root_node = compiled.ast_nodes()[0].id();
    assert_eq!(root_node.generation(), second.generation());
    assert_eq!(BlueJsProgramRegistry::ABI, BLUEJS_PROGRAM_DEBUG_ABI_V1);
}

#[test]
fn the_generation_space_can_be_exhausted() {
    let mut registry = BlueJsProgramRegistry {
        next_generation: u64::MAX,
        ..BlueJsProgramRegistry::default()
    };
    assert_eq!(
        registry.install(source("page:///last.js", "sha256:last"), &script("1")),
        Err(BlueJsProgramDebugError::GenerationExhausted)
    );
}

#[test]
fn identity_components_past_their_limits_are_rejected() {
    let nested = script("function f(){return 1} f();");
    let compile = |program: &BlueJsProgramV1| program.compile().unwrap();
    let mut registry = BlueJsProgramRegistry::default();
    let limits = |code_units, ast_nodes, instruction_offset| IdentityLimits {
        code_units,
        ast_nodes,
        instruction_offset,
    };
    let install = |registry: &mut BlueJsProgramRegistry, limits| {
        registry.install_limited(
            source("page:///limit.js", "sha256:limit"),
            compile(&nested),
            collect_ast_nodes(&nested),
            limits,
        )
    };
    let all = usize::MAX;
    assert_eq!(
        install(&mut registry, limits(0, all, all)),
        Err(BlueJsProgramDebugError::CodeUnitLimitExceeded)
    );
    assert_eq!(
        install(&mut registry, limits(all, 0, all)),
        Err(BlueJsProgramDebugError::AstNodeLimitExceeded)
    );
    assert_eq!(
        install(&mut registry, limits(all, all, 0)),
        Err(BlueJsProgramDebugError::InvalidInstructionBoundary)
    );
    assert!(install(&mut registry, limits(all, all, all)).is_ok());
}

#[test]
fn identity_components_are_bounded_by_u32_even_without_a_smaller_limit() {
    assert_eq!(
        identity_component(7, usize::MAX, BlueJsProgramDebugError::UnknownProgram),
        Ok(7)
    );
    assert_eq!(
        identity_component(
            u32::MAX as usize + 1,
            usize::MAX,
            BlueJsProgramDebugError::UnknownProgram
        ),
        Err(BlueJsProgramDebugError::UnknownProgram)
    );
}

#[test]
fn top_level_provenance_that_disagrees_with_the_bytecode_is_rejected() {
    let program = script("1; 2;");
    let mut registry = BlueJsProgramRegistry::default();

    // One AST statement fewer than the bytecode's statement offsets.
    let mut nodes = collect_ast_nodes(&program);
    let last_statement = nodes.iter().rposition(|node| node.top_level_statement);
    nodes.remove(last_statement.unwrap());
    assert_eq!(
        registry.install_with_ast_nodes(
            source("page:///short.js", "sha256:short"),
            program.compile().unwrap(),
            nodes
        ),
        Err(BlueJsProgramDebugError::AstBytecodeShapeMismatch)
    );

    // An offset that is not the start of any instruction.
    let mut bytecode = program.compile().unwrap();
    bytecode.root_statement_offsets[0] = Some(u32::MAX);
    assert_eq!(
        registry.install_with_ast_nodes(
            source("page:///bad.js", "sha256:bad"),
            bytecode,
            collect_ast_nodes(&program)
        ),
        Err(BlueJsProgramDebugError::AstBytecodeShapeMismatch)
    );
}

#[test]
fn replacing_an_unknown_program_fails_before_compiling() {
    let mut registry = BlueJsProgramRegistry::default();
    let handle = registry
        .install(source("page:///main.js", "sha256:one"), &script("1"))
        .unwrap();
    assert!(registry.invalidate(handle));
    assert_eq!(
        registry.replace(
            handle,
            source("page:///main.js", "sha256:two"),
            &script("2")
        ),
        Err(BlueJsProgramDebugError::UnknownProgram)
    );
}

#[test]
fn safe_points_and_ast_nodes_of_another_generation_are_rejected_everywhere() {
    let mut registry = BlueJsProgramRegistry::default();
    let first = registry
        .install(source("page:///a.js", "sha256:a"), &script("1"))
        .unwrap();
    let second = registry
        .install(source("page:///b.js", "sha256:b"), &script("1; 2;"))
        .unwrap();
    let first_program = registry.get(first).unwrap().clone();
    let second_program = registry.get(second).unwrap().clone();
    let foreign_node = first_program.ast_nodes()[0].id();
    let foreign_point = first_program.safe_points().next().unwrap();

    // Through the registry and directly on the compiled program.
    assert_eq!(
        registry.safe_point_for_ast_node(second, foreign_node),
        Err(BlueJsProgramDebugError::StaleAstNode)
    );
    assert_eq!(
        second_program.safe_point_for_ast_node(foreign_node),
        Err(BlueJsProgramDebugError::StaleAstNode)
    );
    assert_eq!(
        second_program.validate_safe_point(foreign_point),
        Err(BlueJsProgramDebugError::StaleSafePoint)
    );

    // A code unit ordinal beyond the inventory, and one naming a
    // different unit than the inventory holds at that ordinal.
    let mut beyond = second_program.safe_points().next().unwrap();
    beyond.code_unit.ordinal = 99;
    assert_eq!(
        second_program.validate_safe_point(beyond),
        Err(BlueJsProgramDebugError::UnknownCodeUnit)
    );
    let mut renamed = second_program.safe_points().next().unwrap();
    renamed.code_unit = first_program.code_units()[0].id();
    renamed.code_unit.generation = second.generation();
    renamed.code_unit.ordinal = 0;
    assert_eq!(second_program.validate_safe_point(renamed), Ok(()));
    let mut mismatched = second_program.clone();
    mismatched.code_units[0].id.ordinal = 5;
    assert_eq!(
        mismatched.validate_safe_point(renamed),
        Err(BlueJsProgramDebugError::UnknownCodeUnit)
    );
}

#[test]
fn install_and_replace_report_compile_and_generation_failures() {
    let mut registry = BlueJsProgramRegistry::default();
    assert_eq!(
        registry.install(
            source("page:///bad.js", "sha256:bad"),
            &script("let duplicate; let duplicate;")
        ),
        Err(BlueJsProgramDebugError::Compilation(
            CompileError::DuplicateBinding("duplicate".into())
        ))
    );
    let handle = registry
        .install(source("page:///main.js", "sha256:one"), &script("1"))
        .unwrap();
    registry.next_generation = u64::MAX;
    assert_eq!(
        registry.replace(
            handle,
            source("page:///main.js", "sha256:two"),
            &script("2")
        ),
        Err(BlueJsProgramDebugError::GenerationExhausted)
    );
    assert!(registry.get(handle).is_ok());
}

#[test]
fn an_ast_node_of_an_invalidated_program_is_unknown() {
    let mut registry = BlueJsProgramRegistry::default();
    let handle = registry
        .install(source("page:///main.js", "sha256:one"), &script("1"))
        .unwrap();
    let node = registry.get(handle).unwrap().ast_nodes()[0].id();
    assert!(registry.invalidate(handle));
    assert_eq!(
        registry.safe_point_for_ast_node(handle, node),
        Err(BlueJsProgramDebugError::UnknownProgram)
    );
}
