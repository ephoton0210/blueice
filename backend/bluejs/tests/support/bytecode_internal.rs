// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

use super::{Bytecode, Opcode};

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
