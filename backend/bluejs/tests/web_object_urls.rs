// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

use blueice_bluejs::{compile, parse, HostFunctionError, HostObjectUrls, Value, Vm};
use std::{cell::RefCell, rc::Rc};

fn run(vm: &mut Vm, source: &str) -> Value {
    vm.execute_script(&compile(&parse(source).unwrap()).unwrap())
        .unwrap()
}

#[derive(Default)]
struct Observed {
    created: Vec<(Vec<u8>, String)>,
    revoked: Vec<String>,
}
struct Host(Rc<RefCell<Observed>>);
impl HostObjectUrls for Host {
    fn create(&mut self, bytes: &[u8], mime_type: &str) -> Result<String, HostFunctionError> {
        let mut observed = self.0.borrow_mut();
        observed.created.push((bytes.to_vec(), mime_type.into()));
        Ok(format!(
            "blob:https://example.test/{}",
            observed.created.len()
        ))
    }
    fn revoke(&mut self, url: &str) -> Result<(), HostFunctionError> {
        self.0.borrow_mut().revoked.push(url.into());
        Ok(())
    }
}

#[test]
fn url_is_a_branded_constructor_with_canonical_parsing_and_serialization() {
    let mut vm = Vm::default();
    vm.install_web_url_api().unwrap();
    assert_eq!(
        run(
            &mut vm,
            r#"
        const url = new URL('../中文?q=hello world#片段', 'https://EXAMPLE.test:443/a/b');
        const original = url instanceof URL && url.origin === 'https://example.test'
            && url.hostname === 'example.test' && url.port === '' && url.protocol === 'https:'
            && url.pathname === '/%E4%B8%AD%E6%96%87' && url.search === '?q=hello%20world'
            && url.hash === '#%E7%89%87%E6%AE%B5' && String(url) === url.href && url.toJSON() === url.href;
        let rejected = 0;
        for (const action of [() => URL('https://example.test/'), () => new URL('relative'),
            () => URL.prototype.toString.call({href:'forged'})]) {
            try { action(); } catch(error) { if(error instanceof TypeError) rejected++; }
        }
        original && rejected === 3 && URL.canParse('/ok', 'https://example.test/')
            && !URL.canParse('relative') && URL.parse('relative') === null
            && URL.parse('https://example.test/') instanceof URL
    "#
        ),
        Value::Bool(true)
    );
}

#[test]
fn object_url_calls_export_only_branded_blob_bytes_and_convert_revoke_to_usvstring() {
    let observed = Rc::new(RefCell::new(Observed::default()));
    let mut vm = Vm::default();
    vm.install_host_object_urls(Host(observed.clone())).unwrap();
    assert_eq!(
        run(
            &mut vm,
            r#"
        const blob = new Blob([new Uint8Array([7,0,255])], {type:'TEXT/PLAIN'});
        const url = URL.createObjectURL(blob);
        let rejected = 0;
        for (const action of [() => URL.createObjectURL({size:3,type:'text/plain'}),
            () => URL.createObjectURL(), () => new URL.createObjectURL(blob), () => URL.revokeObjectURL()]) {
            try { action(); } catch(error) { if(error instanceof TypeError) rejected++; }
        }
        const returned = URL.revokeObjectURL({toString() { return url + '\uD800'; }});
        url === 'blob:https://example.test/1' && rejected === 4 && returned === undefined
    "#
        ),
        Value::Bool(true)
    );
    let observed = observed.borrow();
    assert_eq!(observed.created, [(vec![7, 0, 255], "text/plain".into())]);
    assert_eq!(
        observed.revoked,
        ["blob:https://example.test/1\u{fffd}".to_string()]
    );
}

#[test]
fn url_setters_and_subclasses_survive_collection_and_reentrant_string_conversion() {
    use blueice_bluejs::{HeapConfig, VmConfig};
    let mut vm = Vm::new(VmConfig {
        heap: HeapConfig {
            nursery_capacity: 1,
            major_threshold_bytes: 1,
            max_heap_bytes: 1_000_000,
        },
        ..VmConfig::default()
    })
    .unwrap();
    vm.install_web_url_api().unwrap();
    assert_eq!(
        run(
            &mut vm,
            r#"
        class PageURL extends URL {}
        const url = new PageURL('https://example.test:443/a?q=one#old');
        url.search = { toString() { url.hostname = 'changed.test'; return '?q=a b'; } };
        url.pathname = '/中文'; url.hash = '';
        url.port = '99999';
        let invalid = false;
        try { url.href = 'relative'; } catch(error) { invalid = error instanceof TypeError; }
        for(let index=0; index<30; index++) { new URL('/'+index, url); }
        invalid && url instanceof PageURL && url instanceof URL
            && url.href === 'https://changed.test/%E4%B8%AD%E6%96%87?q=a%20b'
            && url.origin === 'https://changed.test' && url.port === '' && url.hash === ''
    "#
        ),
        Value::Bool(true)
    );
}

#[test]
fn url_web_idl_members_have_enumerable_configurable_descriptors() {
    let mut vm = Vm::default();
    vm.install_web_url_api().unwrap();
    assert_eq!(
        run(
            &mut vm,
            r#"
        const href = Object.getOwnPropertyDescriptor(URL.prototype, 'href');
        const origin = Object.getOwnPropertyDescriptor(URL.prototype, 'origin');
        const method = Object.getOwnPropertyDescriptor(URL.prototype, 'toJSON');
        const create = Object.getOwnPropertyDescriptor(URL, 'createObjectURL');
        href.enumerable && href.configurable && typeof href.get === 'function' && typeof href.set === 'function'
            && origin.enumerable && origin.set === undefined && method.enumerable && method.writable
            && create.enumerable && create.writable && create.configurable
    "#
        ),
        Value::Bool(true)
    );
}
