// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

use super::*;

fn runtime() -> (bluejs::BlueJsPageRuntime, bluejs::BlueJsPageOrigin) {
    let origin = bluejs::BlueJsPageOrigin::new("https://protocol.test").unwrap();
    let mut runtime = bluejs::BlueJsPageRuntime::default();
    runtime.open_realm(7, origin.clone()).unwrap();
    runtime
        .configure_realm_bindings(7, |host| {
            host.install_web_file_api()?;
            host.install_window_aliases()?;
            host.install_host_console()?;
            host.install_host_error_reporting()?;
            host.install_host_rejection_tracking()
        })
        .unwrap();
    (runtime, origin)
}

#[test]
fn direct_implicit_protocols_keep_the_original_operand_source() {
    let mut failures = Vec::new();
    for define in [true, false] {
        for (name,source) in [
            ("heritage", "const Base: any=0;class C extends\n Base {}"),
            ("prototype", "function Parent(): void {}const Base: any=Parent;Base.prototype=0;class C extends\n Base {}"),
            ("array spread", "const input: any=null;const result: any=[...\n input];"),
            ("call spread", "function consume(...values: any[]): void {}function inputValue(): any {return null;}const input: [any]=inputValue();consume(...\n input);"),
            ("construct spread", "class C {constructor(...values: any[]){}}function inputValue(): any {return null;}const input: [any]=inputValue();new C(...\n input);"),
            ("super spread", "class Base {constructor(...values: any[]){}}function inputValue(): any {return null;}const input: [any]=inputValue();class C extends Base {constructor(){super(...\n input);}}new C();"),
            ("symbol template", "const input: any=Symbol();const result: any=`prefix:${\n input}`;"),
            ("native template", "class Input {toString(): any {return {};}valueOf(): any {return {};}}const input: any=new Input();const result: any=`prefix:${\n input}`;"),
            ("revoked spread", "const Factory: any=(globalThis as any).Proxy;const record: any=Factory.revocable({value:1},{});const input: {value: any}=record.proxy;record.revoke();const result: any={...\n input};"),
            ("ownKeys spread", "const Factory: any=(globalThis as any).Proxy;class Handler {ownKeys(): any {try{throw 'caught';}catch(error){}return 0;}}const input: {value: any}=new Factory({value:1},new Handler());const result: any={...\n input};"),
        ] {
            let artifact=compile_direct_script(ENTRY,&MapLoader::from([ModuleSource::new(ENTRY,source)]),CompilerOptions {use_define_for_class_fields:Some(define),..CompilerOptions::default()}).unwrap_or_else(|error|panic!("{name}: {error:?}"));
            let (mut runtime,origin)=runtime();
            let attachment=artifact.attach_in_page_realm(&mut runtime,7,&origin).unwrap();
            assert!(runtime.execute_program(7,attachment.handle).is_err(),"{name}");
            let batch=runtime.drain_console(7).unwrap();
            assert_eq!(batch.records.len(),1,"{name}: {batch:?}");
            let actual=batch.records[0].source.as_ref().map(|location|(location.filename.as_str(),location.line,location.column));
            if actual!=Some((ENTRY,2,2)) {failures.push((name,define,format!("{actual:?}")));}
        }
    }
    assert!(failures.is_empty(), "{failures:?}");
}

#[test]
fn direct_dependency_protocol_sources_survive_await_and_field_modes() {
    for define in [true, false] {
        for awaited in [false, true] {
            for body in [
                "const Base: any=0;export class C extends\n Base {}",
                "const input: any=null;export const result: any=[...\n input];",
                "const input: any=Symbol();export const result: any=`prefix:${\n input}`;",
                "const Factory: any=(globalThis as any).Proxy;const record: any=Factory.revocable({value:1},{});const input: {value: any}=record.proxy;record.revoke();export const result: any={...\n input};",
            ] {
                let prefix=if awaited {"await 0;"} else {""};
                let dependency=format!("{prefix}{body}");
                let graph=compile_direct_module_graph(GRAPH_ENTRY,&MapLoader::from([ModuleSource::new(GRAPH_ENTRY,"import './dep.ts';"),ModuleSource::new("graph/dep.ts",&dependency)]),CompilerOptions {use_define_for_class_fields:Some(define),..CompilerOptions::default()}).unwrap();
                let (mut runtime,origin)=runtime();
                let attachment=graph.attach_in_page_realm(&mut runtime,7,&origin).unwrap();
                assert!(attachment.execute_in_page_realm(&mut runtime,7).is_err(),"{dependency}");
                runtime.run_microtask_checkpoint(7).unwrap();
                let batch=runtime.drain_console(7).unwrap();
                assert_eq!(batch.records.len(),1,"{dependency}: {batch:?}");
                let location=batch.records[0].source.as_ref().unwrap_or_else(||panic!("{dependency}: {batch:?}"));
                assert_eq!((location.filename.as_str(),location.line,location.column),("graph/dep.ts",2,2),"{dependency}");
            }
        }
    }
}

#[test]
fn direct_iteration_loops_remain_outside_the_admitted_runtime_surface() {
    for (module_only, source) in [
        (false, "const input: any=null;for(const value of input){}"),
        (false, "const input: any=null;for(const key in input){}"),
        (
            true,
            "const input: any=null;for await(const value of input){}",
        ),
    ] {
        let loader = MapLoader::from([ModuleSource::new(ENTRY, source)]);
        if !module_only {
            assert!(
                matches!(
                    compile_direct_script(ENTRY, &loader, CompilerOptions::default()),
                    Err(BridgeError::UnsupportedRuntimeTarget { .. })
                ),
                "{source}"
            );
        }
        let module = format!("export const marker: number=0;{source}");
        let loader = MapLoader::from([ModuleSource::new(GRAPH_ENTRY, &module)]);
        assert!(
            matches!(
                compile_direct_module_graph(GRAPH_ENTRY, &loader, CompilerOptions::default()),
                Err(BridgeError::UnsupportedRuntimeTarget { .. })
            ),
            "{module}"
        );
    }
}

#[test]
fn direct_with_and_using_forms_remain_outside_the_admitted_runtime_surface() {
    for source in [
        "with(null){}",
        "{using value=1;}",
        "async function fail(){await using value=1;}fail();",
        "for(using value of [1]){}",
    ] {
        let loader = MapLoader::from([ModuleSource::new(ENTRY, source)]);
        assert!(
            compile_direct_script(ENTRY, &loader, CompilerOptions::default()).is_err(),
            "{source}"
        );
    }
}
