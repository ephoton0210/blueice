// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

use super::*;

pub(super) struct Transcript {
    pub(super) writer: BufWriter<File>,
}

impl Transcript {
    pub(super) fn create(path: &Path) -> Result<Self, String> {
        if let Some(parent) = path
            .parent()
            .filter(|parent| !parent.as_os_str().is_empty())
        {
            std::fs::create_dir_all(parent).map_err(|error| {
                format!(
                    "creating transcript directory {}: {error}",
                    parent.display()
                )
            })?;
        }
        let file = OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(path)
            .map_err(|error| {
                format!(
                    "creating transcript {} (it must not already exist): {error}",
                    path.display()
                )
            })?;
        Ok(Self {
            writer: BufWriter::new(file),
        })
    }

    pub(super) fn record(&mut self, kind: &str, value: Value) -> Result<(), String> {
        serde_json::to_writer(&mut self.writer, &json!({ "kind": kind, "data": value }))
            .map_err(|error| format!("encoding transcript: {error}"))?;
        self.writer
            .write_all(b"\n")
            .map_err(|error| format!("writing transcript: {error}"))?;
        self.writer
            .flush()
            .map_err(|error| format!("flushing transcript: {error}"))
    }
}

pub(super) struct McpProcess {
    pub(super) child: Child,
    pub(super) stdin: Option<BufWriter<ChildStdin>>,
    pub(super) replies: Receiver<Value>,
    pub(super) next_id: u64,
}

impl McpProcess {
    pub(super) fn start(binary: &Path, launcher_socket: &Path) -> Result<Self, String> {
        if !binary.is_file() {
            return Err(format!(
                "MCP server binary is not a file: {}",
                binary.display()
            ));
        }
        let mut child = Command::new(binary)
            .arg("--launcher-socket")
            .arg(launcher_socket)
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::inherit())
            .spawn()
            .map_err(|error| format!("starting {}: {error}", binary.display()))?;
        let stdin = child
            .stdin
            .take()
            .ok_or_else(|| "MCP server did not expose stdin".to_string())?;
        let stdout = child
            .stdout
            .take()
            .ok_or_else(|| "MCP server did not expose stdout".to_string())?;
        let (sender, replies) = mpsc::channel();
        thread::spawn(move || {
            for line in BufReader::new(stdout).lines() {
                let Ok(line) = line else { return };
                let Ok(value) = serde_json::from_str::<Value>(&line) else {
                    continue;
                };
                if sender.send(value).is_err() {
                    return;
                }
            }
        });
        let mut process = Self {
            child,
            stdin: Some(BufWriter::new(stdin)),
            replies,
            next_id: 1,
        };
        let initialized = process.request(
            "initialize",
            json!({
                "protocolVersion": "2025-06-18",
                "capabilities": {},
                "clientInfo": { "name": "blueice-phase6-agent", "version": "0.1.0" },
            }),
        )?;
        if initialized.get("error").is_some() {
            return Err(format!("MCP initialization failed: {initialized}"));
        }
        process.notify("notifications/initialized", json!({}))?;
        Ok(process)
    }

    pub(super) fn send(&mut self, message: &Value) -> Result<(), String> {
        let stdin = self
            .stdin
            .as_mut()
            .ok_or_else(|| "MCP server stdin is already closed".to_string())?;
        serde_json::to_writer(&mut *stdin, message)
            .map_err(|error| format!("writing MCP JSON: {error}"))?;
        stdin
            .write_all(b"\n")
            .and_then(|_| stdin.flush())
            .map_err(|error| format!("flushing MCP request: {error}"))
    }

    pub(super) fn request(&mut self, method: &str, params: Value) -> Result<Value, String> {
        let id = self.next_id;
        self.next_id += 1;
        self.send(&json!({ "jsonrpc": "2.0", "id": id, "method": method, "params": params }))?;
        let deadline = Instant::now() + MCP_TIMEOUT;
        loop {
            let remaining = deadline.saturating_duration_since(Instant::now());
            let reply = self
                .replies
                .recv_timeout(remaining)
                .map_err(|_| format!("timed out waiting for MCP {method}"))?;
            if reply.get("id") == Some(&json!(id)) {
                return Ok(reply);
            }
        }
    }

    pub(super) fn notify(&mut self, method: &str, params: Value) -> Result<(), String> {
        self.send(&json!({ "jsonrpc": "2.0", "method": method, "params": params }))
    }

    pub(super) fn call_tool(
        &mut self,
        name: &str,
        arguments: Value,
    ) -> Result<McpToolResult, String> {
        let reply = self.request(
            "tools/call",
            json!({ "name": name, "arguments": arguments }),
        )?;
        if let Some(error) = reply.get("error") {
            return Err(format!("MCP {name} protocol error: {error}"));
        }
        let result = reply
            .get("result")
            .ok_or_else(|| format!("MCP {name} reply has no result: {reply}"))?;
        let text = result["content"]
            .as_array()
            .into_iter()
            .flatten()
            .filter(|content| content["type"] == "text")
            .filter_map(|content| content["text"].as_str())
            .collect::<Vec<_>>()
            .join("\n");
        if result["isError"] == Value::Bool(true) {
            return Err(format!("MCP {name} rejected the scenario action: {text}"));
        }
        let image = result["content"]
            .as_array()
            .into_iter()
            .flatten()
            .find(|content| content["type"] == "image" && content["mimeType"] == "image/png")
            .and_then(|content| content["data"].as_str())
            .map(|data| {
                base64::engine::general_purpose::STANDARD
                    .decode(data)
                    .map_err(|error| format!("decoding MCP PNG: {error}"))
            })
            .transpose()?;
        Ok(McpToolResult { text, image })
    }

    pub(super) fn close(&mut self) {
        self.stdin.take();
        let deadline = Instant::now() + Duration::from_secs(2);
        while Instant::now() < deadline {
            match self.child.try_wait() {
                Ok(Some(_)) | Err(_) => return,
                Ok(None) => thread::sleep(Duration::from_millis(20)),
            }
        }
        let _ = self.child.kill();
        let _ = self.child.wait();
    }
}

