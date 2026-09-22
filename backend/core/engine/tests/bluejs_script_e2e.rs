// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

//! Phase 13's end-to-end proof: a script-bearing Phase 3 page is parsed into
//! the core DOM, BlueJS executes its script over the real script IPC boundary,
//! and the mutated DOM reaches paint.

use blueice_bluejs::{ScriptDomClient, Vm, run_script_process};
use blueice_engine::{TabManager, session};
use blueice_engine::script::{ScriptScheduler, ScriptSession, handle_script_connection};
use blueice_ipc::script::ScriptCommand;
use blueice_paint::PaintCommand;
use std::os::unix::net::UnixStream;
use std::sync::mpsc;
use std::thread;

#[test]
fn script_bearing_fixture_mutates_the_core_dom_and_its_phase_3_render_output() {
    let fixture = r#"<ul id="target" style="display: none"><li class="source first">old</li><li class="source">kept</li></ul><script>
        let target = document.querySelector('#target');
        let count = document.querySelectorAll('.source').length;
        let first = document.querySelector('.first');
        let inserted = document.createElement('li');
        inserted.textContent = 'inserted';
        target.insertBefore(inserted, first);
        first.remove();
        target.classList.add('ready');
        target.style.display = 'block';
        target.setAttribute('data-count', count);
        let message = document.createElement('p');
        message.textContent = `${target.className}:${target.style.display}:${target.getAttribute('data-count')}:${target.innerHTML.includes('inserted')}`;
        target.appendChild(message);
    </script>"#;
    let (client, mut core) = UnixStream::pair().unwrap();
    let (result_tx, result_rx) = mpsc::channel();
    let (script_tx, script_rx) = mpsc::channel();
    let core_thread = thread::spawn(move || {
        let mut tabs = TabManager::new(320.0, 200.0);
        let tab = tabs.default_tab();
        tabs.get_mut(tab).unwrap().load_html_str(fixture, None);
        script_tx
            .send(tabs.get(tab).unwrap().inline_script_sources())
            .unwrap();
        handle_script_connection(&mut tabs, &mut core).unwrap();
        let page = tabs.get(tab).unwrap();
        result_tx.send((page.dom_dump(), page.render())).unwrap();
    });

    let host = ScriptDomClient::handshake(client, 1).unwrap();
    let mut vm = Vm::new();
    vm.set_dom_host(Box::new(host)).unwrap();
    let scripts = script_rx.recv().unwrap();
    assert_eq!(scripts.len(), 1);
    vm.evaluate(&scripts[0]).unwrap();
    drop(vm);

    let (dom, frame) = result_rx.recv().unwrap();
    core_thread.join().unwrap();
    assert!(dom.contains("class=\"ready\""));
    assert!(dom.contains("data-count=\"2\""));
    assert!(dom.contains("\"inserted\""));
    assert!(!dom.contains("\"old\""));
    let painted_text = frame
        .commands
        .iter()
        .filter_map(|command| match command {
            PaintCommand::Text { text, .. } => Some(text.as_str()),
            _ => None,
        })
        .collect::<Vec<_>>()
        .join(" ");
    assert_eq!(painted_text, "inserted kept ready:block:2:true");
}

