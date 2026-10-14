// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

use blueice_bluejs::{
    parse, parse_module, BlueJsPageOrigin, BlueJsPageRuntime, BlueJsPageRuntimeConfig,
    BlueJsPageRuntimeError, BlueJsProgramV1, BlueJsSourceIdentity, HostAsyncFunction,
    HostFunctionError, HostObjectKey, HostPromiseCompletion, HostPromiseToken, HostTaskSource,
    HostValue, JsString, RuntimeError,
};
use std::{cell::RefCell, rc::Rc};

struct Page {
    runtime: BlueJsPageRuntime,
    requests: Rc<RefCell<Vec<HostPromiseToken>>>,
    log: Rc<RefCell<Vec<String>>>,
    serial: u32,
}

impl Page {
    fn new() -> Self {
        Self::with_config(BlueJsPageRuntimeConfig::default())
    }

    fn with_config(config: BlueJsPageRuntimeConfig) -> Self {
        let mut runtime = BlueJsPageRuntime::new(config).unwrap();
        runtime.open_realm(1, origin()).unwrap();
        let requests = Rc::new(RefCell::new(Vec::new()));
        let pending = requests.clone();
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
                })?;
                host.install_async_global_function(
                    "request",
                    1,
                    HostTaskSource::Networking,
                    move |token: HostPromiseToken, _args: &[HostValue]| {
                        pending.borrow_mut().push(token);
                        Ok(())
                    },
                )
            })
            .unwrap();
        Self {
            runtime,
            requests,
            log,
            serial: 0,
        }
    }

    fn execute(&mut self, source: &str) {
        self.try_execute(source).unwrap();
    }

    fn try_execute(&mut self, source: &str) -> Result<(), BlueJsPageRuntimeError> {
        self.serial += 1;
        let program = self
            .runtime
            .install_program(
                1,
                &origin(),
                BlueJsSourceIdentity::new(
                    format!("page:///async-{}.js", self.serial),
                    format!("sha256:async-{}", self.serial),
                )
                .unwrap(),
                &BlueJsProgramV1::Script(parse(source).unwrap()),
            )
            .unwrap();
        self.runtime.execute_program(1, program)?;
        self.runtime.run_microtask_checkpoint(1)?;
        Ok(())
    }

    fn token(&self, index: usize) -> HostPromiseToken {
        self.requests.borrow()[index]
    }

    fn expect(&self, expected: &[&str]) {
        assert_eq!(*self.log.borrow(), expected);
    }
}

fn origin() -> BlueJsPageOrigin {
    BlueJsPageOrigin::new("https://async.test").unwrap()
}

#[test]
fn host_completion_settles_a_real_promise_only_in_its_document_task() {
    let mut page = Page::new();
    page.execute("var promise=request('resource');record(promise instanceof Promise?'promise':'wrong');promise.then(value=>{record(value);queueMicrotask(()=>record('micro'));});record('script');");
    page.expect(&["promise", "script"]);
    assert!(page
        .runtime
        .queue_host_promise_completion(
            1,
            page.token(0),
            HostPromiseCompletion::Resolve(HostValue::String("done".into()))
        )
        .unwrap());
    page.runtime.run_microtask_checkpoint(1).unwrap();
    page.expect(&["promise", "script"]);
    assert_eq!(page.runtime.next_task_wake_in_ms(1, 0).unwrap(), Some(0));
    assert_eq!(page.runtime.run_task_turn(1, 0, 1).unwrap().tasks_run, 1);
    page.expect(&["promise", "script", "done", "micro"]);
}

#[test]
fn rejected_completions_and_duplicates_have_one_observable_result() {
    let mut page = Page::new();
    page.execute("request('resource').catch(error=>record(error.name+':'+error.message));");
    let token = page.token(0);
    assert!(page
        .runtime
        .queue_host_promise_completion(
            1,
            token,
            HostPromiseCompletion::RejectError(HostFunctionError::new("refused"))
        )
        .unwrap());
    assert!(!page
        .runtime
        .queue_host_promise_completion(1, token, HostPromiseCompletion::Resolve(HostValue::Null))
        .unwrap());
    assert_eq!(page.runtime.run_task_turn(1, 0, 64).unwrap().tasks_run, 1);
    page.expect(&["TypeError:refused"]);
    assert!(!page
        .runtime
        .queue_host_promise_completion(1, token, HostPromiseCompletion::Resolve(HostValue::Null))
        .unwrap());
}

