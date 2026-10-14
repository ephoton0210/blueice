// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

//! HTML cleanup tasks, driven through the public page-runtime host entries.

use blueice_bluejs::{
    parse, BlueJsPageOrigin, BlueJsPageRuntime, BlueJsPageRuntimeConfig, BlueJsPageRuntimeError,
    BlueJsProgramV1, BlueJsSourceIdentity, HostValue, RuntimeError, Value,
    MAX_PENDING_BROWSER_TASKS,
};
use std::{cell::RefCell, rc::Rc};

struct Page {
    runtime: BlueJsPageRuntime,
    log: Rc<RefCell<Vec<String>>>,
    serial: u32,
}

impl Page {
    fn new() -> Self {
        Self::with_config(BlueJsPageRuntimeConfig::default())
    }

    fn with_config(config: BlueJsPageRuntimeConfig) -> Self {
        let mut runtime = BlueJsPageRuntime::new(config).unwrap();
        runtime
            .open_realm(1, BlueJsPageOrigin::new("https://cleanup.test").unwrap())
            .unwrap();
        let log = Rc::new(RefCell::new(Vec::new()));
        let output = log.clone();
        runtime
            .configure_realm_bindings(1, |host| {
                host.install_host_timers()?;
                host.install_global_function("record", 1, move |args: &[HostValue]| {
                    if let Some(HostValue::String(value)) = args.first() {
                        output.borrow_mut().push(value.to_utf8().unwrap());
                    }
                    Ok(HostValue::Undefined)
                })
            })
            .unwrap();
        Self {
            runtime,
            log,
            serial: 0,
        }
    }

    fn execute(&mut self, source: &str) {
        self.try_execute(source).unwrap();
    }

    fn try_execute(&mut self, source: &str) -> Result<Value, BlueJsPageRuntimeError> {
        self.serial += 1;
        let program = self
            .runtime
            .install_program(
                1,
                &BlueJsPageOrigin::new("https://cleanup.test").unwrap(),
                BlueJsSourceIdentity::new(
                    format!("page:///cleanup-{}.js", self.serial),
                    format!("sha256:cleanup-{}", self.serial),
                )
                .unwrap(),
                &BlueJsProgramV1::Script(parse(source).unwrap()),
            )
            .unwrap();
        self.runtime.execute_program(1, program)
    }

    fn checkpoint(&mut self) {
        self.runtime.run_microtask_checkpoint(1).unwrap();
    }

    fn expect(&self, expected: &[&str]) {
        assert_eq!(*self.log.borrow(), expected);
    }
}

#[test]
fn page_cleanup_waits_for_a_browser_task_after_the_promise_checkpoint() {
    let mut page = Page::new();
    page.execute("var registry=new FinalizationRegistry(value=>{record(value);Promise.resolve().then(()=>record('cleanup-micro'));});registry.register({},'cleanup');queueMicrotask(()=>record('micro'));record('script');");
    page.checkpoint();
    page.expect(&["script", "micro"]);
    page.runtime.flush_drag_tasks(1).unwrap();
    page.expect(&["script", "micro", "cleanup", "cleanup-micro"]);
    page.runtime.flush_drag_tasks(1).unwrap();
    page.expect(&["script", "micro", "cleanup", "cleanup-micro"]);
}

#[test]
fn unregister_before_the_cleanup_task_suppresses_the_cleared_cell() {
    let mut page = Page::new();
    page.execute("var token={},registry=new FinalizationRegistry(record);registry.register({},'cancelled',token);");
    page.checkpoint();
    page.execute("if(!registry.unregister(token))throw 'missing pending cell';");
    page.checkpoint();
    page.runtime.flush_drag_tasks(1).unwrap();
    page.expect(&[]);
}

#[test]
fn a_throwing_cleanup_callback_checkpoints_and_preserves_other_cells() {
    let mut page = Page::new();
    page.execute("var calls=0,registry=new FinalizationRegistry(()=>{calls++;record('cleanup');queueMicrotask(()=>record('micro'));if(calls===1)throw 7;});registry.register({},1);registry.register({},2);");
    page.checkpoint();
    page.expect(&[]);
    page.runtime.flush_drag_tasks(1).unwrap();
    page.expect(&["cleanup", "micro", "cleanup", "micro"]);
}

#[test]
fn document_browser_turns_drive_shadow_realm_cleanup_tasks() {
    let mut page = Page::new();
    page.execute("var shadow=new ShadowRealm();var read=shadow.evaluate('globalThis.cleaned=0;globalThis.registry=new FinalizationRegistry(()=>cleaned++);registry.register({},0);()=>cleaned;');record(String(read()));");
    page.checkpoint();
    page.expect(&["0"]);
    page.runtime.flush_drag_tasks(1).unwrap();
    page.execute("record(String(read()));");
    page.expect(&["0", "1"]);
}

