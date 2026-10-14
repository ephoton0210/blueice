// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

//! The loopback-server backend: talks to a local Ollama, Hugging Face TGI, or
//! `llama.cpp` server through `blueice-loopback-model`, so it inherits the
//! shared loopback-only, no-proxy, no-redirect, bounded-size rules rather
//! than reimplementing them.

use super::{Completion, InferenceBackend};
use blueice_loopback_model::{chat, ChatLimits, ChatRequest, LocalModelConfig};
use std::time::Duration;

// The assistant tolerates far more latency and output than the gatekeeper.
const LIMITS: ChatLimits = ChatLimits {
    timeout: Duration::from_secs(60),
    max_request_bytes: 512 * 1024,
    max_reply_bytes: 256 * 1024,
    max_tokens: 0, // replaced per completion
};

pub struct LoopbackBackend {
    config: LocalModelConfig,
}

impl LoopbackBackend {
    /// Validates the endpoint with the shared loopback policy.
    pub fn new(provider: String, base_url: String, model: String) -> Result<Self, String> {
        Ok(LoopbackBackend {
            config: blueice_loopback_model::validate_config(provider, base_url, model)?,
        })
    }
}

impl InferenceBackend for LoopbackBackend {
    fn name(&self) -> &str {
        &self.config.provider
    }

    fn complete(&self, completion: Completion<'_>) -> Result<String, String> {
        // A blocking HTTP call cannot be interrupted, so cancellation is only
        // honoured before it starts.
        if completion.is_cancelled() {
            return Err("cancelled".to_string());
        }
        chat(
            &self.config,
            ChatRequest {
                system: completion.system,
                user: completion.user,
                user_agent: "BlueIce-Assistant/0.1",
            },
            ChatLimits {
                max_tokens: completion.max_tokens,
                ..LIMITS
            },
        )
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::{Read, Write};
    use std::net::TcpListener;
    use std::thread;

    #[test]
    fn a_non_loopback_endpoint_is_refused_at_construction() {
        for base in ["https://127.0.0.1:1/v1/", "http://example.com:80/v1/"] {
            assert!(LoopbackBackend::new("ollama".into(), base.into(), "m".into()).is_err());
        }
    }

    #[test]
    fn a_completion_goes_through_the_shared_bounded_chat_transport() {
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let port = listener.local_addr().unwrap().port();
        let worker = thread::spawn(move || {
            let (mut stream, _) = listener.accept().unwrap();
            let mut bytes = Vec::new();
            let header_end = loop {
                let mut chunk = [0u8; 4096];
                let n = stream.read(&mut chunk).unwrap();
                assert!(n > 0);
                bytes.extend_from_slice(&chunk[..n]);
                if let Some(end) = bytes.windows(4).position(|w| w == b"\r\n\r\n") {
                    break end + 4;
                }
            };
            let length: usize = String::from_utf8_lossy(&bytes[..header_end])
                .lines()
                .find_map(|l| {
                    let (k, v) = l.split_once(':')?;
                    k.eq_ignore_ascii_case("content-length")
                        .then(|| v.trim().parse().unwrap())
                })
                .unwrap();
            while bytes.len() - header_end < length {
                let mut chunk = [0u8; 4096];
                let n = stream.read(&mut chunk).unwrap();
                assert!(n > 0);
                bytes.extend_from_slice(&chunk[..n]);
            }
            let request = String::from_utf8_lossy(&bytes).to_string();
            let body = r#"{"choices":[{"message":{"content":"done"}}]}"#;
            stream
                .write_all(
                    format!(
                        "HTTP/1.1 200 OK\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}",
                        body.len()
                    )
                    .as_bytes(),
                )
                .unwrap();
            request
        });
        let backend = LoopbackBackend::new(
            "llamacpp".into(),
            format!("http://127.0.0.1:{port}/v1/"),
            "m".into(),
        )
        .unwrap();
        assert_eq!(backend.name(), "llamacpp");
        let reply = backend.complete(Completion {
            system: "sys",
            user: "usr",
            max_tokens: 321,
            cancel: None,
        });
        assert_eq!(reply, Ok("done".to_string()));
        let request = worker.join().unwrap();
        assert!(request.contains("BlueIce-Assistant/0.1"));
        assert!(request.contains("\"max_tokens\":321"));
    }

    #[test]
    fn an_unreachable_server_is_a_failed_completion() {
        let port = {
            let listener = TcpListener::bind("127.0.0.1:0").unwrap();
            listener.local_addr().unwrap().port()
        };
        let backend = LoopbackBackend::new(
            "ollama".into(),
            format!("http://127.0.0.1:{port}/v1/"),
            "m".into(),
        )
        .unwrap();
        assert!(backend
            .complete(Completion {
                system: "s",
                user: "u",
                max_tokens: 8,
                cancel: None,
            })
            .is_err());
    }

    #[test]
    fn a_completion_cancelled_before_it_starts_never_reaches_the_server() {
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        listener.set_nonblocking(true).unwrap();
        let port = listener.local_addr().unwrap().port();
        let backend = LoopbackBackend::new(
            "ollama".into(),
            format!("http://127.0.0.1:{port}/v1/"),
            "m".into(),
        )
        .unwrap();
        let token = crate::backend::CancelToken::new();
        token.cancel();
        assert_eq!(
            backend.complete(Completion {
                system: "s",
                user: "u",
                max_tokens: 8,
                cancel: Some(&token),
            }),
            Err("cancelled".to_string())
        );
        assert!(matches!(listener.accept(), Err(e) if e.kind() == std::io::ErrorKind::WouldBlock));
    }
}
