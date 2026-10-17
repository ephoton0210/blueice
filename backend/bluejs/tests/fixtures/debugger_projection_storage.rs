// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

//! Tiny native projections keep their document owner through final delivery.

use super::*;
use std::cell::Cell;

fn page() -> Vm {
    let mut vm = Vm::default();
    vm.use_bounded_host_microtask_checkpoints().unwrap();
    vm
}
fn observer(vm: &Vm) -> Vm {
    Vm {
        host_scheduling: vm.host_scheduling.as_ref().map(SchedulingScope::child),
        ..Vm::default()
    }
}
fn source(vm: &Vm) -> VmDebuggerInspection<VmDebuggerValuePreview> {
    vm.heap
        .debugger_owned_value_preview(
            &Value::String("kept".into()),
            vm.object_prototype,
            vm.array_prototype,
        )
        .unwrap()
}
fn project(
    source: &VmDebuggerInspection<VmDebuggerValuePreview>,
) -> VmDebuggerInspection<Vec<u16>> {
    source
        .try_map(8, |value, output| {
            let VmDebuggerValuePreview::StringUnits(units) = value else {
                panic!("string fixture")
            };
            output.copy_slice(units)
        })
        .unwrap()
}

#[cfg_attr(test, test)]
fn debugger_projection_storage_output_outlives_source_and_vm() {
    let vm = page();
    let view = observer(&vm);
    let original = source(&vm);
    let projected = project(&original);
    drop(original);
    drop(vm);
    assert_eq!(&**projected, &[107, 101, 112, 116]);
    assert!(view.host_scheduling_snapshot().payload_bytes >= 8);
    drop(projected);
    assert_eq!(view.host_scheduling_snapshot().payload_bytes, 0);
}

#[cfg_attr(test, test)]
fn debugger_projection_storage_copy_coexists_then_shares_clones() {
    let vm = page();
    let view = observer(&vm);
    let original = source(&vm);
    let before = view.host_scheduling_snapshot().payload_bytes;
    let projected = project(&original);
    let both = view.host_scheduling_snapshot().payload_bytes;
    assert!(both > before);
    let clone = projected.clone();
    assert!(std::ptr::eq(projected.as_ref(), clone.as_ref()));
    assert_eq!(view.host_scheduling_snapshot().payload_bytes, both);
    drop(projected);
    drop(original);
    drop(vm);
    assert!(view.host_scheduling_snapshot().payload_bytes > 0);
    drop(clone);
    assert_eq!(view.host_scheduling_snapshot().payload_bytes, 0);
}

#[cfg_attr(test, test)]
fn debugger_projection_storage_terminal_preparation_never_calls_converter() {
    let vm = page();
    let view = observer(&vm);
    let original = source(&vm);
    let before = view.host_scheduling_snapshot().payload_bytes;
    vm.host_scheduling.as_ref().unwrap().fail();
    let called = Cell::new(false);
    let result = original.try_map(0, |_, _| {
        called.set(true);
        Ok(7u32)
    });
    assert_eq!(result, Err(RuntimeError::SchedulingLimit));
    assert!(!called.get());
    assert_eq!(view.host_scheduling_snapshot().payload_bytes, before);
    drop(original);
    drop(vm);
    assert_eq!(view.host_scheduling_snapshot().payload_bytes, 0);
}

#[cfg_attr(test, test)]
fn debugger_projection_storage_failed_conversion_releases_candidate_buffers() {
    let vm = page();
    let view = observer(&vm);
    let original = source(&vm);
    let before = view.host_scheduling_snapshot().payload_bytes;
    let result: Result<VmDebuggerInspection<Vec<u16>>, RuntimeError> =
        original.try_map(8, |value, output| {
            let VmDebuggerValuePreview::StringUnits(units) = value else {
                panic!("string fixture")
            };
            let _copy = output.copy_slice(units)?;
            Err(RuntimeError::Unsupported("fixed conversion failure"))
        });
    assert_eq!(
        result,
        Err(RuntimeError::Unsupported("fixed conversion failure"))
    );
    assert_eq!(view.host_scheduling_snapshot().payload_bytes, before);
}

#[cfg_attr(test, test)]
fn debugger_projection_storage_neutral_output_keeps_neutral_policy() {
    let vm = Vm::default();
    let original = source(&vm);
    let projected = project(&original);
    assert!(original.inner._storage.is_none());
    assert!(projected.inner._storage.is_none());
    drop(vm);
    drop(original);
    assert_eq!(&**projected, &[107, 101, 112, 116]);
}

#[cfg_attr(test, test)]
fn debugger_projection_storage_documents_keep_independent_copies() {
    let first = page();
    let second = page();
    let first_view = observer(&first);
    let second_view = observer(&second);
    let original = source(&first);
    let other = source(&second);
    let projected = project(&original);
    let before = first_view.host_scheduling_snapshot().payload_bytes;
    let other_projection = project(&other);
    assert_eq!(first_view.host_scheduling_snapshot().payload_bytes, before);
    drop(original);
    drop(first);
    drop(other);
    drop(second);
    drop(projected);
    assert_eq!(first_view.host_scheduling_snapshot().payload_bytes, 0);
    assert!(second_view.host_scheduling_snapshot().payload_bytes > 0);
    drop(other_projection);
    assert_eq!(second_view.host_scheduling_snapshot().payload_bytes, 0);
}