#[test]
fn a_navigation_discards_pending_and_already_queued_completions() {
    let mut page = Page::new();
    page.execute(
        "request('first').then(()=>record('old'));request('second').then(()=>record('old'));",
    );
    let first = page.token(0);
    let second = page.token(1);
    assert!(page
        .runtime
        .queue_host_promise_completion(1, first, HostPromiseCompletion::Resolve(HostValue::Null))
        .unwrap());
    page.runtime.navigate(1, origin()).unwrap();
    assert!(!page
        .runtime
        .queue_host_promise_completion(1, first, HostPromiseCompletion::Resolve(HostValue::Null))
        .unwrap());
    assert!(!page
        .runtime
        .queue_host_promise_completion(1, second, HostPromiseCompletion::Resolve(HostValue::Null))
        .unwrap());
    assert_eq!(page.runtime.run_task_turn(1, 0, 64).unwrap().tasks_run, 0);
    page.expect(&[]);
}

#[test]
fn invalid_arguments_reject_without_starting_the_external_operation() {
    let mut page = Page::new();
    page.execute("var promise=request({});record(promise instanceof Promise?'promise':'wrong');promise.catch(error=>record(error.name));");
    page.expect(&["promise", "TypeError"]);
    assert!(page.requests.borrow().is_empty());
    assert_eq!(
        page.runtime
            .host_scheduling_usage(1)
            .unwrap()
            .pending_reactions,
        0
    );
}

#[test]
fn a_synchronous_start_failure_returns_a_rejected_promise() {
    let mut page = Page::new();
    page.runtime
        .configure_realm_bindings(1, |host| {
            host.install_async_global_function(
                "refused",
                1,
                HostTaskSource::Networking,
                |_token: HostPromiseToken, _args: &[HostValue]| {
                    Err(HostFunctionError::new("service unavailable"))
                },
            )
        })
        .unwrap();
    page.execute("var promise=refused('resource');record(promise instanceof Promise?'promise':'wrong');promise.catch(error=>record(error.name+':'+error.message));");
    page.expect(&["promise", "TypeError:service unavailable"]);
    assert_eq!(
        page.runtime
            .host_scheduling_usage(1)
            .unwrap()
            .pending_reactions,
        0
    );
}

#[test]
fn completions_are_fifo_by_arrival_and_yield_to_other_sources() {
    let mut page = Page::new();
    let pending = page.requests.clone();
    page.runtime
        .configure_realm_bindings(1, |host| {
            host.install_async_global_function(
                "userRequest",
                0,
                HostTaskSource::UserInteraction,
                move |token: HostPromiseToken, _args: &[HostValue]| {
                    pending.borrow_mut().push(token);
                    Ok(())
                },
            )
        })
        .unwrap();
    page.execute("request('first').then(record);request('second').then(record);userRequest().then(record);setTimeout(()=>record('timer'),0);");
    for (index, value) in [(1, "second"), (0, "first"), (2, "user")] {
        assert!(page
            .runtime
            .queue_host_promise_completion(
                1,
                page.token(index),
                HostPromiseCompletion::Resolve(HostValue::String(value.into()))
            )
            .unwrap());
    }
    assert_eq!(page.runtime.run_task_turn(1, 0, 4).unwrap().tasks_run, 4);
    page.expect(&["timer", "second", "user", "first"]);
}

