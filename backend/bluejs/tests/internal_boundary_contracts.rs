// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

//! Execute valid internal boundary fixtures in the ordinary instrumented
//! library, so raw coverage is not supplied only by a unit-test instantiation.

#[cfg(coverage)]
#[test]
fn ordinary_library_synthetic_module_boundary_contracts() {
    blueice_bluejs::Vm::verify_synthetic_module_boundary_contracts();
}

#[cfg(coverage)]
#[test]
fn ordinary_library_module_source_ownership_contracts() {
    blueice_bluejs::Vm::verify_module_source_ownership_contracts();
}

#[cfg(coverage)]
#[test]
fn ordinary_library_module_context_ownership_contracts() {
    blueice_bluejs::Vm::verify_module_context_ownership_contracts();
}

#[cfg(coverage)]
#[test]
fn ordinary_library_module_referrer_ownership_contracts() {
    blueice_bluejs::Vm::verify_module_referrer_ownership_contracts();
}

#[cfg(coverage)]
#[test]
fn ordinary_library_realm_template_cache_contracts() {
    blueice_bluejs::Vm::verify_realm_template_cache_contracts();
}

#[cfg(coverage)]
#[test]
fn ordinary_library_generator_entry_admission_contracts() {
    blueice_bluejs::Vm::verify_generator_entry_admission_contracts();
}

#[cfg(coverage)]
#[test]
fn ordinary_library_suspension_storage_contracts() {
    blueice_bluejs::Vm::verify_suspension_storage_contracts();
}

#[cfg(coverage)]
#[test]
fn ordinary_library_generator_resume_admission_contracts() {
    blueice_bluejs::Vm::verify_generator_resume_admission_contracts();
}

#[cfg(coverage)]
#[test]
fn ordinary_library_global_binding_admission_contracts() {
    blueice_bluejs::Vm::verify_global_binding_admission_contracts();
}

#[cfg(coverage)]
#[test]
fn ordinary_library_dynamic_binding_admission_contracts() {
    blueice_bluejs::Vm::verify_dynamic_binding_admission_contracts();
}

#[cfg(coverage)]
#[test]
fn ordinary_library_environment_admission_contracts() {
    blueice_bluejs::Vm::verify_environment_admission_contracts();
}

#[cfg(coverage)]
#[test]
fn ordinary_library_generator_storage_ownership_contracts() {
    blueice_bluejs::Vm::verify_generator_storage_ownership_contracts();
}

#[cfg(coverage)]
#[test]
fn ordinary_library_frame_map_admission_contracts() {
    blueice_bluejs::Vm::verify_frame_map_admission_contracts();
}

#[cfg(coverage)]
#[test]
fn ordinary_library_scope_admission_contracts() {
    blueice_bluejs::Vm::verify_scope_admission_contracts();
}

#[cfg(coverage)]
#[test]
fn ordinary_library_frame_binding_admission_contracts() {
    blueice_bluejs::Vm::verify_frame_binding_admission_contracts();
}

#[cfg(coverage)]
#[test]
fn ordinary_library_call_entry_storage_contracts() {
    blueice_bluejs::Vm::verify_call_entry_storage_contracts();
}

#[cfg(coverage)]
#[test]
fn ordinary_library_disposal_ownership_contracts() {
    blueice_bluejs::Vm::verify_disposal_ownership_contracts();
}

#[cfg(coverage)]
#[test]
fn ordinary_library_interpreter_record_ownership_contracts() {
    blueice_bluejs::Vm::verify_interpreter_record_ownership_contracts();
}

#[cfg(coverage)]
#[test]
fn ordinary_library_execution_control_capacity_contracts() {
    blueice_bluejs::Vm::verify_execution_control_capacity_contracts();
}

#[cfg(coverage)]
#[test]
fn ordinary_library_module_dependency_ownership_contracts() {
    blueice_bluejs::Vm::verify_module_dependency_ownership_contracts();
}

#[cfg(coverage)]
#[test]
fn ordinary_library_import_waiter_capacity_contracts() {
    blueice_bluejs::Vm::verify_import_waiter_capacity_contracts();
}

#[cfg(coverage)]
#[test]
fn ordinary_library_promise_table_capacity_contracts() {
    blueice_bluejs::Vm::verify_promise_table_capacity_contracts();
}

#[cfg(coverage)]
#[test]
fn ordinary_library_browser_task_capacity_contracts() {
    blueice_bluejs::Vm::verify_browser_task_capacity_contracts();
}

#[cfg(coverage)]
#[test]
fn ordinary_library_scheduling_capacity_contracts() {
    blueice_bluejs::Vm::verify_scheduling_capacity_contracts();
}

