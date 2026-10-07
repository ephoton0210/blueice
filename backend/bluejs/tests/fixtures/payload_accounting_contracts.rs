// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

use super::*;

#[cfg_attr(test, test)]
fn owned_payload_accumulation_preserves_unavailable_children_and_real_overflow() {
    assert_eq!(add_payload(0, Some(0)), Some(0));
    assert_eq!(add_payload(7, Some(11)), Some(18));
    assert_eq!(add_payload(usize::MAX - 1, Some(1)), Some(usize::MAX));
    assert_eq!(add_payload(usize::MAX, Some(1)), None);
    assert_eq!(add_payload(7, None), None);
    let source =
        "class Example { field = {value: 1}; #private = 2; method() { return this.field; } }";
    let program = crate::parse(source).unwrap();
    let before = program.owned_heap_payload_bytes().unwrap();
    crate::compile(&program).unwrap();
    assert_eq!(program.owned_heap_payload_bytes(), Some(before));
}

#[cfg(coverage)]
impl Program {
    #[doc(hidden)]
    pub fn verify_payload_accounting_contracts() {
        owned_payload_accumulation_preserves_unavailable_children_and_real_overflow();
    }
}