#[test]
fn cleanup_discovered_during_a_timer_checkpoint_needs_no_new_script_entry() {
    let mut page = Page::new();
    page.execute("var registry=new FinalizationRegistry(record);function noop(){}setTimeout(()=>{registry.register({},'cleanup');for(var i=0;i<3;i++)Promise.resolve('x'.repeat(400000)).then(noop);record('timer');},10);");
    page.checkpoint();
    assert_eq!(page.runtime.run_due_timers(1, 10).unwrap().tasks_run, 1);
    page.expect(&["timer"]);
    assert_eq!(
        page.runtime
            .realm_stats(1)
            .unwrap()
            .scheduling
            .pending_browser_tasks,
        1
    );
    page.runtime.flush_drag_tasks(1).unwrap();
    page.expect(&["timer", "cleanup"]);
}

#[test]
fn cleanup_fifo_is_shared_between_the_document_and_its_shadow_realm() {
    let mut page = Page::new();
    page.execute("var read=new ShadowRealm().evaluate('globalThis.cleaned=0;globalThis.registry=new FinalizationRegistry(()=>cleaned++);registry.register({},0);()=>cleaned;');var registry=new FinalizationRegistry(()=>record(String(read())));registry.register({},0);");
    page.checkpoint();
    page.expect(&[]);
    page.runtime.flush_drag_tasks(1).unwrap();
    page.expect(&["1"]);
}

#[test]
fn cleanup_callbacks_can_create_shadow_realms_without_reordering_their_tasks() {
    let mut page = Page::new();
    page.execute("var read,second,first=new FinalizationRegistry(()=>{read=new ShadowRealm().evaluate('globalThis.cleaned=0;globalThis.registry=new FinalizationRegistry(()=>cleaned++);registry.register({},0);()=>cleaned;');second=new FinalizationRegistry(()=>record(String(read())));second.register({},0);for(var i=0;i<3;i++)Promise.resolve('x'.repeat(400000)).then(()=>{});});first.register({},0);");
    page.checkpoint();
    page.runtime.flush_drag_tasks(1).unwrap();
    page.expect(&["1"]);
}

#[test]
fn a_registry_keeps_one_pending_task_and_yields_between_bounded_cleanup_turns() {
    let mut page = Page::new();
    page.execute("var registry=new FinalizationRegistry(()=>{record('cleanup');queueMicrotask(()=>record('micro'));});for(var i=0;i<100;i++)registry.register({},i);");
    page.checkpoint();
    assert_eq!(
        page.runtime
            .realm_stats(1)
            .unwrap()
            .scheduling
            .pending_browser_tasks,
        1
    );
    page.expect(&[]);
    page.runtime.flush_drag_tasks(1).unwrap();
    let first_turn: Vec<_> = (0..64).flat_map(|_| ["cleanup", "micro"]).collect();
    page.expect(&first_turn);
    assert_eq!(
        page.runtime
            .realm_stats(1)
            .unwrap()
            .scheduling
            .pending_browser_tasks,
        1
    );
    page.runtime.flush_drag_tasks(1).unwrap();
    let expected: Vec<_> = (0..100).flat_map(|_| ["cleanup", "micro"]).collect();
    page.expect(&expected);
    assert_eq!(
        page.runtime
            .realm_stats(1)
            .unwrap()
            .scheduling
            .pending_browser_tasks,
        0
    );
}

#[test]
fn the_browser_task_cap_is_shared_with_shadow_realm_cleanup() {
    let mut page = Page::new();
    page.execute("var registries=[];for(var i=0;i<32;i++){var r=new FinalizationRegistry(()=>record('parent'));registries.push(r);r.register({},0);}var read=new ShadowRealm().evaluate('globalThis.cleaned=0;globalThis.registries=[];for(var i=0;i<32;i++){var r=new FinalizationRegistry(()=>cleaned++);registries.push(r);r.register({},0);}()=>cleaned;');");
    page.checkpoint();
    assert_eq!(
        page.runtime
            .realm_stats(1)
            .unwrap()
            .scheduling
            .pending_browser_tasks,
        MAX_PENDING_BROWSER_TASKS
    );
    page.expect(&[]);
    page.runtime.flush_drag_tasks(1).unwrap();
    assert_eq!(page.log.borrow().len(), 32);
    assert_eq!(
        page.runtime
            .realm_stats(1)
            .unwrap()
            .scheduling
            .pending_browser_tasks,
        0
    );
    page.execute("record(String(read()));");
    assert_eq!(page.log.borrow().last().unwrap(), "32");
}

#[test]
fn excess_cleanup_tasks_fail_the_owning_document_before_any_callback() {
    let mut page = Page::new();
    assert_eq!(page.try_execute("var registries=[];for(var i=0;i<65;i++){var r=new FinalizationRegistry(()=>record('must not run'));registries.push(r);r.register({},0);}"), Err(BlueJsPageRuntimeError::Runtime(RuntimeError::SchedulingLimit)));
    page.expect(&[]);
    assert_eq!(
        page.runtime
            .realm_stats(1)
            .unwrap()
            .scheduling
            .pending_browser_tasks,
        MAX_PENDING_BROWSER_TASKS
    );
    assert_eq!(
        page.runtime.flush_drag_tasks(1),
        Err(BlueJsPageRuntimeError::Runtime(
            RuntimeError::SchedulingLimit
        ))
    );
}