impl Drop for McpProcess {
    fn drop(&mut self) {
        self.close();
    }
}

pub(super) struct McpToolResult {
    pub(super) text: String,
    pub(super) image: Option<Vec<u8>>,
}

/// A deliberately small common transport for the supported local
/// OpenAI-compatible Chat Completions servers. Keeping this transport generic
/// does not widen browser authority: every tool action remains bounded below.
pub(super) struct LocalChat {
    pub(super) provider: LocalModelProvider,
    pub(super) endpoint: Url,
}

impl LocalChat {
    pub(super) fn new(provider: LocalModelProvider, provider_base: Url) -> Result<Self, String> {
        let endpoint = provider_base
            .join("chat/completions")
            .map_err(|error| format!("creating {} chat endpoint: {error}", provider.name()))?;
        Ok(Self { provider, endpoint })
    }

    pub(super) fn create(&self, request: &Value) -> Result<Value, String> {
        let body = serde_json::to_string(request)
            .map_err(|error| format!("encoding {} chat request: {error}", self.provider.name()))?;
        let mut response = ureq::post(self.endpoint.as_str())
            .header("User-Agent", "BlueIce-Phase6-Agent/0.1")
            .content_type("application/json")
            .send(body)
            .map_err(|error| format!("calling local {}: {error}", self.provider.name()))?;
        let body = response
            .body_mut()
            .read_to_string()
            .map_err(|error| format!("reading {} reply: {error}", self.provider.name()))?;
        serde_json::from_str(&body).map_err(|error| {
            format!(
                "parsing {} reply: {error}; body: {body}",
                self.provider.name()
            )
        })
    }
}

pub(super) fn function_calls(response: &Value) -> Result<Vec<Value>, String> {
    let message = response["choices"]
        .as_array()
        .and_then(|choices| choices.first())
        .map(|choice| &choice["message"])
        .ok_or_else(|| "local chat reply has no choices[0].message".to_string())?;
    match &message["tool_calls"] {
        Value::Null => Ok(Vec::new()),
        Value::Array(calls) => Ok(calls.clone()),
        // TGI has also returned one object instead of a one-element array.
        // It has the same constrained processing path as the standard form.
        Value::Object(_) => Ok(vec![message["tool_calls"].clone()]),
        _ => Err("local chat reply has malformed tool_calls".to_string()),
    }
}

pub(super) fn tool_call_id(call: &Value) -> Result<String, String> {
    if let Some(id) = call["id"].as_str() {
        return Ok(id.to_string());
    }
    if let Some(id) = call["id"].as_i64() {
        return Ok(id.to_string());
    }
    Err("local tool call has no string or integer id".to_string())
}

pub(super) fn final_text(response: &Value) -> String {
    response["choices"]
        .as_array()
        .and_then(|choices| choices.first())
        .and_then(|choice| choice["message"]["content"].as_str())
        .filter(|text| !text.trim().is_empty())
        .map(ToOwned::to_owned)
        .unwrap_or_default()
}
