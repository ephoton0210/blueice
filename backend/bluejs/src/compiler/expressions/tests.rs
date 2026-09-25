// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

use super::*;

#[test]
fn template_site_ids_stop_before_wrapping() {
    let counter = std::sync::atomic::AtomicU64::new(u64::MAX - 1);
    assert!(matches!(next_template_site_id(&counter), Ok(id) if id == u64::MAX - 1));
    assert!(matches!(
        next_template_site_id(&counter),
        Err(CompileError::ProgramTooLarge)
    ));
    assert_eq!(counter.load(std::sync::atomic::Ordering::Relaxed), u64::MAX);
}

#[test]
fn private_update_operands_reject_unrepresentable_owners() {
    assert!(matches!(
        private_update_operand(u32::MAX / 4, UpdateOp::Dec, true),
        Ok(u32::MAX)
    ));
    assert!(matches!(
        private_update_operand(u32::MAX / 4 + 1, UpdateOp::Inc, false),
        Err(CompileError::ProgramTooLarge)
    ));
}
