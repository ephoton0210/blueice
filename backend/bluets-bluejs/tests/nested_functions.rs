// This Source Code Form is subject to the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

use blueice_bluejs::{Value, Vm};
use blueice_bluets::{CompilerOptions, MapLoader, ModuleSource, RuntimePolicy};
use blueice_bluets_bluejs::{
    compile_direct_script,
    page_host_typings::{page_host_dom_node_event_runtime_bindings_v8, PageHostDocumentTypingsV1},
    DirectScript,
};

fn compile(source: &str) -> Result<DirectScript, blueice_bluets_bluejs::BridgeError> {
    let bindings = page_host_dom_node_event_runtime_bindings_v8();
    let ambient = PageHostDocumentTypingsV1::generate_dom_node_event()
        .verified_dom_node_event_ambient_module(&bindings)
        .unwrap();
    compile_direct_script(
        "memory:///nested.ts",
        &MapLoader::from([ModuleSource::new("memory:///nested.ts", source)]),
        CompilerOptions {
            runtime_policy: RuntimePolicy::Checked,
            require_declared_global_calls: true,
            ambient_declaration_modules: vec![ambient],
            ..CompilerOptions::default()
        },
    )
}

#[test]
fn nested_functions_capture_outer_values_and_are_hoisted() {
    for source in [
        "function outer(value: number): number { function inner(delta: number): number { return value + delta; } return inner(2); } outer(5);",
        "function outer(): number { return inner(4); function inner(value: number): number { return value + 3; } } outer();",
        "class A { run(value: number): number { function inner(delta: number): number { return value + delta; } return inner(2); } } new A().run(5);",
        "function outer(value: number): number { try { function inner(delta: number): number { return value + delta; } return inner(2); } finally {} } outer(5);",
        "function outer(value: number): number { while (value < 8) { function inner(delta: number): number { return value + delta; } return inner(2); } return 0; } outer(5);",
    ]{
        let script=compile(source).unwrap();
        assert_eq!(Vm::default().execute_script(&script.bytecode).unwrap(),Value::Number(7.0));
    }
}

#[test]
fn nested_function_signatures_shadow_locally_without_leaking_to_siblings() {
    let source="function inner(value: string): string { return value; } function outer(): number { function inner(value: number): number { return value + 3; } return inner(4); } outer();";
    let script = compile(source).unwrap();
    assert_eq!(
        Vm::default().execute_script(&script.bytecode).unwrap(),
        Value::Number(7.0)
    );
    assert!(compile(&format!("{source} inner(2);")).is_err());
    assert!(compile("function outer(): number { function hidden(value: number): number { return value; } return hidden(1); } function sibling(): number { return hidden(2); } sibling();").is_err());
    assert!(compile("function outer(): number { function inner(value: number): number { return 'bad'; } return inner(4); } outer();").is_err());
}

#[test]
fn nested_event_callbacks_execute_with_their_real_lexical_state() {
    let script=compile("function dispatch(): number { const target = new EventTarget(); let count: number = 0; function handler(event: BlueIceEvent): void { count += 1; } target.addEventListener('custom', handler); target.dispatchEvent(new CustomEvent('custom', {detail: 7})); return count; } dispatch();").unwrap();
    let mut vm = Vm::default();
    vm.install_web_file_api().unwrap();
    assert_eq!(
        vm.execute_script(&script.bytecode).unwrap(),
        Value::Number(1.0)
    );
}

#[test]
fn nested_function_depth_reports_a_resource_error_instead_of_recursing_unboundedly() {
    for depth in [63, 64, 65, 70] {
        let mut source = String::from("function outer(): number {");
        for index in 0..depth {
            source.push_str(&format!("function f{index}(): number {{"));
        }
        source.push_str("return 1;");
        for index in (0..depth).rev() {
            source.push_str(&format!("}} return f{index}();"));
        }
        source.push_str("} outer();");
        let result = compile(&source);
        if depth <= 64 {
            assert!(result.is_ok(), "depth {depth}: {:?}", result.err());
        } else {
            let error = result.err().expect("Depth must be rejected");
            assert!(format!("{error:?}").contains("ResourceLimit"));
        }
    }
}
