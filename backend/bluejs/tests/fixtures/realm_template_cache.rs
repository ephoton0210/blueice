// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

//! Small jobs share one realm's tagged-template identities and cache owner.

use super::*;
use crate::vm::host_scheduling::SharedSchedulingCharge;

fn compile(source: &str) -> Bytecode {
    crate::compile(&crate::parse(source).unwrap()).unwrap()
}

fn bounded_vm() -> Vm {
    let mut vm = Vm::default();
    vm.use_bounded_host_microtask_checkpoints().unwrap();
    vm.remaining_instructions = 100_000;
    vm
}

fn leave_room(vm: &mut Vm, room: usize) -> SharedSchedulingCharge {
    let used = vm.host_scheduling_usage().unwrap().payload_bytes;
    vm.host_scheduling
        .as_ref()
        .unwrap()
        .shared_charge(MAX_HOST_SCHEDULING_BYTES - used - room)
        .unwrap()
}

fn complete(vm: &mut Vm) {
    for _ in 0..16 {
        if !vm.run_next_promise_job().unwrap() {
            return;
        }
    }
    assert!(
        vm.promise_jobs.is_empty(),
        "small template round trip exceeded sixteen turns"
    );
}

#[cfg_attr(test, test)]
fn realm_template_cache_async_resume_sees_a_site_created_after_suspension() {
    let mut vm = bounded_vm();
    vm.execute_script(&compile("var same=false;var release;var gate=new Promise(r=>release=r);function site(){return (x=>x)`tiny`;}async function wait(){await gate;return site();}var pending=wait();var first=site();release();pending.then(x=>same=x===first);0;")).unwrap();
    complete(&mut vm);
    assert_eq!(
        vm.lookup_global_name("same").unwrap(),
        Some(Value::Bool(true))
    );
}

#[cfg_attr(test, test)]
fn realm_template_cache_async_generator_sees_a_site_created_after_suspension() {
    let mut vm = bounded_vm();
    vm.execute_script(&compile("var same=false;var release;var gate=new Promise(r=>release=r);function site(){return (x=>x)`tiny`;}async function* wait(){await gate;yield site();}var pending=wait().next();var first=site();release();pending.then(x=>same=x.value===first);0;")).unwrap();
    complete(&mut vm);
    assert_eq!(
        vm.lookup_global_name("same").unwrap(),
        Some(Value::Bool(true))
    );
}

#[cfg_attr(test, test)]
fn realm_template_cache_miss_refuses_before_heap_effects() {
    let mut vm = bounded_vm();
    vm.string_intrinsics().unwrap();
    vm.stack.reserve(16);
    vm.stack.push(Value::Number(7.0));
    let code = compile("tag`tiny`");
    let before = vm.heap.stats();
    let _peer = leave_room(&mut vm, 0);
    assert_eq!(
        vm.template_object(&code.templates[0]),
        Err(RuntimeError::SchedulingLimit)
    );
    assert_eq!(vm.heap.stats().managed_bytes, before.managed_bytes);
    assert_eq!(
        vm.heap.stats().root_registrations,
        before.root_registrations
    );
    assert!(vm.templates.is_empty());
    assert_eq!(vm.stack, vec![Value::Number(7.0)]);
}

