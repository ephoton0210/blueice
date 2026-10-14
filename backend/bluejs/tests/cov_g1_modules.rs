// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

//! Synthetic (`with { type }`) module requests whose specifier cannot be
//! resolved, and bytes modules that exceed the buffer size limit or a small
//! heap.

use blueice_bluejs::{
    compile_module, parse_module, Bytecode, HeapConfig, RuntimeError, Value, Vm, VmConfig,
};
use std::collections::HashMap;

fn build(sources: &[(&str, &str)]) -> HashMap<String, Bytecode> {
    sources
        .iter()
        .map(|(name, source)| {
            let module = parse_module(source).unwrap_or_else(|e| panic!("{name}: {e:?}"));
            (
                format!("t/{name}"),
                compile_module(&module).unwrap_or_else(|e| panic!("{name}: {e:?}")),
            )
        })
        .collect()
}

fn resolution_error(source: &str) -> Result<Value, RuntimeError> {
    let mut vm = Vm::default();
    vm.execute_module_graph("t/main.js", &build(&[("main.js", source)]))
}

#[test]
fn an_import_cannot_escape_the_host_root() {
    assert_eq!(
        resolution_error("import v from '../../data.json' with { type: 'json' }; v"),
        Err(RuntimeError::ModuleResolution(
            "relative module request ../../data.json escapes its host root".into()
        ))
    );
}

#[test]
fn a_re_export_cannot_escape_the_host_root() {
    assert_eq!(
        resolution_error("export { default as v } from '../../data.txt' with { type: 'text' };"),
        Err(RuntimeError::ModuleResolution(
            "relative module request ../../data.txt escapes its host root".into()
        ))
    );
}

#[test]
fn a_specifier_cannot_smuggle_a_module_type_separator() {
    let message =
        RuntimeError::ModuleResolution("a module specifier cannot contain a NUL character".into());
    assert_eq!(
        resolution_error("import v from './a\\0b.json' with { type: 'json' }; v"),
        Err(message.clone())
    );
    assert_eq!(
        resolution_error("export * from './a\\0b.txt' with { type: 'text' };"),
        Err(message)
    );
}

#[test]
fn a_bytes_resource_larger_than_the_buffer_limit_is_rejected() {
    let mut vm = Vm::new(VmConfig {
        heap: HeapConfig {
            nursery_capacity: 256,
            major_threshold_bytes: 262_144,
            max_heap_bytes: 262_144,
        },
        ..VmConfig::default()
    })
    .unwrap();
    vm.set_bytes_module_sources(HashMap::from([("t/big.bin".to_string(), vec![0; 300_000])]));
    assert_eq!(
        vm.execute_module_graph(
            "t/main.js",
            &build(&[(
                "main.js",
                "import b from './big.bin' with { type: 'bytes' }; b.length"
            )])
        ),
        Err(RuntimeError::RangeError(
            "immutable ArrayBuffer length is too large".into()
        ))
    );
}
