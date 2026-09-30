// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

use super::{Bytecode, Opcode, MAY_USE_INLINE_CACHE};
use crate::{Value, Vm};

fn code(bytes: &[u8]) -> Bytecode {
    let mut bytecode = Bytecode::empty();
    bytecode.code = bytes.to_vec();
    bytecode
}

#[test]
fn decodes_an_instruction_with_and_without_an_operand() {
    let bytecode = code(&[Opcode::Constant as u8, 7, 0, 0, 0, Opcode::Pop as u8]);
    let constant = bytecode.instruction(0).unwrap();
    assert_eq!(
        (constant.opcode, constant.operand),
        (Opcode::Constant, Some(7))
    );
    let pop = bytecode.instruction(5).unwrap();
    assert_eq!((pop.opcode, pop.operand), (Opcode::Pop, None));
    assert_eq!(bytecode.instructions().count(), 2);
}

#[test]
fn malformed_code_decodes_to_no_instruction() {
    // Past the end, an unassigned opcode byte, and an operand cut short.
    let bytecode = code(&[Opcode::Pop as u8, 0xFF, Opcode::Constant as u8, 1, 2]);
    assert!(bytecode.instruction(9).is_none());
    assert!(bytecode.instruction(1).is_none());
    assert!(bytecode.instruction(2).is_none());
    // The iterator stops at the first byte it cannot decode.
    assert_eq!(bytecode.instructions().count(), 1);
}

#[test]
fn an_empty_program_exposes_no_metadata() {
    let bytecode = Bytecode::empty();
    assert_eq!(bytecode.bytes(), &[] as &[u8]);
    assert!(bytecode.constants().is_empty());
    assert!(bytecode.root_statement_offsets().is_empty());
    assert_eq!(bytecode.child_code_units().count(), 0);
}

#[test]
fn opcode_metadata_reports_width_and_inline_cache_use() {
    assert_eq!((Opcode::Pop.width(), Opcode::Pop.flags()), (1, 0));
    assert_eq!(Opcode::Constant.width(), 5);
    assert_eq!(Opcode::GetProperty.flags(), MAY_USE_INLINE_CACHE);
}

#[test]
fn exactly_the_assigned_opcode_bytes_decode() {
    let decoded = (0..=u8::MAX).filter_map(Opcode::decode).collect::<Vec<_>>();
    // Bytes are assigned to opcodes densely from zero, in table order.
    assert_eq!(decoded.first(), Some(&Opcode::Constant));
    assert!(decoded
        .iter()
        .enumerate()
        .all(|(index, opcode)| *opcode as usize == index));
    assert!(Opcode::decode(decoded.len() as u8).is_none());
}

#[test]
fn decoder_rejects_unknown_opcodes_and_truncated_operands() {
    assert_eq!(Opcode::decode(u8::MAX), None);

    let mut code = Bytecode::empty();
    code.code = vec![u8::MAX];
    assert_eq!(code.instruction(0), None);
    assert_eq!(code.instructions().next(), None);

    for operand_bytes in 0..4 {
        code.code = vec![Opcode::Constant as u8];
        code.code.extend(std::iter::repeat_n(0, operand_bytes));
        assert_eq!(code.instruction(0), None);
        assert_eq!(code.instructions().next(), None);
    }

    code.code = vec![Opcode::Constant as u8, 0x78, 0x56, 0x34, 0x12];
    let instruction = code.instruction(0).unwrap();
    assert_eq!(instruction.opcode, Opcode::Constant);
    assert_eq!(instruction.operand, Some(0x1234_5678));
    assert_eq!(code.instruction(code.code.len()), None);
}

#[test]
fn compiled_root_ranges_and_child_indices_match_the_executable_statements() {
    let program = crate::parse(
        "function first() { return 1; } let answer = 2; function second() { return answer; }",
    )
    .unwrap();
    let code = crate::compile(&program).unwrap();
    let ranges = code.root_statement_ranges();
    let children = code.root_function_child_indices();
    assert_eq!(ranges.len(), 3);
    assert_eq!(children, &[Some(0), None, Some(1)]);
    assert_eq!(code.root_statement_offsets().len(), ranges.len());
    for (offset, range) in code.root_statement_offsets().iter().zip(ranges) {
        let (start, end) = range.expect("each executable root statement has a range");
        assert_eq!(*offset, Some(start));
        assert!(start < end);
        assert!(end as usize <= code.bytes().len());
    }
    assert!(children
        .iter()
        .flatten()
        .all(|&index| (index as usize) < code.functions.len()));
}

#[test]
fn debugger_binding_layout_matches_only_structurally_equal_code_units() {
    let same_source = "let a = 1; let b = 2; function f() { return a + b; }";
    let first = crate::compile(&crate::parse(same_source).unwrap()).unwrap();
    let second = crate::compile(&crate::parse(same_source).unwrap()).unwrap();
    assert!(first.debugger_binding_layout_matches(&second));

    let different_source = "let a = 1; let b = 2; let c = 3; function f() { return a + b + c; }";
    let different = crate::compile(&crate::parse(different_source).unwrap()).unwrap();
    assert!(!first.debugger_binding_layout_matches(&different));
}

#[test]
fn a_tagged_template_call_site_caches_its_strings_object_by_identity() {
    // `TemplateSiteId` is a per-call-site `Bytecode::templates` key used as a
    // real `HashMap` key (`Vm`'s per-execution `templates` cache) -- ECMA-262
    // requires re-evaluating the same tagged-template call site to return the
    // exact same (frozen) strings array object both times.
    let program = crate::parse(
        "function tag(strings) { return strings; } \
         function f() { return tag`a${1}b`; } \
         Object.is(f(), f());",
    )
    .unwrap();
    let code = crate::compile(&program).unwrap();
    assert_eq!(Vm::default().execute(&code).unwrap(), Value::Bool(true));
}