#[cfg_attr(test, test)]
fn realm_template_cache_stays_owned_once_across_displaced_frames() {
    let mut vm = bounded_vm();
    vm.stack.reserve(16);
    let code = compile("tag`first`;tag`second`");
    let first = vm.template_object(&code.templates[0]).unwrap();
    let address = vm.templates.get(&code.templates[0].id).unwrap() as *const ObjectId;
    let control = vm.current_execution_control_bytes();
    let before = vm.host_scheduling_usage().unwrap().payload_bytes;
    let execution = vm.suspend_module_execution();
    assert_eq!(
        vm.templates.get(&code.templates[0].id).copied(),
        first.object_id()
    );
    assert_eq!(execution.control_charge.bytes(), control);
    assert_eq!(vm.host_scheduling_usage().unwrap().payload_bytes, before);
    let second = vm.template_object(&code.templates[1]).unwrap();
    vm.restore_module_execution(execution);
    assert_eq!(vm.templates.len(), 2);
    assert_eq!(
        vm.templates.get(&code.templates[0].id).unwrap() as *const ObjectId,
        address
    );
    vm.with_roots(|heap| {
        heap.collect_major();
        Ok(())
    })
    .unwrap();
    assert_eq!(vm.template_object(&code.templates[0]).unwrap(), first);
    assert_eq!(vm.template_object(&code.templates[1]).unwrap(), second);
    assert_eq!(vm.displaced_execution_control_bytes.get(), 0);
}

#[cfg_attr(test, test)]
fn realm_template_cache_joint_exact_capacity_and_one_byte_refusal() {
    for full in [false, true] {
        for spare_stack in [false, true] {
            for accepts in [false, true] {
                let mut vm = bounded_vm();
                for _ in 0..if full { 3 } else { 1 } {
                    vm.templates
                        .insert(TemplateSiteId::new(), vm.object_prototype);
                }
                vm.stack = Vec::with_capacity(if spare_stack { 8 } else { 1 });
                vm.stack.push(Value::Number(7.0));
                let table_bytes = if full {
                    let mut target = HashMap::<TemplateSiteId, ObjectId>::new();
                    target.reserve(vm.templates.len() + 1);
                    host_scheduling::map_slots(&target)
                } else {
                    0
                };
                let bytes = table_bytes
                    + if spare_stack {
                        0
                    } else {
                        3 * std::mem::size_of::<Value>()
                    };
                if bytes == 0 && !accepts {
                    continue;
                }
                let len = vm.templates.len();
                let capacity = vm.templates.capacity();
                let _peer = leave_room(&mut vm, bytes - usize::from(!accepts));
                let before = vm.host_scheduling_snapshot().payload_bytes;
                let prepared = vm.prepare_template_cache_insert();
                if accepts {
                    let prepared = prepared.unwrap();
                    assert_eq!(vm.displaced_execution_control_bytes.get(), 0);
                    assert_eq!(
                        vm.host_scheduling_snapshot().payload_bytes - before,
                        table_bytes
                            + if spare_stack {
                                0
                            } else {
                                2 * std::mem::size_of::<Value>()
                            }
                    );
                    assert!(vm.stack.capacity() >= 3);
                    drop(prepared);
                } else {
                    assert!(matches!(prepared, Err(RuntimeError::SchedulingLimit)));
                }
                assert_eq!(
                    (vm.templates.len(), vm.templates.capacity()),
                    (len, capacity)
                );
                assert_eq!(vm.stack, vec![Value::Number(7.0)]);
                assert_eq!(vm.displaced_execution_control_bytes.get(), 0);
                vm.reconcile_scheduling_ownership();
                // Only actual retained operand capacity can outlive staging.
                assert_eq!(
                    vm.host_scheduling_snapshot().payload_bytes - before,
                    if accepts && !spare_stack {
                        2 * std::mem::size_of::<Value>()
                    } else {
                        0
                    }
                );
            }
        }
    }
}

#[cfg_attr(test, test)]
fn realm_template_cache_hits_allocate_nothing_at_a_full_native_budget() {
    let mut vm = bounded_vm();
    let code = compile("tag`tiny`");
    let first = vm.template_object(&code.templates[0]).unwrap();
    vm.stack = Vec::new();
    let before = vm.heap.stats();
    let _peer = leave_room(&mut vm, 0);
    assert_eq!(vm.template_object(&code.templates[0]).unwrap(), first);
    assert_eq!(vm.stack.capacity(), 0);
    assert_eq!(vm.heap.stats(), before);
}