#[cfg(coverage)]
#[test]
fn ordinary_library_document_timer_count_contracts() {
    blueice_bluejs::Vm::verify_document_timer_count_contracts();
}

#[cfg(coverage)]
#[test]
fn ordinary_library_task_payload_handoff_contracts() {
    blueice_bluejs::Vm::verify_task_payload_handoff_contracts();
}

#[cfg(coverage)]
#[test]
fn ordinary_library_remaining_region_boundary_contracts() {
    blueice_bluejs::Vm::verify_remaining_region_boundary_contracts();
}

#[cfg(coverage)]
#[test]
fn ordinary_library_test262_assertion_boundary_contracts() {
    blueice_bluejs::Vm::verify_test262_assertion_boundary_contracts();
}

#[cfg(coverage)]
#[test]
fn ordinary_library_heap_object_storage_boundary_contracts() {
    blueice_bluejs::Heap::verify_object_storage_boundary_contracts();
}

#[cfg(coverage)]
#[test]
fn ordinary_library_common_boundary_contracts() {
    blueice_bluejs::Vm::verify_common_boundary_contracts();
    blueice_bluejs::Program::verify_payload_accounting_contracts();
}

#[cfg(coverage)]
#[test]
fn ordinary_library_page_runtime_contracts() {
    blueice_bluejs::BlueJsPageRuntime::verify_retained_page_runtime_contracts();
    blueice_bluejs::BlueJsPageRuntime::verify_page_runtime_boundary_contracts();
}

#[cfg(coverage)]
#[test]
fn ordinary_library_object_boundary_contracts() {
    blueice_bluejs::Vm::verify_object_boundary_contracts();
}

#[cfg(coverage)]
#[test]
fn ordinary_library_execution_boundary_contracts() {
    blueice_bluejs::Vm::verify_agent_host_boundary_contracts();
    blueice_bluejs::Vm::verify_execution_boundary_contracts();
    blueice_bluejs::Vm::verify_dynamic_function_boundary_contracts();
}

#[cfg(coverage)]
#[test]
fn ordinary_library_tail_eval_result_boundary_contracts() {
    blueice_bluejs::Vm::verify_tail_eval_result_boundary_contracts();
}

#[cfg(coverage)]
#[test]
fn ordinary_library_d1_vm_boundary_contracts() {
    blueice_bluejs::Vm::verify_d1_vm_boundary_contracts();
}

#[cfg(coverage)]
#[test]
fn ordinary_library_regex_pool_boundary_contracts() {
    blueice_bluejs::regex_worker::verify_regex_pool_boundary_contracts();
}

#[cfg(coverage)]
#[test]
fn ordinary_library_agent_receive_callback_boundary_contracts() {
    blueice_bluejs::Vm::verify_agent_receive_callback_boundary_contracts();
}

#[cfg(coverage)]
#[test]
fn ordinary_library_temporal_boundary_contracts() {
    blueice_bluejs::Vm::verify_temporal_string_boundary_contracts();
    blueice_bluejs::Vm::verify_plain_difference_boundary_contracts();
    blueice_bluejs::Vm::verify_zoned_difference_boundary_contracts();
}

#[cfg(coverage)]
#[test]
fn ordinary_library_array_from_async_boundary_contracts() {
    blueice_bluejs::Vm::verify_array_from_async_boundary_contracts();
}

#[cfg(coverage)]
#[test]
fn ordinary_library_generator_delegation_boundary_contracts() {
    blueice_bluejs::Vm::verify_generator_delegation_boundary_contracts();
}

#[cfg(coverage)]
#[test]
fn ordinary_library_binary_data_boundary_contracts() {
    blueice_bluejs::Vm::verify_binary_data_boundary_contracts();
    blueice_bluejs::Vm::verify_typed_array_boundary_contracts();
}

#[cfg(coverage)]
#[test]
fn ordinary_library_retained_internal_contracts() {
    blueice_bluejs::Vm::verify_foreign_realm_boundary_contracts();
    blueice_bluejs::Vm::verify_shadow_boundary_contracts();
    blueice_bluejs::Vm::verify_error_boundary_contracts();
    blueice_bluejs::Vm::verify_retained_vm_contracts();
    blueice_bluejs::Vm::verify_closure_execution_contracts();
    blueice_bluejs::Vm::verify_module_linking_contracts();
    blueice_bluejs::Vm::verify_plain_difference_arithmetic_contracts();
    blueice_bluejs::Vm::verify_zoned_difference_arithmetic_contracts();
    blueice_bluejs::Vm::verify_debugger_lifecycle_contracts();
}

#[cfg(coverage)]
#[test]
fn ordinary_library_independent_boundary_contracts() {
    blueice_bluejs::Vm::verify_independent_boundary_contracts();
}