#[test]
fn completion_capabilities_are_scoped_to_one_runtime_and_tab() {
    let mut page = Page::new();
    let mut other = Page::new();
    page.execute("request('first').then(record);");
    other.execute("request('other').then(record);");
    assert!(!other
        .runtime
        .queue_host_promise_completion(
            1,
            page.token(0),
            HostPromiseCompletion::Resolve(HostValue::String("foreign".into()))
        )
        .unwrap());
    page.runtime.open_realm(2, origin()).unwrap();
    assert!(!page
        .runtime
        .queue_host_promise_completion(
            2,
            page.token(0),
            HostPromiseCompletion::Resolve(HostValue::Null)
        )
        .unwrap());
    assert_eq!(other.runtime.run_task_turn(1, 0, 64).unwrap().tasks_run, 0);
    assert!(page
        .runtime
        .queue_host_promise_completion(
            1,
            page.token(0),
            HostPromiseCompletion::Resolve(HostValue::String("owner".into()))
        )
        .unwrap());
    assert_eq!(page.runtime.run_task_turn(1, 0, 64).unwrap().tasks_run, 1);
    page.expect(&["owner"]);
    other.expect(&[]);
}

#[test]
fn a_completion_can_return_a_branded_wrapper_with_async_methods() {
    let mut page = Page::new();
    let pending = page.requests.clone();
    let mut family = None;
    page.runtime
        .configure_realm_bindings(1, |host| {
            let response = host.create_host_object_family()?;
            host.install_global_object_factory(
                "sameResponse",
                0,
                response,
                |_args: &[HostValue]| Ok(Some(HostObjectKey::new(1, 1, 7))),
            )?;
            host.install_host_object_method(
                response,
                "status",
                0,
                |key: HostObjectKey, _args: &[HostValue]| {
                    assert_eq!(key, HostObjectKey::new(1, 1, 7));
                    Ok(HostValue::Number(200.0))
                },
            )?;
            host.install_async_host_object_method(
                response,
                "body",
                0,
                HostTaskSource::Networking,
                move |token: HostPromiseToken, key: HostObjectKey, _args: &[HostValue]| {
                    assert_eq!(key, HostObjectKey::new(1, 1, 7));
                    pending.borrow_mut().push(token);
                    Ok(())
                },
            )?;
            family = Some(response);
            Ok(())
        })
        .unwrap();
    page.execute("request('resource').then(response=>{record(String(response===sameResponse()));record(String(response.status()));response.body().then(record);response.body.call({}).catch(error=>record(error.name));});");
    assert!(page
        .runtime
        .queue_host_promise_completion(
            1,
            page.token(0),
            HostPromiseCompletion::ResolveObject {
                family: family.unwrap(),
                key: HostObjectKey::new(1, 1, 7)
            }
        )
        .unwrap());
    assert_eq!(page.runtime.run_task_turn(1, 0, 1).unwrap().tasks_run, 1);
    page.expect(&["true", "200", "TypeError"]);
    assert_eq!(page.requests.borrow().len(), 2);
    assert!(page
        .runtime
        .queue_host_promise_completion(
            1,
            page.token(1),
            HostPromiseCompletion::Resolve(HostValue::String("body".into()))
        )
        .unwrap());
    assert_eq!(page.runtime.run_task_turn(1, 0, 1).unwrap().tasks_run, 1);
    page.expect(&["true", "200", "TypeError", "body"]);
}

struct Cancellable {
    pending: Rc<RefCell<Vec<HostPromiseToken>>>,
    cancelled: Rc<RefCell<Vec<HostPromiseToken>>>,
}

impl HostAsyncFunction for Cancellable {
    fn start(
        &mut self,
        token: HostPromiseToken,
        _args: &[HostValue],
    ) -> Result<(), HostFunctionError> {
        self.pending.borrow_mut().push(token);
        Ok(())
    }
    fn cancel(&mut self, token: HostPromiseToken) {
        self.cancelled.borrow_mut().push(token);
    }
}

