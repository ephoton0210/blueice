// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

//! Native event callback cleanup through the public page-runtime boundary.

use blueice_bluejs::{
    parse, parse_module, BlueJsPageOrigin, BlueJsPageRuntime, BlueJsPageRuntimeConfig,
    BlueJsPageRuntimeError, BlueJsProgramV1, BlueJsSourceIdentity, HostDragEventData,
    HostDragEventKind, HostFileSelectionEvent, HostObjectFamily, HostObjectKey, HostValue,
    RuntimeError,
};
use std::{cell::RefCell, rc::Rc};

struct Page {
    runtime: BlueJsPageRuntime,
    family: HostObjectFamily,
    log: Rc<RefCell<Vec<String>>>,
    serial: u32,
}

impl Page {
    fn new(node_events: bool) -> Self {
        Self::with_config(node_events, BlueJsPageRuntimeConfig::default())
    }

    fn with_config(node_events: bool, config: BlueJsPageRuntimeConfig) -> Self {
        let mut runtime = BlueJsPageRuntime::new(config).unwrap();
        runtime.open_realm(7, origin()).unwrap();
        let log = Rc::new(RefCell::new(Vec::new()));
        let output = log.clone();
        let mut family = None;
        runtime
            .configure_realm_bindings(7, |host| {
                host.install_web_file_api()?;
                host.install_host_timers()?;
                let nodes = host.create_host_object_family()?;
                host.install_host_file_selection_event_methods(nodes)?;
                if node_events {
                    host.install_host_node_event_methods(nodes, |key| {
                        Ok((1..=key.object()).rev().map(key_for).collect())
                    })?;
                }
                for (name, id) in [("target", 2), ("parent", 1)] {
                    host.install_global_object_factory(
                        name,
                        0,
                        nodes,
                        move |_: &[HostValue]| Ok(Some(key_for(id))),
                    )?;
                }
                host.install_global_function("record", 1, move |args: &[HostValue]| {
                    if let Some(HostValue::String(value)) = args.first() {
                        output.borrow_mut().push(value.to_utf8().unwrap());
                    }
                    Ok(HostValue::Undefined)
                })?;
                family = Some(nodes);
                Ok(())
            })
            .unwrap();
        Self {
            runtime,
            family: family.unwrap(),
            log,
            serial: 0,
        }
    }

    fn execute(&mut self, source: &str) {
        self.serial += 1;
        let program = self
            .runtime
            .install_program(
                7,
                &origin(),
                BlueJsSourceIdentity::new(
                    format!("page:///event-{}.js", self.serial),
                    format!("sha256:event-{}", self.serial),
                )
                .unwrap(),
                &BlueJsProgramV1::Script(parse(source).unwrap()),
            )
            .unwrap();
        self.runtime.execute_program(7, program).unwrap();
    }

    fn dispatch(&mut self, name: &str) -> Result<(), BlueJsPageRuntimeError> {
        match name {
            "click" => self
                .runtime
                .dispatch_host_click(7, self.family, key_for(2))
                .map(|_| ()),
            "input" => self.runtime.dispatch_host_file_selection_event(
                7,
                self.family,
                &[key_for(2), key_for(1)],
                HostFileSelectionEvent::Input,
            ),
            "dragover" => self
                .runtime
                .dispatch_host_drag_event(
                    7,
                    self.family,
                    &[key_for(2), key_for(1)],
                    &HostDragEventData::new(HostDragEventKind::DragOver, 0.0, 0.0),
                )
                .map(|_| ()),
            _ => unreachable!(),
        }
    }

    fn expect(&self, expected: &[&str]) {
        assert_eq!(*self.log.borrow(), expected);
    }
}

fn origin() -> BlueJsPageOrigin {
    BlueJsPageOrigin::new("https://events.test").unwrap()
}

fn key_for(id: u64) -> HostObjectKey {
    HostObjectKey::new(7, 1, id)
}

