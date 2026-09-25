// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

//! The one place BlueIce talks to a local model. Every in-process AI consumer
//! (the safety gatekeeper, the assistant) reaches a model only through this
//! crate, so the loopback-only rules exist once: a credential-free
//! `http://127.0.0.1:<port>/v1/` or `http://[::1]:<port>/v1/` endpoint, no
//! proxy, no redirects, and bounded request, reply, and latency. It holds no
//! consumer logic and no heavyweight inference dependency.

use std::io::Read;
use std::time::Duration;
use url::{Host, Url};

/// A validated local-model endpoint. Only [`validate_config`] builds one.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LocalModelConfig {
    pub provider: String,
    pub base_url: String,
    pub model: String,
}

/// Per-consumer bounds for one chat exchange.
#[derive(Debug, Clone, Copy)]
pub struct ChatLimits {
    pub timeout: Duration,
    pub max_request_bytes: usize,
    pub max_reply_bytes: usize,
    pub max_tokens: u32,
}

/// One system+user chat exchange.
#[derive(Debug, Clone, Copy)]
pub struct ChatRequest<'a> {
    pub system: &'a str,
    pub user: &'a str,
    pub user_agent: &'a str,
}

pub fn validate_config(
    provider: String,
    base_url: String,
    model: String,
) -> Result<LocalModelConfig, String> {
    if !matches!(provider.as_str(), "ollama" | "huggingface" | "llamacpp") {
        return Err("local model provider must be ollama, huggingface, or llamacpp".to_string());
    }
    if model.is_empty()
        || model.len() > 128
        || !model.bytes().all(|byte| {
            byte.is_ascii_alphanumeric()
                || matches!(byte, b'.' | b'_' | b'-' | b'/' | b':' | b'@' | b'+')
        })
    {
        return Err("local model name must be 1–128 safe ASCII characters".to_string());
    }
    let url = Url::parse(&base_url).map_err(|_| "local model base URL is invalid".to_string())?;
    if url.scheme() != "http"
        || !matches!(url.host(), Some(Host::Ipv4(ip)) if ip == std::net::Ipv4Addr::LOCALHOST)
            && !matches!(url.host(), Some(Host::Ipv6(ip)) if ip == std::net::Ipv6Addr::LOCALHOST)
        || url.port().is_none_or(|port| port == 0)
        || !url.username().is_empty()
        || url.password().is_some()
        || url.path() != "/v1/"
        || url.query().is_some()
        || url.fragment().is_some()
    {
        return Err("local model base must be credential-free http://127.0.0.1:<port>/v1/ or http://[::1]:<port>/v1/".to_string());
    }
    Ok(LocalModelConfig {
        provider,
        base_url: url.to_string(),
        model,
    })
}

