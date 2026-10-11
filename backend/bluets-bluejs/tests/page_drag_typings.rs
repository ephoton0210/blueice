// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

use blueice_bluejs::{Value, Vm, VmConfig};
use blueice_bluets::{CompilerOptions, MapLoader, ModuleSource, RuntimePolicy};
use blueice_bluets_bluejs::page_host_typings::{
    page_host_dom_drag_runtime_bindings_v1, page_host_dom_file_runtime_bindings_v1,
    PageHostDocumentTypingsV1, PAGE_HOST_DOM_DRAG_DECLARATION_MODULE_ID_V1,
    PAGE_HOST_DOM_DRAG_PROFILE_V1,
};
use blueice_bluets_bluejs::{compile_direct_script, BridgeError, DirectScript};

fn compile_drag(source: &str) -> Result<DirectScript, BridgeError> {
    let ambient = PageHostDocumentTypingsV1::generate_dom_drag()
        .verified_dom_drag_ambient_module(&page_host_dom_drag_runtime_bindings_v1())
        .unwrap();
    compile_direct_script(
        "memory:///drag.ts",
        &MapLoader::from([ModuleSource::new("memory:///drag.ts", source)]),
        CompilerOptions {
            runtime_policy: RuntimePolicy::Checked,
            require_declared_global_calls: true,
            ambient_declaration_modules: vec![ambient],
            ..CompilerOptions::default()
        },
    )
}

#[test]
fn drag_profile_has_an_exact_inventory_and_preserves_the_earlier_file_profile() {
    let file = PageHostDocumentTypingsV1::generate_dom_file();
    let drag = PageHostDocumentTypingsV1::generate_dom_drag();
    let bindings = page_host_dom_drag_runtime_bindings_v1();
    assert_eq!(file.runtime_bindings.len(), 16);
    assert!(!file.declaration_source.contains("declare const DragEvent:"));
    assert_eq!(drag.profile, PAGE_HOST_DOM_DRAG_PROFILE_V1);
    assert_ne!(drag.declaration_hash, file.declaration_hash);
    assert_eq!(bindings.len(), 24);
    assert!(drag.verify_runtime_bindings(&bindings[..23]).is_err());
    assert!(drag
        .verify_runtime_bindings(&page_host_dom_file_runtime_bindings_v1())
        .is_err());
    let ambient = drag.verified_dom_drag_ambient_module(&bindings).unwrap();
    assert_eq!(ambient.id, PAGE_HOST_DOM_DRAG_DECLARATION_MODULE_ID_V1);
    let mut changed = drag.clone();
    changed
        .declaration_source
        .push_str("\ndeclare const OSPath: string;\n");
    assert!(changed.verified_dom_drag_ambient_module(&bindings).is_err());
    let mut duplicate = bindings.to_vec();
    duplicate.push(bindings[23]);
    assert!(drag.verify_runtime_bindings(&duplicate).is_err());
}

#[test]
fn typed_drag_callbacks_use_only_installed_methods_and_readonly_event_state() {
    for source in [
        "function over(event: BlueIceDragEvent): void { event.preventDefault(); event.dataTransfer!.dropEffect = 'copy'; const x: number = event.clientX; const caps: boolean = event.getModifierState('CapsLock'); const kinds: boolean = event.dataTransfer!.types.includes('text/plain'); } document.getElementById('drop')!.addEventListener('dragover', over, {capture: true, once: true, passive: false}); document.removeEventListener('dragover', over, true);",
        "function dropped(event: BlueIceDragEvent): void { event.stopImmediatePropagation(); const file: BlueIceFile | null = event.dataTransfer!.files.item(0); } const listener: BlueIceDragEventListener = {handleEvent: dropped}; document.addEventListener('drop', listener); document.removeEventListener('drop', listener, {capture: false});",
        "const transfer = new DataTransfer(); const text: BlueIceDataTransferItem | null = transfer.items.add('data', 'text/plain'); const file: BlueIceDataTransferItem | null = transfer.items.add(new File(['data'], 'data.txt')); transfer.items.remove(0); transfer.items.clear(); transfer.clearData();",
        "const event = new DragEvent('drop', {clientX: 1.25, buttons: 1, ctrlKey: true, modifierCapsLock: true, dataTransfer: new DataTransfer()}); const base: BlueIceEvent = event; event.initMouseEvent('legacy', false, true, null, 2, 0, 0, 4, 5); event.initUIEvent('ui'); event.composedPath(); const phase: number = Event.AT_TARGET;",
        "const error = new DOMException('duplicate', 'NotSupportedError'); const code: number = error.code; const name: string = error.name;",
    ] {
        compile_drag(source).unwrap_or_else(|error| panic!("{source}: {error:?}"));
    }
    for source in [
        "document.addEventListener('unknown-drag', () => {});",
        "function dropped(event: BlueIceDragEvent): void {} document.addEventListener('drop', dropped, {signal: {}});",
        "new DragEvent('drop').clientX = 99;",
        "new DragEvent('drop').isTrusted = true;",
        "new DataTransfer().files = null;",
        "new DataTransfer().types.length = 0;",
        "new DataTransfer().items.add('text');",
        "new DataTransfer().setData('text/plain', 1);",
        "new DataTransferItemList();",
        "new DataTransfer().setDragImage(document.getElementById('image')!, 0, 0);",
        "document.dispatchEvent(new DragEvent('drop'));",
    ] {
        assert!(compile_drag(source).is_err(), "{source}");
    }
}

#[test]
fn checked_drag_constructors_lower_directly_and_execute_the_real_vm_interfaces() {
    let script = compile_drag(
        "const transfer = new DataTransfer(); transfer.setData('TEXT', 'owned 中文'); const event = new DragEvent('drop', {dataTransfer: transfer, clientX: 1.25, ctrlKey: true}); const result: boolean = event.dataTransfer!.getData('text/plain') === 'owned 中文' && event.clientX === 1.25 && event.getModifierState('Control') && !event.isTrusted; result;",
    )
    .unwrap();
    let mut vm = Vm::new(VmConfig::default()).unwrap();
    vm.install_web_file_api().unwrap();
    assert_eq!(
        vm.execute_script(&script.bytecode).unwrap(),
        Value::Bool(true)
    );
}
