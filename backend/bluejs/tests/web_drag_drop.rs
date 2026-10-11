// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

// Behavioral reference: https://html.spec.whatwg.org/multipage/dnd.html
use blueice_bluejs::{compile, parse, HostObjectKey, HostValue, RuntimeError, Value, Vm, VmConfig};

fn run(vm: &mut Vm, source: &str) -> Result<Value, RuntimeError> {
    vm.execute_script(&compile(&parse(source).unwrap()).unwrap())
}

fn browser_vm(config: VmConfig) -> Vm {
    let mut vm = Vm::new(config).unwrap();
    vm.install_web_file_api().unwrap();
    let family = vm.create_host_object_family().unwrap();
    vm.install_host_file_selection_event_methods(family)
        .unwrap();
    vm.install_host_object_factory("dragTarget", 0, family, |_: &[HostValue]| {
        Ok(Some(HostObjectKey::new(7, 2, 11)))
    })
    .unwrap();
    vm
}

#[test]
fn browser_dom_accepts_the_complete_drag_event_vocabulary_on_branded_nodes() {
    assert_eq!(
        run(
            &mut browser_vm(VmConfig::default()),
            r#"
            const target = dragTarget();
            const listener = function(event) {};
            for (const type of ['dragstart','drag','dragenter','dragover','dragleave','drop','dragend']) {
                target.addEventListener(type, listener);
                target.removeEventListener(type, listener);
            }
            let forged = false;
            try {
                Object.getPrototypeOf(target).addEventListener.call({}, 'drop', listener);
            } catch (error) { forged = error instanceof TypeError; }
            forged
            "#,
        )
        .unwrap(),
        Value::Bool(true),
    );
}

#[test]
fn constructed_data_transfer_normalizes_formats_appends_replacements_and_has_frozen_types() {
    assert_eq!(
        run(
            &mut browser_vm(VmConfig::default()),
            r#"
            const transfer = new DataTransfer();
            const empty = transfer.dropEffect === 'none' && transfer.effectAllowed === 'none'
                && transfer.items.length === 0 && transfer.files.length === 0;
            transfer.setData('TEXT', 'First 中文');
            transfer.setData('text/html', '<b>Public</b>');
            transfer.setData('text/plain', 'Replacement 😀');
            transfer.setData('URL', '# comment\r\nhttps://example.test/first\r\nhttps://example.test/second');
            const order = transfer.types.join(',') === 'text/html,text/plain,text/uri-list';
            const data = transfer.getData('text') === 'Replacement 😀'
                && transfer.getData('url') === 'https://example.test/first'
                && transfer.getData('TEXT/HTML') === '<b>Public</b>';
            transfer.effectAllowed = 'copyMove'; transfer.dropEffect = 'move';
            transfer.effectAllowed = 'invalid'; transfer.dropEffect = 'invalid';
            const effects = transfer.effectAllowed === 'copyMove' && transfer.dropEffect === 'move';
            transfer.clearData('TEXT');
            const removed = transfer.getData('text') === '' && transfer.types.join(',') === 'text/html,text/uri-list';
            transfer.clearData();
            empty && order && data && effects && removed && transfer.items.length === 0
                && Object.isFrozen(transfer.types)
            "#,
        )
        .unwrap(),
        Value::Bool(true),
    );
}

#[test]
fn data_transfer_items_keep_files_live_and_clear_data_removes_only_text_items() {
    let mut vm = browser_vm(VmConfig::default());
    assert_eq!(
        run(
            &mut vm,
            r#"
            const transfer = new DataTransfer();
            const items = transfer.items, files = transfer.files;
            const file = new File([new Uint8Array([0,255,7])], 'constructed.bin', {type:'APPLICATION/OCTET-STREAM',lastModified:123});
            const item = items.add(file);
            const text = items.add('Public text', 'TEXT/PLAIN');
            let duplicate = false;
            try { items.add('Second text', 'text/plain'); }
            catch (error) { duplicate = error instanceof DOMException && error instanceof Error
                && error.name === 'NotSupportedError' && error.code === 9; }
            const copied = item.getAsFile();
            const identity = transfer.items === items && transfer.files === files && files.length === 1
                && files[0] instanceof File && copied instanceof File && copied.name === file.name
                && copied.size === file.size && copied.type === file.type && text.getAsFile() === null;
            const metadata = item.kind === 'file' && item.type === 'application/octet-stream'
                && text.kind === 'string' && text.type === 'text/plain'
                && transfer.types.join(',') === 'text/plain,Files';
            transfer.clearData();
            const retained = items.length === 1 && files.length === 1 && transfer.types.join(',') === 'Files';
            let bytes = '';
            files[0].bytes().then(function(value) { bytes = Array.from(value).join(','); });
            items.clear();
            duplicate && identity && metadata && retained && files.length === 0 && items.length === 0
            "#,
        )
        .unwrap(),
        Value::Bool(true),
    );
    vm.run_promise_jobs().unwrap();
    assert_eq!(
        run(&mut vm, "bytes === '0,255,7'").unwrap(),
        Value::Bool(true)
    );
}

