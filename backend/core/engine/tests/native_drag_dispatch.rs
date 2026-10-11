// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

#![cfg(unix)]

use blueice_engine::script::javascript::{JavaScriptPageExecutionReport, PageJavaScriptExecutor};
use blueice_engine::{TabId, TabManager};
use blueice_ipc::page_host::{PageHostDragEvent, PageHostDragKind, PageHostDragOutcome};
use serde_json::json;
use std::sync::{Arc, Mutex};
#[allow(dead_code)]
#[path = "fixtures/native_drag_browser.rs"]
mod browser;
use browser::{scoped, Browser};

#[test]
fn native_drag_enters_new_target_before_leaving_old_target_with_scoped_related_target() {
    let events = Arc::new(Mutex::new(Vec::new()));
    let mut browser = Browser::with_executor(Some(Box::new(DragRecorder {
        events: events.clone(),
        replace_enter: None,
        enters: 0,
    })));
    let first = browser.point("Drop text");
    let second = browser.point("Read only drop");
    browser.enter(70, first, vec!["text/plain"]);
    browser.enter(70, second.clone(), vec!["text/plain"]);
    let leave = browser.command(json!({"Leave":{"drag_id":70}}));
    browser
        .request(scoped(leave, 1, 1), "NativeDragState")
        .unwrap();
    let events = events.lock().unwrap();
    assert_eq!(
        events
            .iter()
            .map(|(_, event)| event.kind)
            .collect::<Vec<_>>(),
        [
            PageHostDragKind::DragEnter,
            PageHostDragKind::DragOver,
            PageHostDragKind::DragEnter,
            PageHostDragKind::DragLeave,
            PageHostDragKind::DragOver,
            PageHostDragKind::DragLeave
        ]
    );
    assert_eq!(events[3].1.related_target, Some(events[2].0));
    assert_eq!(
        events[3].1.operation,
        blueice_ipc::input::NativeDragOperation::None
    );
    assert_eq!(
        events[5].1.operation,
        blueice_ipc::input::NativeDragOperation::None
    );
    assert!(events
        .iter()
        .enumerate()
        .all(|(index, (_, event))| index == 3 || event.related_target.is_none()));
    assert_eq!(events[3].1.point().y, second["y"].as_f64().unwrap());
    assert_eq!(
        f64::from_bits(events[3].1.page_y_bits),
        second["y"].as_f64().unwrap()
    );
    assert_ne!(events[2].1.offset_y_bits, events[3].1.offset_y_bits);
}

#[test]
fn native_drag_replacement_during_enter_prevents_old_target_leave_and_over() {
    let events = Arc::new(Mutex::new(Vec::new()));
    let mut browser = Browser::with_executor(Some(Box::new(DragRecorder {
        events: events.clone(),
        replace_enter: Some(2),
        enters: 0,
    })));
    let first = browser.point("Drop text");
    let second = browser.point("Read only drop");
    browser.enter(71, first, vec!["text/plain"]);
    let command = browser.command(json!({"Enter":{"drag_id":71,"point":second,"types":["text/plain"],"allowed_operations":["Copy"]}}));
    assert!(browser
        .request(scoped(command, 1, 1), "NativeDragState")
        .is_err());
    let events = events.lock().unwrap();
    assert_eq!(
        events
            .iter()
            .map(|(_, event)| event.kind)
            .collect::<Vec<_>>(),
        [
            PageHostDragKind::DragEnter,
            PageHostDragKind::DragOver,
            PageHostDragKind::DragEnter
        ]
    );
}

struct DragRecorder {
    events: Arc<Mutex<Vec<(u64, PageHostDragEvent)>>>,
    replace_enter: Option<usize>,
    enters: usize,
}

impl PageJavaScriptExecutor for DragRecorder {
    fn synchronize_and_execute(&mut self, _: &TabManager) -> std::io::Result<()> {
        Ok(())
    }
    fn drain_reports_for_tab(&mut self, _: TabId) -> Vec<JavaScriptPageExecutionReport> {
        Vec::new()
    }
    fn dispatch_drag_serving_script(
        &mut self,
        tabs: &mut TabManager,
        tab: TabId,
        node: u64,
        event: PageHostDragEvent,
        _: Option<&blueice_engine::script::ScriptRequestReceiver>,
    ) -> std::io::Result<Option<PageHostDragOutcome>> {
        if event.kind == PageHostDragKind::DragEnter {
            self.enters += 1;
            if self.replace_enter == Some(self.enters) {
                tabs.get_mut(tab)
                    .unwrap()
                    .load_html_str("<p>Replacement</p>", None);
            }
        }
        self.events.lock().unwrap().push((node, event));
        Ok(None)
    }
}