#[test]
fn navigation_and_close_cancel_unfinished_service_waits() {
    let mut page = Page::new();
    let pending = page.requests.clone();
    let cancelled = Rc::new(RefCell::new(Vec::new()));
    let revoked = cancelled.clone();
    page.runtime
        .configure_realm_bindings(1, |host| {
            host.install_async_global_function(
                "cancellable",
                0,
                HostTaskSource::Networking,
                Cancellable {
                    pending,
                    cancelled: revoked,
                },
            )
        })
        .unwrap();
    page.execute("cancellable();cancellable();cancellable();");
    let completed = page.token(0);
    assert!(page
        .runtime
        .queue_host_promise_completion(
            1,
            completed,
            HostPromiseCompletion::Resolve(HostValue::Null)
        )
        .unwrap());
    page.runtime.run_task_turn(1, 0, 1).unwrap();
    page.runtime.navigate(1, origin()).unwrap();
    assert_eq!(cancelled.borrow().len(), 2);
    assert!(cancelled.borrow().contains(&page.token(1)));
    assert!(cancelled.borrow().contains(&page.token(2)));
    let pending = page.requests.clone();
    let revoked = cancelled.clone();
    page.runtime
        .configure_realm_bindings(1, |host| {
            host.install_async_global_function(
                "cancellable",
                0,
                HostTaskSource::Networking,
                Cancellable {
                    pending,
                    cancelled: revoked,
                },
            )
        })
        .unwrap();
    page.execute("cancellable();");
    assert!(page.runtime.close_realm(1));
    assert_eq!(cancelled.borrow().len(), 3);
    assert!(cancelled.borrow().contains(&page.token(3)));
}

#[test]
fn pending_external_waits_have_a_document_limit_before_service_start() {
    let mut page = Page::new();
    assert_eq!(
        page.try_execute(
            "for(var i=0;i<1025;i++){try{request();}catch(error){record('escaped');}}"
        ),
        Err(BlueJsPageRuntimeError::Runtime(
            RuntimeError::SchedulingLimit
        ))
    );
    assert_eq!(page.requests.borrow().len(), 1024);
    page.expect(&[]);
    page.runtime.navigate(1, origin()).unwrap();
    assert_eq!(
        page.runtime
            .host_scheduling_usage(1)
            .unwrap()
            .pending_reactions,
        0
    );
}

#[test]
fn queued_completions_share_the_browser_task_limit() {
    let mut page = Page::new();
    page.execute("for(var i=0;i<65;i++)request();");
    for index in 0..64 {
        assert!(page
            .runtime
            .queue_host_promise_completion(
                1,
                page.token(index),
                HostPromiseCompletion::Resolve(HostValue::Null)
            )
            .unwrap());
    }
    assert_eq!(
        page.runtime.queue_host_promise_completion(
            1,
            page.token(64),
            HostPromiseCompletion::Resolve(HostValue::Null)
        ),
        Err(BlueJsPageRuntimeError::Runtime(
            RuntimeError::SchedulingLimit
        ))
    );
}

#[test]
fn queued_completion_payloads_are_charged_before_another_is_admitted() {
    let mut page = Page::new();
    page.execute("for(var i=0;i<11;i++)request();");
    for index in 0..10 {
        assert!(page
            .runtime
            .queue_host_promise_completion(
                1,
                page.token(index),
                HostPromiseCompletion::Resolve(HostValue::String(JsString::from_code_units(
                    vec![u16::from(b'x'); 400000]
                )))
            )
            .unwrap());
    }
    assert!(page.runtime.host_scheduling_usage(1).unwrap().payload_bytes >= 8_000_000);
    assert_eq!(
        page.runtime.queue_host_promise_completion(
            1,
            page.token(10),
            HostPromiseCompletion::Resolve(HostValue::String(JsString::from_code_units(
                vec![u16::from(b'x'); 400000]
            )))
        ),
        Err(BlueJsPageRuntimeError::Runtime(
            RuntimeError::SchedulingLimit
        ))
    );
}

#[test]
fn live_host_waits_keep_async_frames_through_major_gc_and_release_them() {
    let mut config = BlueJsPageRuntimeConfig::default();
    config.vm.heap.nursery_capacity = 1;
    config.vm.heap.major_threshold_bytes = 1;
    config.vm.heap.max_heap_bytes = 1024 * 1024;
    let mut page = Page::with_config(config);
    page.execute("async function consume(){var saved={marker:37};var value=await request('resource');record(value+':'+saved.marker);}consume();for(var i=0;i<200;i++){var temporary=new Array(50).fill(i);}temporary=null;");
    assert!(
        page.runtime
            .host_scheduling_usage(1)
            .unwrap()
            .pending_reactions
            > 0
    );
    assert!(page
        .runtime
        .queue_host_promise_completion(
            1,
            page.token(0),
            HostPromiseCompletion::Resolve(HostValue::String("body".into()))
        )
        .unwrap());
    assert_eq!(page.runtime.run_task_turn(1, 0, 1).unwrap().tasks_run, 1);
    page.expect(&["body:37"]);
    assert_eq!(
        page.runtime
            .host_scheduling_usage(1)
            .unwrap()
            .pending_reactions,
        0
    );
}

