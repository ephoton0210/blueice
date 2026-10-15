// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

use blueice_bluets::{CompilerOptions, MapLoader, ModuleSource, RuntimePolicy};
use blueice_bluets_bluejs::{
    compile_direct_script,
    page_host_typings::{
        page_host_dom_console_runtime_bindings_v14, page_host_dom_window_runtime_bindings_v13,
        PageHostDocumentTypingsV1,
    },
};
#[test]
fn console_profile_keeps_the_old_inventory_and_checks_namespace_and_window_members() {
    let profile = PageHostDocumentTypingsV1::generate_dom_console();
    let bindings = page_host_dom_console_runtime_bindings_v14();
    assert_eq!(bindings.len(), 52);
    assert_eq!(
        &bindings[..51],
        &page_host_dom_window_runtime_bindings_v13()
    );
    assert!(profile
        .verified_dom_console_ambient_module(&bindings[..51])
        .is_err());
    let ambient = profile
        .verified_dom_console_ambient_module(&bindings)
        .unwrap();
    for (source,accepted) in [
        ("console.log('x', 1, true, {a: 1}); window.console.warn('%d', 2); self.console.error(new Error('page'));",true),
        ("console.assert(false, 'missing'); console.count(); console.countReset('x'); console.time('x'); console.timeLog('x', 1); console.timeEnd('x');",true),
        ("console.group('x'); console.groupCollapsed(); console.groupEnd(); console.table([{x: 1}], ['x']); console.dir({x: 1}); console.dirxml('x'); console.trace(); console.clear();",true),
        ("console.time(7);",false), ("window.console.countReset(1);",false),
        ("console.assert('not a boolean');",false), ("window.console.table({}, [7]);",false),
        ("console.missing();",false),
    ] {
        let result=compile_direct_script("memory:///console.ts",&MapLoader::from([ModuleSource::new("memory:///console.ts",source)]),CompilerOptions{runtime_policy:RuntimePolicy::Checked,require_declared_global_calls:true,ambient_declaration_modules:vec![ambient.clone()],..CompilerOptions::default()});
        assert_eq!(result.is_ok(),accepted,"{source}: {:?}",result.err());
    }
    assert!(!PageHostDocumentTypingsV1::generate_dom_window()
        .declaration_source
        .contains("BlueIceConsole"));
}