fn callbacks_checkpoint_individually(name: &str) {
    for nodes in [false, true] {
        let mut page = Page::new(nodes);
        page.execute(&format!("target().addEventListener('{name}',()=>{{record('first');queueMicrotask(()=>record('first-micro'));}});target().addEventListener('{name}',()=>{{record('second');Promise.resolve().then(()=>record('second-micro'));}});"));
        page.dispatch(name).unwrap();
        page.expect(&["first", "first-micro", "second", "second-micro"]);
    }
}

#[test]
fn native_click_listeners_checkpoint_before_the_next_listener() {
    callbacks_checkpoint_individually("click");
}

#[test]
fn native_selection_listeners_checkpoint_before_the_next_listener() {
    callbacks_checkpoint_individually("input");
}

#[test]
fn native_drag_listeners_checkpoint_before_the_next_listener() {
    callbacks_checkpoint_individually("dragover");
}

#[test]
fn script_dispatch_keeps_microtasks_behind_the_calling_script() {
    let mut page = Page::new(true);
    page.execute("target().addEventListener('x',()=>{record('first');queueMicrotask(()=>record('micro'));});target().addEventListener('x',()=>record('second'));target().dispatchEvent(new Event('x'));record('script');");
    page.expect(&["first", "second", "script"]);
    page.runtime.run_microtask_checkpoint(7).unwrap();
    page.expect(&["first", "second", "script", "micro"]);
}

#[test]
fn a_native_listener_microtask_can_remove_later_listeners_and_stop_bubbling() {
    let mut page = Page::new(true);
    page.execute("function later(){record('removed');}target().addEventListener('click',e=>{record('first');queueMicrotask(()=>{record('micro');target().removeEventListener('click',later);e.stopPropagation();});});target().addEventListener('click',later);target().addEventListener('click',()=>record('last'));parent().addEventListener('click',()=>record('parent'));");
    page.dispatch("click").unwrap();
    page.expect(&["first", "micro", "last"]);
}

#[test]
fn throwing_native_callbacks_and_jobs_do_not_skip_their_cleanup_or_later_listeners() {
    let mut page = Page::new(true);
    page.execute("target().addEventListener('click',()=>{record('first');queueMicrotask(()=>{record('throw-micro');throw 8;});queueMicrotask(()=>record('micro'));throw {reason:7};});target().addEventListener('click',()=>record('second'));");
    page.dispatch("click").unwrap();
    page.expect(&["first", "throw-micro", "micro", "second"]);
}

#[test]
fn passive_callback_state_remains_active_during_its_microtasks() {
    let mut page = Page::new(true);
    page.execute("target().addEventListener('click',e=>queueMicrotask(()=>{e.preventDefault();record(String(e.eventPhase===2&&e.currentTarget===target()&&!e.defaultPrevented));}),{passive:true,once:true});target().addEventListener('click',e=>record(String(!e.defaultPrevented)));");
    assert!(!page
        .runtime
        .dispatch_host_click(7, page.family, key_for(2))
        .unwrap());
    page.expect(&["true", "true"]);
}

#[test]
fn one_native_dispatch_shares_its_256_job_budget_across_listeners() {
    let mut page = Page::new(true);
    page.execute("var count=0;function job(){count++;}function first(){for(var i=0;i<200;i++)queueMicrotask(job);}target().addEventListener('click',first);target().addEventListener('click',()=>{first();});target().addEventListener('click',()=>record('must not run'));");
    assert_eq!(
        page.dispatch("click"),
        Err(BlueJsPageRuntimeError::Runtime(
            RuntimeError::InstructionLimit
        ))
    );
    page.execute("record(String(count));");
    page.expect(&["256"]);
    page.runtime.run_click_microtask_checkpoint(7).unwrap();
    page.execute("record(String(count));");
    page.expect(&["256", "400"]);
}