#[test]
fn a_debugger_pause_retains_completions_without_running_jobs_or_busy_wakes() {
    let mut page = Page::new();
    page.execute("request('resource').then(record);");
    let program = page
        .runtime
        .install_program(
            1,
            &origin(),
            BlueJsSourceIdentity::new("page:///pause.js", "sha256:pause").unwrap(),
            &BlueJsProgramV1::Script(parse("debugger;").unwrap()),
        )
        .unwrap();
    page.runtime
        .execute_program_until_debugger_pause_at_root_offset(1, program, 0)
        .unwrap();
    assert!(page
        .runtime
        .queue_host_promise_completion(
            1,
            page.token(0),
            HostPromiseCompletion::Resolve(HostValue::String("done".into()))
        )
        .unwrap());
    let turn = page.runtime.run_task_turn(1, 0, 64).unwrap();
    assert_eq!(turn.tasks_run, 0);
    assert_eq!(turn.next_wake_in_ms, None);
    page.expect(&[]);
    page.runtime.resume_debugger_execution(1).unwrap();
    assert_eq!(page.runtime.run_task_turn(1, 0, 1).unwrap().tasks_run, 1);
    page.expect(&["done"]);
}

#[test]
fn invalid_completion_data_preserves_the_wait_for_a_valid_retry() {
    let mut page = Page::new();
    page.execute("request('resource').then(record);");
    let token = page.token(0);
    assert_eq!(
        page.runtime.queue_host_promise_completion(
            1,
            token,
            HostPromiseCompletion::Resolve(HostValue::String("x".repeat(600000).into()))
        ),
        Err(BlueJsPageRuntimeError::Runtime(RuntimeError::StringLimit {
            limit: 1024 * 1024
        }))
    );
    assert!(page
        .runtime
        .queue_host_promise_completion(
            1,
            token,
            HostPromiseCompletion::Resolve(HostValue::String("small".into()))
        )
        .unwrap());
    assert_eq!(page.runtime.run_task_turn(1, 0, 1).unwrap().tasks_run, 1);
    page.expect(&["small"]);
}

#[test]
fn an_external_service_thread_returns_data_and_a_token_without_a_vm() {
    let mut page = Page::new();
    let (started_tx, started_rx) = std::sync::mpsc::sync_channel(1);
    let worker = std::thread::spawn(move || {
        let (token, value) = started_rx.recv().unwrap();
        (token, HostPromiseCompletion::Resolve(value))
    });
    page.runtime
        .configure_realm_bindings(1, |host| {
            host.install_async_global_function(
                "service",
                1,
                HostTaskSource::Networking,
                move |token: HostPromiseToken, args: &[HostValue]| {
                    started_tx
                        .try_send((token, args[0].clone()))
                        .map_err(|_| HostFunctionError::new("service queue full"))
                },
            )
        })
        .unwrap();
    page.execute("service('worker 中文').then(record);record('script');");
    page.expect(&["script"]);
    let (token, completion) = worker.join().unwrap();
    assert!(page
        .runtime
        .queue_host_promise_completion(1, token, completion)
        .unwrap());
    assert_eq!(page.runtime.run_task_turn(1, 0, 1).unwrap().tasks_run, 1);
    page.expect(&["script", "worker 中文"]);
}

#[test]
fn async_callables_have_normal_metadata_and_cannot_be_constructed() {
    let mut page = Page::new();
    page.execute("record(request.name+':'+request.length+':'+Object.prototype.hasOwnProperty.call(request,'prototype'));try{new request();}catch(error){record(error.name);}var promise=request();record(String(Object.keys(promise).length));");
    page.expect(&["request:1:false", "TypeError", "0"]);
    assert_eq!(page.requests.borrow().len(), 1);
}