#[cfg_attr(test, test)]
fn realm_template_cache_spare_entry_uses_no_fictitious_growth_charge() {
    let mut vm = bounded_vm();
    vm.stack.reserve(8);
    let code = compile("tag`first`;tag`second`");
    let first = vm.template_object(&code.templates[0]).unwrap();
    let capacity = vm.templates.capacity();
    let before = vm.host_scheduling_usage().unwrap().payload_bytes;
    let _peer = leave_room(&mut vm, 0);
    let second = vm.template_object(&code.templates[1]).unwrap();
    assert_ne!(first, second);
    assert_eq!(vm.templates.capacity(), capacity);
    assert_eq!(
        vm.own_scheduling_usage().payload_bytes
            + vm.heap.root_storage_bytes()
            + vm.heap.collector_storage_bytes(),
        before
    );
    assert_eq!(vm.displaced_execution_control_bytes.get(), 0);
}

#[cfg_attr(test, test)]
fn realm_template_cache_string_refusal_releases_detached_table_and_preserves_entries() {
    let mut vm = bounded_vm();
    vm.stack.reserve(8);
    let code = compile("tag`a`;tag`b`;tag`c`;tag`oversized`");
    for site in &code.templates[..3] {
        vm.template_object(site).unwrap();
    }
    assert_eq!(vm.templates.len(), vm.templates.capacity());
    vm.stack.push(Value::Number(7.0));
    let before = vm.host_scheduling_usage().unwrap().payload_bytes;
    let heap = vm.heap.stats();
    let entries = vm.templates.clone();
    vm.config.max_string_bytes = 2;
    assert_eq!(
        vm.template_object(&code.templates[3]),
        Err(RuntimeError::StringLimit { limit: 2 })
    );
    assert!(vm.templates == entries);
    assert_eq!(vm.host_scheduling_usage().unwrap().payload_bytes, before);
    assert_eq!(vm.heap.stats(), heap);
    assert_eq!(vm.displaced_execution_control_bytes.get(), 0);
    assert_eq!(vm.stack, vec![Value::Number(7.0)]);
}

#[cfg_attr(test, test)]
fn realm_template_cache_heap_refusal_publishes_nothing_and_restores_operands() {
    let mut vm = bounded_vm();
    vm.string_intrinsics().unwrap();
    vm.stack.reserve(8);
    vm.stack.push(Value::Number(7.0));
    vm.with_roots(|heap| {
        heap.collect_major();
        Ok(())
    })
    .unwrap();
    let code = compile("tag`tiny`");
    let before = vm.host_scheduling_usage().unwrap().payload_bytes;
    let limit = vm.heap.allow_only(0);
    assert_eq!(
        vm.template_object(&code.templates[0]),
        Err(RuntimeError::Heap(HeapError::HeapLimitExceeded { limit }))
    );
    assert!(vm.templates.is_empty());
    assert_eq!(vm.templates.capacity(), 0);
    assert_eq!(vm.stack, vec![Value::Number(7.0)]);
    assert_eq!(vm.displaced_execution_control_bytes.get(), 0);
    assert_eq!(vm.host_scheduling_usage().unwrap().payload_bytes, before);
    vm.heap.allow_only(16 * 1024 * 1024);
    assert!(vm.template_object(&code.templates[0]).is_ok());
}

#[cfg_attr(test, test)]
fn realm_template_cache_preserves_frozen_descriptors_and_invalid_cooked_escapes() {
    for nursery in [1, 16] {
        let mut config = VmConfig::default();
        config.heap.nursery_capacity = nursery;
        let mut vm = Vm::new(config).unwrap();
        vm.use_bounded_host_microtask_checkpoints().unwrap();
        let source = r#"function tag(a){return Object.isFrozen(a)&&Object.isFrozen(a.raw)&&a[0]===undefined&&a.raw[0]==='\\unicode'&&!Object.getOwnPropertyDescriptor(a,'raw').enumerable;}tag`\unicode`"#;
        assert_eq!(vm.execute_script(&compile(source)), Ok(Value::Bool(true)));
        assert_eq!(vm.displaced_execution_control_bytes.get(), 0);
    }
}