#[test]
fn callback_checkpoints_do_not_refill_the_dispatch_instruction_budget() {
    let mut config = BlueJsPageRuntimeConfig::default();
    config.vm.instruction_budget = 1000;
    let mut page = Page::with_config(true, config);
    page.execute("var count=0;for(var i=0;i<8;i++)target().addEventListener('click',()=>{for(var j=0;j<30;j++)count++;queueMicrotask(()=>{});});target().addEventListener('click',()=>record('must not run'));");
    assert_eq!(
        page.dispatch("click"),
        Err(BlueJsPageRuntimeError::Runtime(
            RuntimeError::InstructionLimit
        ))
    );
    page.expect(&[]);
}

#[test]
fn nested_script_dispatch_does_not_checkpoint_inside_a_native_listener() {
    let mut page = Page::new(true);
    page.execute("var other=new EventTarget();other.addEventListener('x',()=>{record('nested');queueMicrotask(()=>record('nested-micro'));});target().addEventListener('click',()=>{record('start');other.dispatchEvent(new Event('x'));record('end');queueMicrotask(()=>record('outer-micro'));});target().addEventListener('click',()=>record('second'));");
    page.dispatch("click").unwrap();
    page.expect(&[
        "start",
        "nested",
        "end",
        "nested-micro",
        "outer-micro",
        "second",
    ]);
}

#[test]
fn file_reader_callback_cleanup_can_abort_before_the_next_native_listener() {
    let mut page = Page::new(true);
    page.execute("var reader=new FileReader();reader.addEventListener('loadstart',()=>{record('first');queueMicrotask(()=>{record('micro');reader.abort();});});reader.addEventListener('loadstart',()=>record('second:'+reader.readyState));reader.onabort=()=>record('abort');reader.onload=()=>record('must not load');reader.readAsText(new Blob(['payload']));");
    page.runtime.flush_drag_tasks(7).unwrap();
    page.expect(&["first", "micro", "abort", "second:2"]);
}

#[test]
fn native_callback_microtasks_can_prevent_the_owner_default_action() {
    for nodes in [false, true] {
        let mut page = Page::new(nodes);
        page.execute(
            "target().addEventListener('click',e=>queueMicrotask(()=>e.preventDefault()));",
        );
        assert!(page
            .runtime
            .dispatch_host_click(7, page.family, key_for(2))
            .unwrap());
    }
}

#[test]
fn a_handle_event_getter_and_callback_share_the_same_callback_cleanup() {
    let mut page = Page::new(true);
    page.execute("var listener={get handleEvent(){record('get');queueMicrotask(()=>record('get-micro'));return function(){record('call');queueMicrotask(()=>record('call-micro'));};}};target().addEventListener('click',listener,{once:true});target().addEventListener('click',()=>record('second'));");
    page.dispatch("click").unwrap();
    page.expect(&["get", "call", "get-micro", "call-micro", "second"]);
}

#[test]
fn exhausting_callback_instructions_does_not_execute_queued_jobs_or_later_listeners() {
    let mut config = BlueJsPageRuntimeConfig::default();
    config.vm.instruction_budget = 1000;
    let mut page = Page::with_config(true, config);
    page.execute("target().addEventListener('click',()=>{queueMicrotask(()=>record('must not run'));while(true){}});target().addEventListener('click',()=>record('must not run'));");
    assert_eq!(
        page.dispatch("click"),
        Err(BlueJsPageRuntimeError::Runtime(
            RuntimeError::InstructionLimit
        ))
    );
    page.expect(&[]);
}

#[test]
fn an_exactly_full_job_budget_still_allows_empty_callback_checkpoints() {
    let mut page = Page::new(true);
    page.execute("var count=0;target().addEventListener('click',()=>{for(var i=0;i<256;i++)queueMicrotask(()=>count++);});for(var i=0;i<8;i++)target().addEventListener('click',()=>{});target().addEventListener('click',()=>record(String(count)));");
    page.dispatch("click").unwrap();
    page.expect(&["256"]);
}