#[test]
fn synthetic_drag_events_keep_identity_coordinates_and_never_become_trusted() {
    assert_eq!(
        run(
            &mut browser_vm(VmConfig::default()),
            r#"
            const transfer = new DataTransfer();
            transfer.setData('text/plain', 'Page-owned data');
            const event = new DragEvent('drop', {dataTransfer:transfer,bubbles:true,cancelable:true,clientX:18,clientY:27});
            event.isTrusted = true;
            event.preventDefault();
            const passive = new DragEvent('dragover');
            passive.preventDefault();
            event instanceof DragEvent && event.type === 'drop' && event.dataTransfer === transfer
                && event.dataTransfer.getData('text') === 'Page-owned data'
                && event.bubbles && event.cancelable && event.defaultPrevented && !event.isTrusted
                && event.clientX === 18 && event.clientY === 27 && event.target === null && event.currentTarget === null
                && passive.dataTransfer === null && !passive.bubbles && !passive.cancelable
                && !passive.defaultPrevented && !passive.isTrusted
            "#,
        )
        .unwrap(),
        Value::Bool(true),
    );
}

#[test]
fn drag_payloads_survive_collection_and_methods_reject_forged_receivers() {
    let mut config = VmConfig::default();
    config.heap.nursery_capacity = 1;
    assert_eq!(
        run(
            &mut browser_vm(config),
            r#"
            let transfer = new DataTransfer();
            transfer.items.add(new File(['Retained 中文'], 'retained.txt', {type:'text/plain'}));
            transfer.setData('text/plain', 'Retained text');
            const event = new DragEvent('drop', {dataTransfer:transfer});
            const items = transfer.items, files = transfer.files;
            transfer = null;
            for (let i=0; i<30; i++) { new Blob(['Allocation ' + i]); }
            let forged = 0;
            for (const call of [
                function() { DataTransfer.prototype.getData.call({}, 'text/plain'); },
                function() { DataTransfer.prototype.setData.call({}, 'text/plain', 'x'); },
                function() { Object.getPrototypeOf(items).add.call({}, 'x', 'text/plain'); },
                function() { Object.getPrototypeOf(items[0]).getAsFile.call({}); }
            ]) {
                try { call(); } catch(error) { if(error instanceof TypeError) forged++; }
            }
            forged === 4 && event.dataTransfer.getData('text') === 'Retained text'
                && items.length === 2 && files.length === 1 && files[0].name === 'retained.txt'
                && items[0].getAsFile().name === files[0].name
                && items[0].getAsFile().size === files[0].size
            "#,
        )
        .unwrap(),
        Value::Bool(true),
    );
}

#[test]
fn dom_exceptions_keep_browser_brands_error_stack_and_legacy_codes() {
    assert_eq!(
        run(
            &mut browser_vm(VmConfig::default()),
            r#"
        const error = new DOMException('Duplicate format', 'NotSupportedError');
        const custom = new DOMException('Custom message', 'PageError');
        const empty = new DOMException();
        let forged = false;
        try { Object.getOwnPropertyDescriptor(DOMException.prototype, 'name').get.call({}); }
        catch(error) { forged = error instanceof TypeError; }
        error instanceof DOMException && error instanceof Error && Error.isError(error)
            && Error.isError(DOMException.prototype) && error.name === 'NotSupportedError'
            && error.message === 'Duplicate format' && error.code === 9
            && error.stack === 'NotSupportedError: Duplicate format'
            && custom.code === 0 && empty.name === 'Error' && empty.message === ''
            && DOMException.NOT_SUPPORTED_ERR === 9 && error.INVALID_STATE_ERR === 11
            && forged
    "#
        )
        .unwrap(),
        Value::Bool(true)
    );
}