#[cfg_attr(test, test)]
fn debugger_projection_storage_boxed_and_transferred_data_keep_one_owner() {
    let vm = page();
    let view = observer(&vm);
    let original = source(&vm);
    let moved = vec![4u8, 5, 6];
    let retained = moved.capacity();
    let projected = original
        .try_map(3 + retained, |_, output| {
            let boxed = output.boxed([1u8, 2, 3])?;
            output.retain_bytes(retained)?;
            Ok((boxed, moved))
        })
        .unwrap();
    drop(original);
    drop(vm);
    assert_eq!(*projected.0, [1, 2, 3]);
    assert_eq!(projected.1, [4, 5, 6]);
    assert!(view.host_scheduling_snapshot().payload_bytes >= 6);
    drop(projected);
    assert_eq!(view.host_scheduling_snapshot().payload_bytes, 0);
}

#[cfg_attr(test, test)]
fn debugger_projection_storage_terminal_converter_cannot_publish_its_output() {
    let vm = page();
    let view = observer(&vm);
    let original = source(&vm);
    let before = view.host_scheduling_snapshot().payload_bytes;
    let scope = vm.host_scheduling.as_ref().unwrap().child();
    let result = original.try_map(8, |value, output| {
        let VmDebuggerValuePreview::StringUnits(units) = value else {
            panic!("string fixture")
        };
        let copied = output.copy_slice(units)?;
        scope.fail();
        Ok(copied)
    });
    assert_eq!(result, Err(RuntimeError::SchedulingLimit));
    assert_eq!(view.host_scheduling_snapshot().payload_bytes, before);
    drop(original);
    drop(vm);
    assert_eq!(view.host_scheduling_snapshot().payload_bytes, 0);
    assert_eq!(
        view.check_scheduling_failure(),
        Err(RuntimeError::SchedulingLimit)
    );
}

#[cfg_attr(test, test)]
fn debugger_projection_storage_final_data_drops_before_its_credit() {
    struct Witness<'a> {
        view: &'a Vm,
        observed: Rc<Cell<bool>>,
    }
    impl Drop for Witness<'_> {
        fn drop(&mut self) {
            self.observed
                .set(self.view.host_scheduling_snapshot().payload_bytes > 0);
        }
    }
    let vm = page();
    let view = observer(&vm);
    let original = source(&vm);
    let observed = Rc::new(Cell::new(false));
    let projected = original
        .try_map(0, |_, _| {
            Ok(Witness {
                view: &view,
                observed: observed.clone(),
            })
        })
        .unwrap();
    drop(original);
    drop(vm);
    drop(projected);
    assert!(observed.get());
    assert_eq!(view.host_scheduling_snapshot().payload_bytes, 0);
}

#[cfg_attr(test, test)]
fn debugger_projection_storage_multi_hop_copy_keeps_the_same_document() {
    let vm = page();
    let view = observer(&vm);
    let original = source(&vm);
    let first = project(&original);
    let last = first
        .try_map(8, |units, output| {
            output.boxed([units[0], units[1], units[2], units[3]])
        })
        .unwrap();
    drop(original);
    drop(first);
    drop(vm);
    assert_eq!(last.as_ref().as_ref(), &[107, 101, 112, 116]);
    assert!(view.host_scheduling_snapshot().payload_bytes >= 8);
    drop(last);
    assert_eq!(view.host_scheduling_snapshot().payload_bytes, 0);
}

impl Vm {
    #[doc(hidden)]
    pub fn verify_debugger_projection_storage_contracts() {
        debugger_projection_storage_failed_document_preserves_healthy_projection();
        debugger_projection_storage_output_outlives_source_and_vm();
        debugger_projection_storage_copy_coexists_then_shares_clones();
        debugger_projection_storage_terminal_preparation_never_calls_converter();
        debugger_projection_storage_failed_conversion_releases_candidate_buffers();
        debugger_projection_storage_neutral_output_keeps_neutral_policy();
        debugger_projection_storage_documents_keep_independent_copies();
        debugger_projection_storage_boxed_and_transferred_data_keep_one_owner();
        debugger_projection_storage_terminal_converter_cannot_publish_its_output();
        debugger_projection_storage_final_data_drops_before_its_credit();
        debugger_projection_storage_multi_hop_copy_keeps_the_same_document();
    }
}

#[cfg_attr(test, test)]
fn debugger_projection_storage_failed_document_preserves_healthy_projection() {
    let first = page();
    let second = page();
    let first_view = observer(&first);
    let second_view = observer(&second);
    let original = source(&first);
    let other = source(&second);
    first.host_scheduling.as_ref().unwrap().fail();
    assert_eq!(
        original.try_map(0, |_, _| Ok(1u8)),
        Err(RuntimeError::SchedulingLimit)
    );
    let projected = project(&other);
    assert!(second.check_scheduling_failure().is_ok());
    drop(original);
    drop(first);
    drop(other);
    drop(second);
    assert_eq!(first_view.host_scheduling_snapshot().payload_bytes, 0);
    assert!(second_view.host_scheduling_snapshot().payload_bytes > 0);
    drop(projected);
    assert_eq!(second_view.host_scheduling_snapshot().payload_bytes, 0);
}
