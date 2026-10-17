// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

use blueice_bluejs::{
    parse, parse_module_with_debug_spans, BlueJsAstNodeKind, BlueJsProgramDebugError,
    BlueJsProgramRegistry, BlueJsProgramV1, BlueJsSourceIdentity, CompileError,
};

#[test]
fn emitted_module_parser_items_match_compiler_root_and_identifier_safe_points() {
    let source = "import { value } from './dep.js';\nexport const answer = value + 1;";
    let (module, statements, identifiers) = parse_module_with_debug_spans(source).unwrap();
    assert_eq!(statements.len(), 1);
    assert_eq!(identifiers.len(), 1);
    let mut registry = BlueJsProgramRegistry::default();
    let handle = registry
        .install(
            BlueJsSourceIdentity::new("page:///main.mjs", "sha256:generated-module").unwrap(),
            &BlueJsProgramV1::Module(module),
        )
        .unwrap();
    let compiled = registry.get(handle).unwrap();
    let statements_in_code = compiled
        .ast_nodes()
        .iter()
        .filter(|node| node.is_top_level_statement())
        .collect::<Vec<_>>();
    assert_eq!(statements_in_code.len(), statements.len());
    let root = compiled
        .safe_point_for_ast_node(statements_in_code[0].id())
        .unwrap();
    assert_eq!(root.code_unit.ordinal(), 0);
    let imported_value = compiled.unique_identifier_safe_point(0, "value").unwrap();
    assert_eq!(imported_value.code_unit.ordinal(), 0);
    // A statement and its first expression may share an instruction. Source
    // projection must treat such a reverse mapping as ambiguous, not invent
    // an instruction between them.
    assert_eq!(
        &source[identifiers[0].start_byte..identifiers[0].end_byte],
        "value"
    );
}

#[test]
fn hoisted_module_function_retains_its_root_statement_for_nested_identifier() {
    let source = "export function inner(){ return 41 + (globalThis).offset; }";
    let (module, statements, identifiers) = parse_module_with_debug_spans(source).unwrap();
    assert_eq!(statements.len(), 1);
    assert_eq!(identifiers.len(), 1);
    assert_eq!(
        &source[identifiers[0].start_byte..identifiers[0].end_byte],
        "globalThis"
    );
    let mut registry = BlueJsProgramRegistry::default();
    let handle = registry
        .install(
            BlueJsSourceIdentity::new("page:///dependency.mjs", "sha256:generated-dependency")
                .unwrap(),
            &BlueJsProgramV1::Module(module),
        )
        .unwrap();
    let compiled = registry.get(handle).unwrap();
    let point = compiled
        .unique_identifier_safe_point(0, "globalThis")
        .unwrap();
    assert_eq!(point.code_unit.ordinal(), 1);
    assert_eq!(
        compiled.unique_identifier_at_safe_point(point),
        Some((0, "globalThis"))
    );
}

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
        (
            Error::NativeStorageLimit,
            "BlueJS shared code native storage limit exceeded",
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
fn unique_root_identifier_evaluation_has_an_exact_compiler_safe_point() {
    let source = || BlueJsSourceIdentity::new("page:///decorated.js", "sha256:decorated").unwrap();
    let mut registry = BlueJsProgramRegistry::default();
    let program =
        BlueJsProgramV1::Script(parse("const decorators = [decoSite]; decorators;").unwrap());
    let handle = registry.install(source(), &program).unwrap();
    let compiled = registry.get(handle).unwrap();
    let point = compiled
        .unique_identifier_safe_point(0, "decoSite")
        .expect("one evaluated decorator identifier");
    assert_eq!(registry.validate_safe_point(handle, point), Ok(()));
    assert_eq!(
        compiled.unique_identifier_at_safe_point(point),
        Some((0, "decoSite"))
    );
    assert_eq!(compiled.unique_identifier_safe_point(0, "decorators"), None);

    let duplicate =
        BlueJsProgramV1::Script(parse("const decorators = [decoSite, decoSite];").unwrap());
    let handle = registry.install(source(), &duplicate).unwrap();
    let compiled = registry.get(handle).unwrap();
    assert_eq!(compiled.unique_identifier_safe_point(0, "decoSite"), None);
}

#[test]
fn nested_repeated_root_callees_have_two_distinct_compiler_ordered_points() {
    let program = BlueJsProgramV1::Script(
        parse("var merged = helper(helper({}, base, true), {b: 2}, false);").unwrap(),
    );
    let mut registry = BlueJsProgramRegistry::default();
    let handle = registry
        .install(
            BlueJsSourceIdentity::new("page:///spread.js", "sha256:spread").unwrap(),
            &program,
        )
        .unwrap();
    let compiled = registry.get(handle).unwrap();
    assert_eq!(compiled.unique_identifier_safe_point(0, "helper"), None);
    let [outer, inner] = compiled
        .two_root_identifier_safe_points(0, "helper")
        .expect("the nested calls evaluate distinct callees in source order");
    assert_ne!(outer, inner);
    assert!(outer.bytecode_offset < inner.bytecode_offset);
    assert_eq!(registry.validate_safe_point(handle, outer), Ok(()));
    assert_eq!(registry.validate_safe_point(handle, inner), Ok(()));
    assert_eq!(compiled.two_root_identifier_safe_points(0, "base"), None);
}

#[test]
fn identifier_provenance_retains_static_block_nesting_depth() {
    let source = "function decoSite() {} class Direct { static { const decorators = [decoSite]; } } const Indirect = (() => { class Nested { static { const decorators = [decoSite]; } } return Nested; })();";
    let mut registry = BlueJsProgramRegistry::default();
    let handle = registry
        .install(
            BlueJsSourceIdentity::new("page:///decorated.js", "sha256:depth").unwrap(),
            &BlueJsProgramV1::Script(parse(source).unwrap()),
        )
        .unwrap();
    let compiled = registry.get(handle).unwrap();
    let direct = compiled
        .unique_identifier_safe_point(1, "decoSite")
        .unwrap();
    let indirect = compiled
        .unique_identifier_safe_point(2, "decoSite")
        .unwrap();
    assert_eq!(
        compiled.code_units()[direct.code_unit.ordinal() as usize].nesting_depth(),
        1
    );
    assert_eq!(
        compiled.code_units()[indirect.code_unit.ordinal() as usize].nesting_depth(),
        2
    );
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