#[test]
fn native_ui_callbacks_finish_before_a_queued_network_completion() {
    let mut page = Page::new();
    let mut family = None;
    let key = HostObjectKey::new(1, 1, 9);
    page.runtime
        .configure_realm_bindings(1, |host| {
            let button = host.create_host_object_family()?;
            host.install_host_click_event_methods(button)?;
            host.install_global_family_object("button", button, key)?;
            family = Some(button);
            Ok(())
        })
        .unwrap();
    page.execute("request('resource').then(record);button.addEventListener('click',()=>{record('click');queueMicrotask(()=>record('click-micro'));});button.addEventListener('click',()=>record('next-listener'));");
    assert!(page
        .runtime
        .queue_host_promise_completion(
            1,
            page.token(0),
            HostPromiseCompletion::Resolve(HostValue::String("network".into()))
        )
        .unwrap());
    assert!(!page
        .runtime
        .dispatch_host_click(1, family.unwrap(), key)
        .unwrap());
    page.expect(&["click", "click-micro", "next-listener"]);
    assert_eq!(page.runtime.run_task_turn(1, 0, 1).unwrap().tasks_run, 1);
    page.expect(&["click", "click-micro", "next-listener", "network"]);
}

#[test]
fn failed_registration_preserves_the_existing_capability() {
    let mut page = Page::new();
    for name in ["request", "bad-name", "Promise"] {
        assert!(matches!(
            page.runtime
                .configure_realm_bindings(1, |host| host.install_async_global_function(
                    name,
                    0,
                    HostTaskSource::Networking,
                    |_token: HostPromiseToken, _args: &[HostValue]| Ok(())
                )),
            Err(BlueJsPageRuntimeError::HostBinding(
                RuntimeError::TypeError(_)
            ))
        ));
    }
    page.execute("request('original').then(record);");
    assert_eq!(page.requests.borrow().len(), 1);
    assert!(page
        .runtime
        .queue_host_promise_completion(
            1,
            page.token(0),
            HostPromiseCompletion::Resolve(HostValue::String("original".into()))
        )
        .unwrap());
    page.runtime.run_task_turn(1, 0, 1).unwrap();
    page.expect(&["original"]);
}

#[test]
fn a_foreign_wrapper_family_cannot_consume_a_completion_capability() {
    let mut page = Page::new();
    let mut other = Page::new();
    let mut foreign = None;
    other
        .runtime
        .configure_realm_bindings(1, |host| {
            foreign = Some(host.create_host_object_family()?);
            Ok(())
        })
        .unwrap();
    page.execute("request('resource').catch(record);");
    let token = page.token(0);
    assert!(matches!(
        page.runtime.queue_host_promise_completion(
            1,
            token,
            HostPromiseCompletion::ResolveObject {
                family: foreign.unwrap(),
                key: HostObjectKey::new(1, 1, 7)
            }
        ),
        Err(BlueJsPageRuntimeError::Runtime(RuntimeError::TypeError(_)))
    ));
    assert!(page
        .runtime
        .queue_host_promise_completion(
            1,
            token,
            HostPromiseCompletion::Reject(HostValue::String("cancelled".into()))
        )
        .unwrap());
    page.runtime.run_task_turn(1, 0, 1).unwrap();
    page.expect(&["cancelled"]);
}

#[test]
fn a_wrapper_then_getter_failure_rejects_instead_of_losing_the_wait() {
    let mut page = Page::new();
    let mut family = None;
    page.runtime
        .configure_realm_bindings(1, |host| {
            let response = host.create_host_object_family()?;
            host.install_global_object_factory(
                "response",
                0,
                response,
                |_args: &[HostValue]| Ok(Some(HostObjectKey::new(1, 1, 7))),
            )?;
            family = Some(response);
            Ok(())
        })
        .unwrap();
    page.execute("Object.defineProperty(Object.getPrototypeOf(response()),'then',{get(){throw new TypeError('bad then');}});request('resource').catch(error=>record(error.message));");
    assert!(page
        .runtime
        .queue_host_promise_completion(
            1,
            page.token(0),
            HostPromiseCompletion::ResolveObject {
                family: family.unwrap(),
                key: HostObjectKey::new(1, 1, 7)
            }
        )
        .unwrap());
    assert_eq!(page.runtime.run_task_turn(1, 0, 1).unwrap().tasks_run, 1);
    page.expect(&["bad then"]);
    assert_eq!(
        page.runtime
            .host_scheduling_usage(1)
            .unwrap()
            .pending_reactions,
        0
    );
}

