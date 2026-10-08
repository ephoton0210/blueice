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

// A reported unavailable child is part of the accounting protocol. This
// fixture supplies real Vec/Box allocations and never fabricates capacity.
struct ReportedPayload(Option<usize>);
impl HeapPayload for ReportedPayload {
    fn heap_payload(&self, _: &mut CountContext) -> Option<usize> {
        self.0
    }
}
#[cfg_attr(test, test)]
fn real_owned_containers_preserve_child_refusal_and_sum_overflow() {
    let mut context = CountContext::default();
    let values = [Some(0), Some(7), None, Some(usize::MAX)];
    for value in values {
        let boxed = Box::new(ReportedPayload(value));
        assert_eq!(
            boxed.heap_payload(&mut context),
            value.and_then(|n| n.checked_add(std::mem::size_of::<ReportedPayload>()))
        );
        let vector = vec![ReportedPayload(value)];
        assert_eq!(
            vector.heap_payload(&mut context),
            value.and_then(
                |n| n.checked_add(vector.capacity() * std::mem::size_of::<ReportedPayload>())
            )
        );
        let optional = value.map(|n| ReportedPayload(Some(n)));
        assert_eq!(
            optional.heap_payload(&mut context),
            Some(value.unwrap_or(0))
        );
    }
    let empty: Vec<ReportedPayload> = Vec::new();
    assert_eq!(empty.heap_payload(&mut context), Some(0));
    let unavailable = Some(ReportedPayload(None));
    assert_eq!(unavailable.heap_payload(&mut context), None);
    let many = vec![ReportedPayload(Some(3)), ReportedPayload(Some(11))];
    assert_eq!(
        many.heap_payload(&mut context),
        Some(many.capacity() * std::mem::size_of::<ReportedPayload>() + 14)
    );
}
#[cfg_attr(test, test)]
fn bigint_accounting_counts_logical_u32_limbs_and_preserves_sign() {
    let mut context = CountContext::default();
    for (decimal, limbs) in [
        ("0", 0usize),
        ("1", 1),
        ("-1", 1),
        ("2147483648", 1),
        ("4294967295", 1),
        ("4294967296", 2),
        ("-4294967296", 2),
        ("18446744073709551616", 3),
        ("-18446744073709551616", 3),
    ] {
        let value = BigInt::parse_bytes(decimal.as_bytes(), 10).unwrap();
        assert_eq!(
            value.heap_payload(&mut context),
            Some(limbs * 4),
            "{decimal}"
        );
    }
}
#[cfg_attr(test, test)]
fn compiler_class_initializer_shapes_count_each_owned_child_once() {
    // These are public owned AST variants emitted by class_field_definition
    // and compiler/functions. Accounting does not execute compiler-private IR.
    let child = crate::parse("this.value = {answer: 42};")
        .unwrap()
        .body
        .pop()
        .unwrap();
    let record = String::from("initializerRecord");
    let mut context = CountContext::default();
    let expected = std::mem::size_of::<Stmt>() + child.heap_payload(&mut context).unwrap();
    let field = Stmt::ClassField(Box::new(child));
    assert_eq!(
        field.heap_payload(&mut CountContext::default()),
        Some(expected)
    );
    let decorated = Stmt::ClassDecoratedField {
        field: Box::new(field),
        record: record.clone(),
    };
    assert_eq!(
        decorated.heap_payload(&mut CountContext::default()),
        Some(std::mem::size_of::<Stmt>() + expected + record.capacity())
    );
    for statement in [
        Stmt::ClassPrivateBrand(record.clone()),
        Stmt::ClassExtraInitializers(record.clone()),
    ] {
        assert_eq!(
            statement.heap_payload(&mut CountContext::default()),
            Some(record.capacity())
        );
    }
}

#[cfg(coverage)]
impl Program {
    #[doc(hidden)]
    pub fn verify_payload_accounting_contracts() {
        owned_payload_accumulation_preserves_unavailable_children_and_real_overflow();
        real_owned_containers_preserve_child_refusal_and_sum_overflow();
        bigint_accounting_counts_logical_u32_limbs_and_preserves_sign();
        compiler_class_initializer_shapes_count_each_owned_child_once();
    }
}
