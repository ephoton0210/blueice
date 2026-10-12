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
fn stream_profile_preserves_reader_profile_and_checks_locking_and_byob_types() {
    use blueice_bluets_bluejs::page_host_typings::page_host_dom_stream_runtime_bindings_v4;
    let old = PageHostDocumentTypingsV1::generate_dom_file_reader();
    let profile = PageHostDocumentTypingsV1::generate_dom_stream();
    let bindings = page_host_dom_stream_runtime_bindings_v4();
    assert_eq!(bindings.len(), 33);
    assert!(!old
        .declaration_source
        .contains("declare const ReadableStream:"));
    assert!(profile.verify_runtime_bindings(&bindings[..32]).is_err());
    let ambient = profile
        .verified_dom_stream_ambient_module(&bindings)
        .unwrap();
    for (source,accepted) in [
        ("function consume(blob: BlueIceBlob): void { const stream = blob.stream(); const reader = stream.getReader(); reader.read(); reader.releaseLock(); }",true),
        ("function consume(reader: BlueIceReadableStreamBYOBReader, view: BlueIceArrayBufferView): void { reader.read(view, {min: 2}); }",true),
        ("function bad(reader: BlueIceReadableStreamBYOBReader): void { reader.read('bytes'); }",false),
        ("function bad(stream: BlueIceReadableStream): void { stream.locked = true; }",false),
    ] {
        let result=compile_direct_script("memory:///streams.ts",&MapLoader::from([ModuleSource::new("memory:///streams.ts",source)]),CompilerOptions {
            runtime_policy:RuntimePolicy::Checked,require_declared_global_calls:true,ambient_declaration_modules:vec![ambient.clone()],..CompilerOptions::default()
        });assert_eq!(result.is_ok(),accepted,"{source}: {:?}",result.as_ref().err());
    }
}

#[test]
fn file_reader_profile_checks_events_and_readonly_state_with_an_exact_inventory() {
    use blueice_bluets_bluejs::page_host_typings::page_host_dom_file_reader_runtime_bindings_v3;
    let old = PageHostDocumentTypingsV1::generate_dom_drag_image();
    let profile = PageHostDocumentTypingsV1::generate_dom_file_reader();
    let bindings = page_host_dom_file_reader_runtime_bindings_v3();
    assert_eq!(bindings.len(), 27);
    assert!(profile.verify_runtime_bindings(&bindings[..26]).is_err());
    assert!(!old.declaration_source.contains("declare const FileReader:"));
    let ambient = profile
        .verified_dom_file_reader_ambient_module(&bindings)
        .unwrap();
    for (source, accepted) in [
        ("function progress(event: BlueIceProgressEvent): void { const loaded: number = event.loaded; } function read(reader: BlueIceFileReader, blob: BlueIceBlob): void { reader.readAsText(blob, 'big5'); reader.onload = progress; reader.addEventListener('progress', progress); }", true),
        ("function read(reader: BlueIceFileReader): void { reader.readAsText('not a Blob'); }", false),
        ("function forge(reader: BlueIceFileReader): void { reader.readyState = 2; }", false),
        ("function dispatch(target: BlueIceEventTarget, event: BlueIceEvent): boolean { return target.dispatchEvent(event); }", true),
    ] {
        let result = compile_direct_script("memory:///reader.ts", &MapLoader::from([ModuleSource::new("memory:///reader.ts", source)]), CompilerOptions {
            runtime_policy: RuntimePolicy::Checked,
            require_declared_global_calls: true,
            ambient_declaration_modules: vec![ambient.clone()],
            ..CompilerOptions::default()
        });
        assert_eq!(result.is_ok(), accepted, "{source}: {:?}", result.as_ref().err());
    }
    assert_eq!(
        old.declaration_hash,
        PageHostDocumentTypingsV1::generate_dom_drag_image().declaration_hash
    );
}

#[test]
fn image_profile_adds_checked_set_drag_image_without_changing_the_v1_profile() {
    let original = PageHostDocumentTypingsV1::generate_dom_drag();
    assert!(!original.declaration_source.contains("setDragImage("));
    let image = PageHostDocumentTypingsV1::generate_dom_drag_image();
    let bindings =
        blueice_bluets_bluejs::page_host_typings::page_host_dom_drag_image_runtime_bindings_v2();
    let ambient = image
        .verified_dom_drag_image_ambient_module(&bindings)
        .unwrap();
    for source in [
        "new DataTransfer().setDragImage(document.getElementById('source')!, 4, 8);",
        "new DataTransfer().setDragImage('source', 4, 8);",
    ] {
        let result = compile_direct_script(
            "memory:///image.ts",
            &MapLoader::from([ModuleSource::new("memory:///image.ts", source)]),
            CompilerOptions {
                runtime_policy: RuntimePolicy::Checked,
                require_declared_global_calls: true,
                ambient_declaration_modules: vec![ambient.clone()],
                ..CompilerOptions::default()
            },
        );
        assert_eq!(
            result.is_ok(),
            source.starts_with("new DataTransfer().setDragImage(document")
        );
    }
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