#[cfg_attr(test, test)]
fn realm_template_cache_rejection_and_repeated_await_keep_the_same_identity() {
    let mut vm = bounded_vm();
    vm.execute_script(&compile("var same=false;var stable=false;var release;var gate=new Promise(r=>release=r);function site(){return (x=>x)`tiny`;}async function fail(){await gate;throw site();}var pending=fail();var first=site();release();pending.catch(x=>same=x===first);(async()=>{await 0;var saved=site();await 0;stable=saved===site();})();0;")).unwrap();
    complete(&mut vm);
    assert_eq!(
        vm.lookup_global_name("same").unwrap(),
        Some(Value::Bool(true))
    );
    assert_eq!(
        vm.lookup_global_name("stable").unwrap(),
        Some(Value::Bool(true))
    );
}

#[cfg_attr(test, test)]
fn realm_template_cache_same_compiled_site_has_independent_realm_objects() {
    let code = compile("tag`tiny`");
    let mut first = bounded_vm();
    let mut second = bounded_vm();
    let a = first.template_object(&code.templates[0]).unwrap();
    let b = second.template_object(&code.templates[0]).unwrap();
    assert_ne!(a, b);
    first.templates.clear();
    first
        .with_roots(|heap| {
            heap.collect_major();
            Ok(())
        })
        .unwrap();
    second
        .with_roots(|heap| {
            heap.collect_major();
            Ok(())
        })
        .unwrap();
    assert_eq!(
        first.heap.get_own(a.object_id().unwrap(), "raw"),
        Err(HeapError::InvalidObject(a.object_id().unwrap()))
    );
    assert!(second.heap.get_own(b.object_id().unwrap(), "raw").is_ok());
    assert_eq!(second.template_object(&code.templates[0]).unwrap(), b);
}

#[cfg_attr(test, test)]
fn realm_template_cache_module_async_frame_observes_the_live_realm_cache() {
    let mut vm = bounded_vm();
    let module = crate::compile_module(&crate::parse_module(
        "export function site(){return (x=>x)`tiny`;}export async function wait(){await gate;return site();}",
    ).unwrap()).unwrap();
    vm.set_module_loader_context(
        "page/main.js",
        HashMap::from([("page/site.js".into(), module)]),
    );
    vm.execute_script(&compile("var same=false;var release;var gate=new Promise(r=>release=r);import('./site.js').then(m=>{var pending=m.wait();var first=m.site();release();return pending.then(x=>same=x===first);});0;")).unwrap();
    complete(&mut vm);
    assert_eq!(
        vm.lookup_global_name("same").unwrap(),
        Some(Value::Bool(true))
    );
}

impl crate::Vm {
    /// Runs finite realm template-cache contracts in unit/coverage builds.
    #[doc(hidden)]
    pub fn verify_realm_template_cache_contracts() {
        realm_template_cache_async_resume_sees_a_site_created_after_suspension();
        realm_template_cache_async_generator_sees_a_site_created_after_suspension();
        realm_template_cache_miss_refuses_before_heap_effects();
        realm_template_cache_stays_owned_once_across_displaced_frames();
        realm_template_cache_joint_exact_capacity_and_one_byte_refusal();
        realm_template_cache_hits_allocate_nothing_at_a_full_native_budget();
        realm_template_cache_spare_entry_uses_no_fictitious_growth_charge();
        realm_template_cache_string_refusal_releases_detached_table_and_preserves_entries();
        realm_template_cache_heap_refusal_publishes_nothing_and_restores_operands();
        realm_template_cache_preserves_frozen_descriptors_and_invalid_cooked_escapes();
        realm_template_cache_rejection_and_repeated_await_keep_the_same_identity();
        realm_template_cache_same_compiled_site_has_independent_realm_objects();
        realm_template_cache_module_async_frame_observes_the_live_realm_cache();
    }
}