#[test]
fn event_and_timer_turns_cross_the_real_core_bluejs_process_protocol() {
    let fixture = r#"<button id="activate">activate</button><p id="event">waiting</p><p id="timer">pending</p><script>
        function eventLabel(event) { return event.type + ':' + event.target.id; }
    </script><script>
        let button = document.getElementById('activate');
        let eventText = document.getElementById('event');
        let timerText = document.getElementById('timer');
        button.addEventListener('click', event => {
            eventText.textContent = eventLabel(event);
            event.preventDefault();
        });
        setTimeout(() => { timerText.textContent = 'timer-fired'; }, 0);
    </script>"#;
    let (bluejs, core) = UnixStream::pair().unwrap();
    let (result_tx, result_rx) = mpsc::channel();
    let script_thread = thread::spawn(move || run_script_process(bluejs));
    let core_thread = thread::spawn(move || {
        let mut tabs = TabManager::new(320.0, 200.0);
        let tab = tabs.default_tab();
        tabs.get_mut(tab).unwrap().load_html_str(fixture, None);
        let button = tabs
            .get(tab)
            .unwrap()
            .script_get_element_by_id("activate")
            .unwrap();
        let mut session = ScriptSession::accept(core).unwrap();
        session.run_document_scripts(&mut tabs, tab).unwrap();
        let event = session
            .dispatch_event(&mut tabs, tab, button, "click")
            .unwrap();
        assert!(event.default_prevented);
        assert!(event.ran_event);
        let timer = session
            .run_turn(&mut tabs, tab, ScriptCommand::RunDueTimers { tab_id: tab.as_u64() })
            .unwrap();
        assert!(timer.ran_timers);
        session.shutdown().unwrap();
        result_tx.send(tabs.get(tab).unwrap().dom_dump()).unwrap();
    });

    let dom = result_rx.recv().unwrap();
    core_thread.join().unwrap();
    script_thread.join().unwrap().unwrap();
    assert!(dom.contains("click:activate"));
    assert!(dom.contains("timer-fired"));
}

#[test]
fn core_session_dispatches_click_before_a_links_default_navigation() {
    let fixture = r#"<a id="activate" href="https://must-not-navigate.invalid/">activate</a><p id="event">waiting</p><script>
        let link = document.getElementById('activate');
        let eventText = document.getElementById('event');
        link.addEventListener('click', event => {
            eventText.textContent = 'default-prevented:' + event.target.id;
            event.preventDefault();
        });
    </script>"#;
    let frame_dir = std::env::temp_dir().join(format!(
        "blueice-bluejs-click-e2e-{}",
        std::process::id()
    ));
    let _ = std::fs::remove_dir_all(&frame_dir);
    let (mut client, server) = UnixStream::pair().unwrap();
    let (bluejs, script_core) = UnixStream::pair().unwrap();
    let script_thread = thread::spawn(move || run_script_process(bluejs));
    let frame_dir_for_core = frame_dir.clone();
    let core_thread = thread::spawn(move || {
        let mut server = server;
        let mut tabs = TabManager::new(320.0, 200.0);
        let tab = tabs.default_tab();
        tabs.get_mut(tab).unwrap().load_html_str(fixture, None);
        let mut script = ScriptSession::accept(script_core).unwrap();
        script.run_document_scripts(&mut tabs, tab).unwrap();
        let mut generation = 0;
        session::run_session_with_script(
            &mut tabs,
            &mut server,
            &frame_dir_for_core,
            &mut generation,
            std::path::Path::new("/tmp/blueice-no-gatekeeper.sock"),
            &mut script,
        )
        .unwrap();
        script.shutdown().unwrap();
    });

    blueice_ipc::client_handshake(&mut client).unwrap();
    client
        .set_read_timeout(Some(std::time::Duration::from_secs(5)))
        .unwrap();
    blueice_ipc::write_client_message(&mut client, &blueice_ipc::ClientMessage::Click { x: 2.0, y: 2.0 })
        .unwrap();
    assert!(matches!(
        blueice_ipc::read_server_message(&mut client).unwrap(),
        blueice_ipc::ServerMessage::FrameReady { .. },
    ));
    blueice_ipc::write_client_message(&mut client, &blueice_ipc::ClientMessage::GetDom).unwrap();
    let dom = match blueice_ipc::read_server_message(&mut client).unwrap() {
        blueice_ipc::ServerMessage::Dom(dom) => dom,
        reply => panic!("expected DOM after prevented link click, got {reply:?}"),
    };
    assert!(dom.contains("default-prevented:activate"));
    blueice_ipc::write_client_message(&mut client, &blueice_ipc::ClientMessage::Shutdown).unwrap();
    core_thread.join().unwrap();
    script_thread.join().unwrap().unwrap();
    let _ = std::fs::remove_dir_all(frame_dir);
}
