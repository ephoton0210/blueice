// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

use blueice_bluets::{CompilerOptions, MapLoader, ModuleSource, RuntimePolicy};
use blueice_bluets_bluejs::{
    compile_direct_script,
    page_host_typings::{page_host_dom_iteration_runtime_bindings_v10, PageHostDocumentTypingsV1},
};

#[test]
fn iteration_profile_preserves_tee_and_checks_iterator_calls_and_options() {
    let old = PageHostDocumentTypingsV1::generate_dom_tee();
    let profile = PageHostDocumentTypingsV1::generate_dom_iteration();
    let bindings = page_host_dom_iteration_runtime_bindings_v10();
    assert_eq!(bindings.len(), 40);
    assert!(!old
        .declaration_source
        .contains("interface BlueIceReadableStreamIterable"));
    assert!(profile.verify_runtime_bindings(&bindings[..39]).is_err());
    let ambient = profile
        .verified_dom_iteration_ambient_module(&bindings)
        .unwrap();
    for(source,accepted)in[
        ("function read(stream: BlueIceReadableStream): BlueIceFilePromise<BlueIceStreamReadResult> { return stream.values({preventCancel:true}).next(); }",true),
        ("function close(stream: BlueIceReadableStream): BlueIceFilePromise<BlueIceStreamReadResult> { return stream.values().return('stop'); }",true),
        ("function branch(stream: BlueIceReadableStream): BlueIceReadableStream { return stream.tee()[1]; }",true),
        ("function bad(stream: BlueIceReadableStream): void { stream.values({preventCancel:7}); }",false),
        ("function bad(stream: BlueIceReadableStream): void { stream.values(7); }",false),
        ("function bad(stream: BlueIceReadableStream): number { return stream.values().next(); }",false),
    ]{
        let result=compile_direct_script("memory:///iteration.ts",&MapLoader::from([ModuleSource::new("memory:///iteration.ts",source)]),CompilerOptions{runtime_policy:RuntimePolicy::Checked,require_declared_global_calls:true,ambient_declaration_modules:vec![ambient.clone()],..CompilerOptions::default()});
        assert_eq!(result.is_ok(),accepted,"{source}: {:?}",result.as_ref().err());
    }
}

#[test]
fn typed_iterator_return_executes_the_native_keyword_named_method() {
    let profile = PageHostDocumentTypingsV1::generate_dom_iteration();
    let ambient = profile
        .verified_dom_iteration_ambient_module(&page_host_dom_iteration_runtime_bindings_v10())
        .unwrap();
    let source="let completed: boolean = false; function record(result: BlueIceStreamReadResult): void { completed = result.done; } const stream = new ReadableStream(); const iterator = stream.values(); iterator.return('stop').then(record); completed;";
    let script = compile_direct_script(
        "memory:///return.ts",
        &MapLoader::from([ModuleSource::new("memory:///return.ts", source)]),
        CompilerOptions {
            runtime_policy: RuntimePolicy::Checked,
            require_declared_global_calls: true,
            ambient_declaration_modules: vec![ambient],
            ..CompilerOptions::default()
        },
    )
    .unwrap();
    let mut vm = blueice_bluejs::Vm::default();
    vm.install_web_file_api().unwrap();
    vm.execute_script(&script.bytecode).unwrap();
    vm.run_browser_tasks_bounded(128).unwrap();
    let probe =
        blueice_bluejs::compile(&blueice_bluejs::parse("completed && !stream.locked").unwrap())
            .unwrap();
    assert_eq!(
        vm.execute_script(&probe).unwrap(),
        blueice_bluejs::Value::Bool(true)
    );
}
