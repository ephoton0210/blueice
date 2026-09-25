// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

//! `blueice-ai-assistant`: the process binary. Deliberately thin -- task and
//! protocol logic live in `blueice_ai_assistant`'s `AssistantService`, covered
//! by its own unit tests. This file only parses arguments, binds a private
//! Unix socket, and serves each accepted connection on its own thread, the
//! way `blueice-ai-gatekeeper`'s binary does.
//!
//! The local model is optional: with no `--model-*` flags every task fails
//! with "no local model is configured" and `core` keeps the original page.

use blueice_ai_assistant::backend::loopback::LoopbackBackend;
use blueice_ai_assistant::backend::{InferenceBackend, NoBackend};
use blueice_ai_assistant::AssistantService;
use blueice_ipc::assistant::default_assistant_socket_path;
use blueice_ipc::local_socket::{bind_private_listener, ensure_private_socket_dir};
use std::io;
use std::path::PathBuf;
use std::process::ExitCode;
use std::sync::Arc;
use std::thread;
use std::time::Duration;

const WRITE_TIMEOUT: Duration = Duration::from_secs(10);

#[derive(Debug, PartialEq)]
struct Args {
    socket: PathBuf,
    model: Option<(String, String, String)>,
}

fn parse_args(args: impl Iterator<Item = String>) -> Result<Args, String> {
    let (mut socket, mut provider, mut base_url, mut name) = (None, None, None, None);
    let mut it = args;
    while let Some(flag) = it.next() {
        let mut value = || it.next().ok_or_else(|| format!("{flag} requires a value"));
        match flag.as_str() {
            "--socket" => socket = Some(PathBuf::from(value()?)),
            "--model-provider" => provider = Some(value()?),
            "--model-base-url" => base_url = Some(value()?),
            "--model-name" => name = Some(value()?),
            other => return Err(format!("unrecognized argument: {other}")),
        }
    }
    let model = match (provider, base_url, name) {
        (None, None, None) => None,
        (Some(provider), Some(base_url), Some(name)) => Some((provider, base_url, name)),
        _ => {
            return Err(
                "--model-provider, --model-base-url, and --model-name must be given together"
                    .to_string(),
            )
        }
    };
    Ok(Args {
        socket: socket.unwrap_or_else(default_assistant_socket_path),
        model,
    })
}

fn run<B: InferenceBackend + 'static>(path: PathBuf, backend: B) -> io::Result<()> {
    if let Some(parent) = path.parent() {
        ensure_private_socket_dir(parent)?;
    }
    // A stale socket from a crashed run would make bind() fail.
    let _ = std::fs::remove_file(&path);
    let listener = bind_private_listener(&path)?;
    let service = Arc::new(AssistantService::new(backend));
    for stream in listener.incoming().flatten() {
        let service = service.clone();
        thread::spawn(move || {
            let _ = stream.set_write_timeout(Some(WRITE_TIMEOUT));
            let mut stream = stream;
            let _ = service.handle_connection(&mut stream);
        });
    }
    Ok(())
}

fn main() -> ExitCode {
    let args = match parse_args(std::env::args().skip(1)) {
        Ok(args) => args,
        Err(message) => {
            eprintln!("blueice-ai-assistant: {message}");
            return ExitCode::FAILURE;
        }
    };
    let result = match args.model {
        Some((provider, base_url, name)) => match LoopbackBackend::new(provider, base_url, name) {
            Ok(backend) => run(args.socket, backend),
            Err(message) => {
                eprintln!("blueice-ai-assistant: {message}");
                return ExitCode::FAILURE;
            }
        },
        None => run(args.socket, NoBackend),
    };
    if let Err(error) = result {
        eprintln!("blueice-ai-assistant: {error}");
        ExitCode::FAILURE
    } else {
        ExitCode::SUCCESS
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn args(flags: &[&str]) -> Result<Args, String> {
        parse_args(flags.iter().map(|flag| flag.to_string()))
    }

    #[test]
    fn no_flags_uses_the_default_socket_and_no_model() {
        let parsed = args(&[]).unwrap();
        assert_eq!(parsed.socket, default_assistant_socket_path());
        assert_eq!(parsed.model, None);
    }

    #[test]
    fn a_socket_and_a_complete_model_are_parsed() {
        let parsed = args(&[
            "--socket",
            "/tmp/a.sock",
            "--model-provider",
            "llamacpp",
            "--model-base-url",
            "http://127.0.0.1:8080/v1/",
            "--model-name",
            "m",
        ])
        .unwrap();
        assert_eq!(parsed.socket, PathBuf::from("/tmp/a.sock"));
        assert_eq!(
            parsed.model,
            Some((
                "llamacpp".into(),
                "http://127.0.0.1:8080/v1/".into(),
                "m".into()
            ))
        );
    }

    #[test]
    fn a_partial_model_or_bad_flag_is_an_error() {
        assert!(args(&["--model-provider", "ollama"]).is_err());
        assert!(args(&["--socket"]).is_err());
        assert!(args(&["--unknown"]).is_err());
    }
}
