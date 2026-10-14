// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

use blueice_engine::script::javascript::PageJavaScriptExecutor;
use blueice_engine::session::{
    run_session, run_session_with_script_and_debugger_requests_and_inline_javascript_executor,
    CoreSessionRequests,
};
use blueice_engine::TabManager;
use serde_json::{json, Value};
use std::io::{Read, Write};
use std::net::Shutdown;
use std::os::unix::net::UnixStream;
use std::path::PathBuf;
use std::sync::atomic::{AtomicU64, Ordering};
use std::thread::JoinHandle;
use std::time::Duration;

pub(crate) fn text(data: &str) -> Value {
    json!({"Text":{"format":"text/plain","data":data}})
}

pub(crate) fn scoped(message: Value, context: u64, window: u64) -> Value {
    json!({"BrowserContext":{"Command":{"context_id":context,"message":{
        "Window":{"Command":{"window_id":window,"message":message}}
    }}}})
}

pub(crate) struct Browser {
    stream: UnixStream,
    worker: Option<JoinHandle<std::io::Result<()>>>,
    root: PathBuf,
    serial: u64,
    pub(crate) context: Value,
}

impl Browser {
    pub(crate) fn new() -> Self {
        Self::with_executor(None)
    }

    pub(crate) fn with_executor(executor: Option<Box<dyn PageJavaScriptExecutor + Send>>) -> Self {
        static NEXT: AtomicU64 = AtomicU64::new(0);
        let root = std::env::temp_dir().join(format!(
            "bi-drag-tests-{}-{}",
            std::process::id(),
            NEXT.fetch_add(1, Ordering::Relaxed)
        ));
        std::fs::create_dir(&root).unwrap();
        let mut tabs = TabManager::new(500.0, 300.0);
        tabs.get_mut(tabs.default_tab()).unwrap().load_html_str(
            "<body style='margin:0'>\
             <input aria-label='Drop text' value='' style='display:block;width:240px;height:32px'>\
             <input aria-label='Disabled drop' value='disabled retained' disabled style='display:block;width:240px;height:32px'>\
             <input aria-label='Inert drop' value='inert retained' inert style='display:block;width:240px;height:32px'>\
             <input aria-label='Read only drop' value='read only retained' readonly style='display:block;width:240px;height:32px'>\
             <input aria-label='Drop files' type=file style='display:block;width:240px;height:32px'>\
             <input aria-label='Protected source' type=password value='private secret' style='display:block;width:240px;height:32px'>\
             <p draggable=true style='margin:0;height:28px'>Drag document words</p>",
            Some("https://example.test/drag".into()),
        );
        let (stream, mut server) = UnixStream::pair().unwrap();
        stream
            .set_read_timeout(Some(Duration::from_secs(5)))
            .unwrap();
        stream
            .set_write_timeout(Some(Duration::from_secs(5)))
            .unwrap();
        let directory = root.clone();
        let worker = std::thread::spawn(move || {
            if let Some(mut executor) = executor {
                run_session_with_script_and_debugger_requests_and_inline_javascript_executor(
                    &mut tabs,
                    &mut server,
                    &directory,
                    &mut 0,
                    &directory.join("unused-gatekeeper.sock"),
                    CoreSessionRequests::default(),
                    Some(executor.as_mut()),
                )
            } else {
                run_session(
                    &mut tabs,
                    &mut server,
                    &directory,
                    &mut 0,
                    &directory.join("unused-gatekeeper.sock"),
                )
            }
        });
        let mut browser = Self {
            stream,
            worker: Some(worker),
            root,
            serial: 0,
            context: Value::Null,
        };
        assert_eq!(
            browser
                .request(json!({"Hello":{"protocol_version":2}}), "Hello")
                .unwrap()["protocol_version"],
            2
        );
        let state = browser
            .request(scoped(json!("GetTextInputState"), 1, 1), "TextInputState")
            .unwrap();
        browser.context = json!({"version":1,"frame_source":state["frame_source"],"document_generation":state["document_generation"]});
        browser
    }

    pub(crate) fn command(&mut self, action: Value) -> Value {
        let frame = self.snapshot()["generation"].clone();
        json!({"NativeDrag":{"context":self.context,"frame_generation":frame,"action":action}})
    }

