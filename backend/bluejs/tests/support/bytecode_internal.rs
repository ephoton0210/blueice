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
