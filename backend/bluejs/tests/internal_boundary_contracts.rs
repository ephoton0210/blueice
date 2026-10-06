// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

//! Execute valid internal boundary fixtures in the ordinary instrumented
//! library, so raw coverage is not supplied only by a unit-test instantiation.

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