    pub(crate) fn input(&mut self, action: Value) -> Value {
        let state = self
            .request(scoped(json!("GetTextInputState"), 1, 1), "TextInputState")
            .unwrap();
        let context = json!({"version":1,"frame_source":state["frame_source"],"document_generation":state["document_generation"],"focus_generation":state["focus_generation"]});
        self.request(
            scoped(
                json!({"TextInput":{"context":context,"action":action}}),
                1,
                1,
            ),
            "TextInputState",
        )
        .unwrap()
    }

    pub(crate) fn select_source(&mut self, value: &str) -> Value {
        self.input(json!({"Key":{"key":"Tab","shift":false}}));
        self.input(json!({"Replace":{"text":value,"replacement":null}}));
        let state = self.input(json!("SelectAll"));
        let rect = &state["focused"]["selection_rects"][0];
        json!({"x":rect["x"].as_f64().unwrap()+1.0,"y":rect["y"].as_f64().unwrap()+rect["height"].as_f64().unwrap()/2.0-state["scroll_y"].as_f64().unwrap()})
    }

    pub(crate) fn drag(&mut self, action: Value) -> Result<Value, String> {
        let command = self.command(action);
        self.request(scoped(command, 1, 1), "NativeDragState")
    }

    pub(crate) fn enter(&mut self, id: u64, point: Value, types: Vec<&str>) -> Value {
        self.drag(json!({"Enter":{"drag_id":id,"point":point,"types":types,"allowed_operations":["Copy"]}}))
            .expect("The actual core must recognize and process a native drag session")
    }

    pub(crate) fn snapshot(&mut self) -> Value {
        self.request(scoped(json!("GetRepresentation"), 1, 1), "Representation")
            .unwrap()
    }

    pub(crate) fn node(&mut self, name: &str) -> Value {
        self.snapshot()["nodes"]
            .as_array()
            .unwrap()
            .iter()
            .find(|node| node["name"] == name)
            .expect("A real core control must exist")
            .clone()
    }

    pub(crate) fn point(&mut self, name: &str) -> Value {
        let bounds = self.node(name)["bounds"].clone();
        assert!(bounds["width"].as_f64().unwrap() > 0.0);
        assert!(bounds["height"].as_f64().unwrap() > 0.0);
        json!({"x":bounds["x"].as_f64().unwrap()+8.0,"y":bounds["y"].as_f64().unwrap()+bounds["height"].as_f64().unwrap()/2.0})
    }

    pub(crate) fn value(&mut self, name: &str) -> String {
        self.node(name)["state"]["value"]
            .as_str()
            .unwrap()
            .to_owned()
    }

    pub(crate) fn request(&mut self, message: Value, expected: &str) -> Result<Value, String> {
        self.serial += 1;
        let request = self.serial;
        let data = serde_json::to_vec(&json!({"tab_id":1,"request_id":request,"message":message}))
            .unwrap();
        self.stream
            .write_all(&(data.len() as u32).to_le_bytes())
            .map_err(|e| e.to_string())?;
        self.stream.write_all(&data).map_err(|e| e.to_string())?;
        for _ in 0..64 {
            let mut length = [0; 4];
            self.stream
                .read_exact(&mut length)
                .map_err(|e| format!("Core did not complete {expected}: {e}"))?;
            let length = u32::from_le_bytes(length) as usize;
            assert!(length > 0 && length <= blueice_ipc::MAX_FRAME_BYTES);
            let mut bytes = vec![0; length];
            self.stream
                .read_exact(&mut bytes)
                .map_err(|e| e.to_string())?;
            let reply: Value = serde_json::from_slice(&bytes).map_err(|e| e.to_string())?;
            assert_eq!(reply["request_id"], request);
            if expected != "Hello" {
                assert_eq!(reply["tab_id"], 1);
            }
            let message = &reply["message"];
            if let Some(error) = message.get("Error") {
                return Err(error["message"].as_str().unwrap().to_owned());
            }
            if let Some(result) = message.get(expected) {
                return Ok(result.clone());
            }
            assert!(
                message.get("FrameReady").is_some() || message.get("TextInputState").is_some(),
                "Unexpected core reply: {message}"
            );
        }
        Err("Core response limit reached".into())
    }
}

impl Drop for Browser {
    fn drop(&mut self) {
        let _ = self.stream.shutdown(Shutdown::Both);
        if let Some(worker) = self.worker.take() {
            let result = worker.join();
            if !std::thread::panicking() {
                assert!(result.unwrap().is_ok());
            }
        }
        let _ = std::fs::remove_dir_all(&self.root);
    }
}