#[test]
fn pending_cleanup_and_file_reader_tasks_share_the_document_cap() {
    let mut page = Page::new();
    page.runtime
        .configure_realm_bindings(1, |host| host.install_web_file_api())
        .unwrap();
    page.execute("var registries=[];for(var i=0;i<64;i++){var r=new FinalizationRegistry(()=>record('must not run'));registries.push(r);r.register({},0);}");
    assert_eq!(
        page.try_execute("var reader=new FileReader();reader.readAsText(new Blob(['payload']));"),
        Err(BlueJsPageRuntimeError::Runtime(
            RuntimeError::SchedulingLimit
        ))
    );
    assert_eq!(
        page.runtime
            .realm_stats(1)
            .unwrap()
            .scheduling
            .pending_browser_tasks,
        MAX_PENDING_BROWSER_TASKS
    );
    page.expect(&[]);
}

#[test]
fn a_resource_failure_in_cleanup_does_not_run_its_queued_microtasks() {
    let mut config = BlueJsPageRuntimeConfig::default();
    config.vm.instruction_budget = 1000;
    let mut page = Page::with_config(config);
    page.execute("var registry=new FinalizationRegistry(()=>{queueMicrotask(()=>record('must not run'));while(true){}});registry.register({},0);");
    page.checkpoint();
    assert_eq!(
        page.runtime.flush_drag_tasks(1),
        Err(BlueJsPageRuntimeError::Runtime(
            RuntimeError::InstructionLimit
        ))
    );
    page.expect(&[]);
}

#[test]
fn navigation_discards_parent_and_shadow_cleanup_tasks() {
    let mut page = Page::new();
    page.execute("var registry=new FinalizationRegistry(record);registry.register({},'old');var read=new ShadowRealm().evaluate('globalThis.registry=new FinalizationRegistry(()=>{});registry.register({},0);()=>0;');");
    assert_eq!(
        page.runtime
            .realm_stats(1)
            .unwrap()
            .scheduling
            .pending_browser_tasks,
        2
    );
    page.runtime
        .navigate(1, BlueJsPageOrigin::new("https://successor.test").unwrap())
        .unwrap();
    page.runtime.flush_drag_tasks(1).unwrap();
    page.expect(&[]);
    assert_eq!(
        page.runtime
            .realm_stats(1)
            .unwrap()
            .scheduling
            .pending_browser_tasks,
        0
    );
    assert!(page.runtime.close_realm(1));
}

#[test]
fn a_debugger_pause_preserves_cleanup_without_running_the_document_tree() {
    let mut page = Page::new();
    page.execute("var registry=new FinalizationRegistry(record);registry.register({},'cleanup');");
    let program = page
        .runtime
        .install_program(
            1,
            &BlueJsPageOrigin::new("https://cleanup.test").unwrap(),
            BlueJsSourceIdentity::new("page:///pause.js", "sha256:pause").unwrap(),
            &BlueJsProgramV1::Script(parse("debugger;").unwrap()),
        )
        .unwrap();
    page.runtime
        .execute_program_until_debugger_pause_at_root_offset(1, program, 0)
        .unwrap();
    page.runtime.flush_drag_tasks(1).unwrap();
    page.expect(&[]);
    assert_eq!(
        page.runtime
            .realm_stats(1)
            .unwrap()
            .scheduling
            .pending_browser_tasks,
        1
    );
    page.runtime.resume_debugger_execution(1).unwrap();
    page.runtime.flush_drag_tasks(1).unwrap();
    page.expect(&["cleanup"]);
}

#[test]
fn a_cleanup_microtask_can_unregister_all_remaining_cells() {
    let mut page = Page::new();
    page.execute("var token={},registry=new FinalizationRegistry(()=>{record('cleanup');queueMicrotask(()=>{if(!registry.unregister(token))throw 'missing cells';record('unregistered');});});for(var i=0;i<10;i++)registry.register({},i,token);");
    page.checkpoint();
    page.runtime.flush_drag_tasks(1).unwrap();
    page.expect(&["cleanup", "unregistered"]);
    assert_eq!(
        page.runtime
            .realm_stats(1)
            .unwrap()
            .scheduling
            .pending_browser_tasks,
        0
    );
}

#[test]
fn cleanup_holdings_survive_callback_gc_and_are_released_after_delivery() {
    let mut page = Page::new();
    page.execute("var weak,registry=new FinalizationRegistry(holding=>{for(var i=0;i<3;i++)Promise.resolve('x'.repeat(400000)).then(()=>{});queueMicrotask(()=>record(holding.value));});(()=>{var holding={value:'held'};weak=new WeakRef(holding);registry.register({},holding);})();");
    page.checkpoint();
    page.runtime.flush_drag_tasks(1).unwrap();
    page.expect(&["held"]);
    page.execute("record(String(weak.deref()===undefined));");
    page.expect(&["held", "true"]);
}