/// Sends one non-streaming chat request and returns the assistant message
/// content. The configuration is re-validated on every use: a hand-edited
/// persisted file must never turn a consumer into an arbitrary HTTP client.
/// Every failure is an `Err`; callers decide their own fail-open/closed policy.
pub fn chat(
    config: &LocalModelConfig,
    request: ChatRequest<'_>,
    limits: ChatLimits,
) -> Result<String, String> {
    let config = validate_config(
        config.provider.clone(),
        config.base_url.clone(),
        config.model.clone(),
    )?;
    let endpoint = Url::parse(&config.base_url)
        .expect("validated local model base URL must parse")
        .join("chat/completions")
        .expect("fixed chat endpoint must join");
    let mut body = serde_json::json!({
        "model": config.model,
        "stream": false,
        "temperature": 0,
        "max_tokens": limits.max_tokens,
        "messages": [
            {"role": "system", "content": request.system},
            {"role": "user", "content": request.user}
        ]
    });
    // llama.cpp implements this OpenAI-compatible request field as a
    // chat-template switch. For thinking-capable models, callers need the
    // short answer in `content`, not a reasoning trace that can consume the
    // whole output budget. Other providers retain their existing request
    // shape rather than receiving a field they may not support.
    if config.provider == "llamacpp" {
        body["reasoning_effort"] = serde_json::Value::String("none".to_string());
    }
    let body = body.to_string();
    if body.len() > limits.max_request_bytes {
        return Err("local model HTTP request exceeds the bounded limit".to_string());
    }
    let agent: ureq::Agent = ureq::Agent::config_builder()
        .timeout_global(Some(limits.timeout))
        .max_redirects(0)
        .proxy(None)
        .build()
        .into();
    let mut response = agent
        .post(endpoint.as_str())
        .header("User-Agent", request.user_agent)
        .content_type("application/json")
        .send(body)
        .map_err(|_| "local model request failed".to_string())?;
    if response.status().as_u16() != 200 {
        return Err("local model returned a non-success status".to_string());
    }
    let mut bytes = Vec::new();
    response
        .body_mut()
        .as_reader()
        .take((limits.max_reply_bytes + 1) as u64)
        .read_to_end(&mut bytes)
        .map_err(|_| "reading local model reply failed".to_string())?;
    if bytes.len() > limits.max_reply_bytes {
        return Err("local model reply exceeds the bounded limit".to_string());
    }
    let reply: serde_json::Value =
        serde_json::from_slice(&bytes).map_err(|_| "local model reply is not JSON".to_string())?;
    reply
        .pointer("/choices/0/message/content")
        .and_then(serde_json::Value::as_str)
        .map(str::to_string)
        .ok_or_else(|| "local model reply has no assistant content".to_string())
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Write;
    use std::net::TcpListener;
    use std::thread;

    fn served_model_reply(
        provider: &str,
        status: &str,
        content: &str,
    ) -> (LocalModelConfig, thread::JoinHandle<serde_json::Value>) {
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let port = listener.local_addr().unwrap().port();
        let response = serde_json::json!({
            "choices": [{"message": {"content": content}}]
        })
        .to_string();
        let status = status.to_string();
        let worker = thread::spawn(move || {
            let (mut stream, _) = listener.accept().unwrap();
            stream
                .set_read_timeout(Some(Duration::from_secs(2)))
                .unwrap();
            let mut bytes = Vec::new();
            let header_end = loop {
                let mut chunk = [0u8; 1024];
                let len = stream.read(&mut chunk).unwrap();
                assert!(len > 0);
                bytes.extend_from_slice(&chunk[..len]);
                if let Some(end) = bytes.windows(4).position(|part| part == b"\r\n\r\n") {
                    break end + 4;
                }
            };
            let headers = String::from_utf8(bytes[..header_end].to_vec()).unwrap();
            assert!(headers.starts_with("POST /v1/chat/completions HTTP/1.1"));
            assert!(!headers.to_ascii_lowercase().contains("authorization:"));
            let length: usize = headers
                .lines()
                .find_map(|line| {
                    let (name, value) = line.split_once(':')?;
                    name.eq_ignore_ascii_case("Content-Length")
                        .then(|| value.trim().parse().unwrap())
                })
                .unwrap();
            while bytes.len() - header_end < length {
                let mut chunk = [0u8; 1024];
                let len = stream.read(&mut chunk).unwrap();
                assert!(len > 0);
                bytes.extend_from_slice(&chunk[..len]);
            }
            let request: serde_json::Value =
                serde_json::from_slice(&bytes[header_end..header_end + length]).unwrap();
            stream.write_all(format!(
                "HTTP/1.1 {status}\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{response}", response.len()
            ).as_bytes()).unwrap();
            request
        });
        (
            validate_config(
                provider.to_string(),
                format!("http://127.0.0.1:{port}/v1/"),
                "local/model".into(),
            )
            .unwrap(),
            worker,
        )
    }

    #[test]
    fn configuration_rejects_remote_redirectable_or_credentialed_targets() {
        for base in [
            "https://127.0.0.1:11434/v1/",
            "http://localhost:11434/v1/",
            "http://127.0.0.2:11434/v1/",
            "http://127.0.0.1:11434/other/",
            "http://user@127.0.0.1:11434/v1/",
            "http://127.0.0.1:11434/v1/?next=evil",
            "http://127.0.0.1:0/v1/",
        ] {
            assert!(
                validate_config("ollama".into(), base.into(), "local-model".into()).is_err(),
                "accepted {base}"
            );
        }
        assert!(validate_config(
            "ollama".into(),
            "http://127.0.0.1:11434/v1/".into(),
            "local-model".into()
        )
        .is_ok());
        assert!(validate_config(
            "huggingface".into(),
            "http://[::1]:8080/v1/".into(),
            "repo/model".into()
        )
        .is_ok());
        assert!(validate_config(
            "llamacpp".into(),
            "http://127.0.0.1:8080/v1/".into(),
            "local-model".into()
        )
        .is_ok());
    }

    const LIMITS: ChatLimits = ChatLimits {
        timeout: Duration::from_secs(7),
        max_request_bytes: 512 * 1024,
        max_reply_bytes: 8 * 1024,
        max_tokens: 128,
    };

    fn ask(config: &LocalModelConfig) -> Result<String, String> {
        chat(
            config,
            ChatRequest {
                system: "system prompt",
                user: "user text",
                user_agent: "BlueIce-Test/0.1",
            },
            LIMITS,
        )
    }

    #[test]
    fn all_local_providers_use_a_bounded_chat_request_and_return_the_content() {
        for (provider, reasoning_effort) in [
            ("ollama", None),
            ("huggingface", None),
            ("llamacpp", Some("none")),
        ] {
            let (config, worker) = served_model_reply(provider, "200 OK", "the answer");
            assert_eq!(ask(&config), Ok("the answer".to_string()));
            let sent = worker.join().unwrap();
            assert_eq!(sent["model"], "local/model");
            assert_eq!(sent["stream"], false);
            assert_eq!(sent["max_tokens"], 128);
            assert_eq!(sent["reasoning_effort"].as_str(), reasoning_effort);
            assert_eq!(sent["messages"][0]["content"], "system prompt");
            assert_eq!(sent["messages"][1]["content"], "user text");
        }
    }

    #[test]
    fn non_success_replies_are_errors() {
        let (config, worker) = served_model_reply("ollama", "302 Found", "the answer");
        assert!(ask(&config).is_err());
        worker.join().unwrap();
    }

    #[test]
    fn a_reply_without_assistant_content_is_an_error() {
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let port = listener.local_addr().unwrap().port();
        let worker = thread::spawn(move || {
            let (mut stream, _) = listener.accept().unwrap();
            let mut buf = [0u8; 8192];
            let _ = stream.read(&mut buf).unwrap();
            let body = r#"{"choices":[]}"#;
            stream
                .write_all(
                    format!(
                        "HTTP/1.1 200 OK\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}",
                        body.len()
                    )
                    .as_bytes(),
                )
                .unwrap();
        });
        let config = validate_config(
            "ollama".into(),
            format!("http://127.0.0.1:{port}/v1/"),
            "local".into(),
        )
        .unwrap();
        assert!(ask(&config).is_err());
        worker.join().unwrap();
    }

    #[test]
    fn oversized_request_is_rejected_before_any_model_request() {
        let config = validate_config(
            "ollama".into(),
            "http://127.0.0.1:11434/v1/".into(),
            "local".into(),
        )
        .unwrap();
        let big = "a".repeat(LIMITS.max_request_bytes);
        assert!(chat(
            &config,
            ChatRequest {
                system: "s",
                user: &big,
                user_agent: "t"
            },
            LIMITS
        )
        .is_err());
    }

    #[test]
    fn local_model_redirects_are_not_followed_even_to_another_loopback_port() {
        let source = TcpListener::bind("127.0.0.1:0").unwrap();
        let target = TcpListener::bind("127.0.0.1:0").unwrap();
        target.set_nonblocking(true).unwrap();
        let source_port = source.local_addr().unwrap().port();
        let target_port = target.local_addr().unwrap().port();
        let server = thread::spawn(move || {
            let (mut stream, _) = source.accept().unwrap();
            let mut buf = [0u8; 4096];
            let _ = stream.read(&mut buf).unwrap();
            stream.write_all(format!(
                "HTTP/1.1 302 Found\r\nLocation: http://127.0.0.1:{target_port}/v1/chat/completions\r\nContent-Length: 0\r\nConnection: close\r\n\r\n"
            ).as_bytes()).unwrap();
        });
        let config = validate_config(
            "ollama".into(),
            format!("http://127.0.0.1:{source_port}/v1/"),
            "local".into(),
        )
        .unwrap();
        assert!(ask(&config).is_err());
        server.join().unwrap();
        assert!(
            matches!(target.accept(), Err(error) if error.kind() == std::io::ErrorKind::WouldBlock)
        );
    }
}