#[test]
fn repeated_external_completions_release_wait_roots_under_a_small_heap() {
    let mut config = BlueJsPageRuntimeConfig::default();
    config.vm.heap.max_heap_bytes = 1024 * 1024;
    let mut page = Page::with_config(config);
    let program = page
        .runtime
        .install_program(
            1,
            &origin(),
            BlueJsSourceIdentity::new("page:///poll.js", "sha256:poll").unwrap(),
            &BlueJsProgramV1::Script(parse("request('poll').then(()=>record('done'));").unwrap()),
        )
        .unwrap();
    for index in 0..1000 {
        page.runtime.execute_program(1, program).unwrap();
        page.runtime.run_microtask_checkpoint(1).unwrap();
        assert!(page
            .runtime
            .queue_host_promise_completion(
                1,
                page.token(index),
                HostPromiseCompletion::Resolve(HostValue::Null)
            )
            .unwrap());
        assert_eq!(page.runtime.run_task_turn(1, 0, 1).unwrap().tasks_run, 1);
        let usage = page.runtime.host_scheduling_usage(1).unwrap();
        assert_eq!(usage.pending_reactions, 0);
        assert_eq!(usage.pending_browser_tasks, 0);
    }
    assert_eq!(page.log.borrow().len(), 1000);
    assert!(page.runtime.realm_stats(1).unwrap().heap.managed_bytes <= 1024 * 1024);
}

#[test]
fn page_module_top_level_await_resumes_from_a_later_host_completion_task() {
    let mut page = Page::new();
    let program=page.runtime.install_program(1,&origin(),BlueJsSourceIdentity::new("page:///await.mjs","sha256:await").unwrap(),
        &BlueJsProgramV1::Module(parse_module("const value=await request('module');record(value);queueMicrotask(()=>record('module-micro'));").unwrap())).unwrap();
    page.runtime.execute_program(1, program).unwrap();
    page.runtime.run_microtask_checkpoint(1).unwrap();
    page.expect(&[]);
    assert!(page
        .runtime
        .queue_host_promise_completion(
            1,
            page.token(0),
            HostPromiseCompletion::Resolve(HostValue::String("module".into()))
        )
        .unwrap());
    assert_eq!(page.runtime.run_task_turn(1, 0, 1).unwrap().tasks_run, 1);
    page.expect(&["module", "module-micro"]);
}

#[test]
fn many_primitive_arguments_preserve_values_and_partial_conversion_never_starts_io() {
    let mut page = Page::new();
    let pending = page.requests.clone();
    page.runtime
        .configure_realm_bindings(1, |host| {
            host.install_async_global_function(
                "many",
                33,
                HostTaskSource::Networking,
                move |token: HostPromiseToken, args: &[HostValue]| {
                    assert_eq!(args.len(), 33);
                    for (index, value) in args.iter().enumerate() {
                        assert_eq!(value, &HostValue::Number(index as f64));
                    }
                    pending.borrow_mut().push(token);
                    Ok(())
                },
            )
        })
        .unwrap();
    page.execute("var values=Array.from({length:33},(_,index)=>index);many(...values).then(record);many(0,{}).catch(error=>record(error.name));");
    page.expect(&["TypeError"]);
    assert_eq!(page.requests.borrow().len(), 1);
    assert!(page
        .runtime
        .queue_host_promise_completion(
            1,
            page.token(0),
            HostPromiseCompletion::Resolve(HostValue::String("all arguments".into()))
        )
        .unwrap());
    page.runtime.run_task_turn(1, 0, 1).unwrap();
    page.expect(&["TypeError", "all arguments"]);
}