#[test]
fn invalid_native_dispatch_preserves_pending_jobs_for_a_later_valid_turn() {
    let mut page = Page::new(true);
    let other = Page::new(true);
    page.execute("queueMicrotask(()=>record('pending'));");
    assert!(matches!(
        page.runtime
            .dispatch_host_click(7, other.family, key_for(2)),
        Err(BlueJsPageRuntimeError::Runtime(RuntimeError::TypeError(_)))
    ));
    page.expect(&[]);
    page.dispatch("click").unwrap();
    page.expect(&["pending"]);
}

#[test]
fn dynamic_import_during_cleanup_preserves_the_dispatch_frame_and_event_roots() {
    let mut config = BlueJsPageRuntimeConfig::default();
    config.vm.heap.nursery_capacity = 1;
    config.vm.heap.major_threshold_bytes = 1;
    let mut page = Page::with_config(true, config);
    let mut modules = Vec::new();
    for (name, source) in [
        ("page:///entry.mjs", "export const ready=true;"),
        (
            "page:///lazy.mjs",
            "for(let i=0;i<200;i++){({value:i});}globalThis.importRuns+=1;export const marker=1;",
        ),
    ] {
        modules.push(
            page.runtime
                .install_program(
                    7,
                    &origin(),
                    BlueJsSourceIdentity::new(name, format!("sha256:{name}")).unwrap(),
                    &BlueJsProgramV1::Module(parse_module(source).unwrap()),
                )
                .unwrap(),
        );
    }
    page.runtime
        .execute_module_graph(7, modules[0], modules.clone())
        .unwrap();
    page.execute("globalThis.importRuns=0;target().addEventListener('click',()=>{record('first');import('page:///lazy.mjs').then(()=>record('imported'));},{once:true});target().addEventListener('click',e=>record('second:'+importRuns+':'+e.eventPhase+':'+(e.currentTarget===target())));");
    page.dispatch("click").unwrap();
    page.expect(&["first", "imported", "second:1:2:true"]);
}

#[test]
fn callback_cleanup_queues_finalization_for_a_later_browser_task() {
    let mut page = Page::new(true);
    page.execute("var retained,registry=new FinalizationRegistry(()=>record('cleanup:'+retained.eventPhase));function noop(){}target().addEventListener('click',e=>{retained=e;record('first');registry.register({},0);for(var i=0;i<3;i++)Promise.resolve('x'.repeat(400000)).then(noop);queueMicrotask(()=>record('micro'));});target().addEventListener('click',()=>record('second'));");
    page.dispatch("click").unwrap();
    page.expect(&["first", "micro", "second"]);
    page.runtime.flush_drag_tasks(7).unwrap();
    page.expect(&["first", "micro", "second", "cleanup:0"]);
}

#[test]
fn timeout_abort_listeners_checkpoint_before_the_next_native_listener() {
    let mut page = Page::new(true);
    page.execute("var signal=AbortSignal.timeout(0);signal.addEventListener('abort',()=>{record('first');queueMicrotask(()=>record('micro'));});signal.addEventListener('abort',()=>record('second'));");
    page.runtime.flush_drag_tasks(7).unwrap();
    page.expect(&["first", "micro", "second"]);
}

#[test]
fn synchronous_abort_keeps_cleanup_behind_its_calling_script() {
    let mut page = Page::new(true);
    page.execute("var controller=new AbortController();controller.signal.addEventListener('abort',()=>{record('first');queueMicrotask(()=>record('micro'));});controller.signal.addEventListener('abort',()=>record('second'));controller.abort();record('script');");
    page.expect(&["first", "second", "script"]);
    page.runtime.run_microtask_checkpoint(7).unwrap();
    page.expect(&["first", "second", "script", "micro"]);
}
