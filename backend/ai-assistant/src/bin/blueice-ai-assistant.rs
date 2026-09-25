// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

//! `blueice-ai-assistant`: the process binary. Deliberately thin -- task and
//! protocol logic live in `blueice_ai_assistant`'s `AssistantService`, covered
//! by its own unit tests. This file parses arguments, picks the inference
//! backend(s), binds a private Unix socket, and serves each accepted
//! connection on its own thread, the way `blueice-ai-gatekeeper`'s binary does.
//!
//! Which backend answers is an explicit choice (`phase-7-local-ai/PLAN.md`,
//! step C3):
//!
//! - `loopback`: a local `llama.cpp`, Ollama, or TGI server, from the
//!   `--model-provider`/`--model-base-url`/`--model-name` flags.
//! - `candle`: an in-process Qwen3 GGUF model (`--candle-model` and
//!   `--candle-tokenizer`; needs the `candle` cargo feature).
//! - `both`: simultaneous mode. The same request goes to both and the first
//!   success wins, which costs double the resources, so it must be asked for
//!   by name -- giving both sets of flags without `--backend` is refused.
//!
//! With no model flags every task fails with "no local model is configured"
//! and `core` keeps the original page.

use blueice_ai_assistant::backend::loopback::LoopbackBackend;
use blueice_ai_assistant::backend::race::Race;
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
/// Tokens of context for an in-process model when `--candle-context` is not
/// given: enough for a page's worth of text, small enough to bound memory.
const DEFAULT_CANDLE_CONTEXT: usize = 4096;

/// Which backend(s) answer requests.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Selection {
    None,
    Loopback,
    Candle,
    Both,
}

#[derive(Debug, PartialEq)]
struct CandleArgs {
    model: PathBuf,
    tokenizer: PathBuf,
    context: usize,
}

#[derive(Debug, PartialEq)]
struct Args {
    socket: PathBuf,
    selection: Selection,
    loopback: Option<(String, String, String)>,
    candle: Option<CandleArgs>,
}

fn parse_args(args: impl Iterator<Item = String>) -> Result<Args, String> {
    let (mut socket, mut provider, mut base_url, mut name) = (None, None, None, None);
    let (mut backend, mut candle_model, mut candle_tokenizer, mut candle_context) =
        (None, None, None, None);
    let mut it = args;
    while let Some(flag) = it.next() {
        let mut value = || it.next().ok_or_else(|| format!("{flag} requires a value"));
        match flag.as_str() {
            "--socket" => socket = Some(PathBuf::from(value()?)),
            "--model-provider" => provider = Some(value()?),
            "--model-base-url" => base_url = Some(value()?),
            "--model-name" => name = Some(value()?),
            "--backend" => backend = Some(value()?),
            "--candle-model" => candle_model = Some(PathBuf::from(value()?)),
            "--candle-tokenizer" => candle_tokenizer = Some(PathBuf::from(value()?)),
            "--candle-context" => {
                candle_context = Some(
                    value()?
                        .parse::<usize>()
                        .ok()
                        .filter(|n| *n >= 64)
                        .ok_or_else(|| {
                            "--candle-context must be a number of at least 64".to_string()
                        })?,
                )
            }
            other => return Err(format!("unrecognized argument: {other}")),
        }
    }
    let loopback = match (provider, base_url, name) {
        (None, None, None) => None,
        (Some(provider), Some(base_url), Some(name)) => Some((provider, base_url, name)),
        _ => {
            return Err(
                "--model-provider, --model-base-url, and --model-name must be given together"
                    .to_string(),
            )
        }
    };
    let candle = match (candle_model, candle_tokenizer) {
        (None, None) => {
            if candle_context.is_some() {
                return Err("--candle-context requires --candle-model".to_string());
            }
            None
        }
        (Some(model), Some(tokenizer)) => Some(CandleArgs {
            model,
            tokenizer,
            context: candle_context.unwrap_or(DEFAULT_CANDLE_CONTEXT),
        }),
        _ => {
            return Err("--candle-model and --candle-tokenizer must be given together".to_string())
        }
    };
    let selection = select(backend.as_deref(), loopback.is_some(), candle.is_some())?;
    Ok(Args {
        socket: socket.unwrap_or_else(default_assistant_socket_path),
        selection,
        loopback,
        candle,
    })
}

/// Resolves `--backend` against which flag sets were given. Every combination
/// that would silently do something other than what was asked is an error.
fn select(choice: Option<&str>, has_loopback: bool, has_candle: bool) -> Result<Selection, String> {
    let requires = |wanted: bool, flags: &str, backend: &str| {
        if wanted {
            Ok(())
        } else {
            Err(format!("--backend {backend} needs {flags}"))
        }
    };
    let forbids = |present: bool, flags: &str, backend: &str| {
        if present {
            Err(format!("--backend {backend} does not use {flags}"))
        } else {
            Ok(())
        }
    };
    const LOOPBACK_FLAGS: &str = "--model-provider, --model-base-url, and --model-name";
    const CANDLE_FLAGS: &str = "--candle-model and --candle-tokenizer";
    match choice {
        None => match (has_loopback, has_candle) {
            (false, false) => Ok(Selection::None),
            (true, false) => Ok(Selection::Loopback),
            (false, true) => Ok(Selection::Candle),
            (true, true) => Err(
                "both a loopback model and a candle model were given; choose \
                 --backend loopback, candle, or both (both runs them at once and costs double the resources)"
                    .to_string(),
            ),
        },
        Some("loopback") => {
            requires(has_loopback, LOOPBACK_FLAGS, "loopback")?;
            forbids(has_candle, CANDLE_FLAGS, "loopback")?;
            Ok(Selection::Loopback)
        }
        Some("candle") => {
            requires(has_candle, CANDLE_FLAGS, "candle")?;
            forbids(has_loopback, LOOPBACK_FLAGS, "candle")?;
            Ok(Selection::Candle)
        }
        Some("both") => {
            requires(has_loopback && has_candle, "both sets of model flags", "both")?;
            Ok(Selection::Both)
        }
        Some(other) => Err(format!(
            "--backend must be loopback, candle, or both, not {other:?}"
        )),
    }
}

fn loopback_backend(
    (provider, base_url, name): (String, String, String),
) -> Result<Arc<dyn InferenceBackend>, String> {
    Ok(Arc::new(LoopbackBackend::new(provider, base_url, name)?))
}

#[cfg(feature = "candle")]
fn candle_backend(args: &CandleArgs) -> Result<Arc<dyn InferenceBackend>, String> {
    Ok(Arc::new(blueice_ai_assistant::backend::candle::load(
        &args.model,
        &args.tokenizer,
        args.context,
    )?))
}

#[cfg(not(feature = "candle"))]
fn candle_backend(_args: &CandleArgs) -> Result<Arc<dyn InferenceBackend>, String> {
    Err("this build has no in-process candle backend (rebuild with --features candle)".to_string())
}

/// Builds what `args` selected, refusing at startup rather than at the first
/// request when anything cannot be loaded.
fn build_backend(args: Args) -> Result<Arc<dyn InferenceBackend>, String> {
    let (loopback, candle) = (args.loopback, args.candle);
    match (args.selection, loopback, candle) {
        (Selection::Loopback, Some(loopback), _) => loopback_backend(loopback),
        (Selection::Candle, _, Some(candle)) => candle_backend(&candle),
        (Selection::Both, Some(loopback), Some(candle)) => Ok(Arc::new(Race::new(
            loopback_backend(loopback)?,
            candle_backend(&candle)?,
        ))),
        _ => Ok(Arc::new(NoBackend)),
    }
}

fn run(path: PathBuf, backend: Arc<dyn InferenceBackend>) -> io::Result<()> {
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
    let socket = args.socket.clone();
    let backend = match build_backend(args) {
        Ok(backend) => backend,
        Err(message) => {
            eprintln!("blueice-ai-assistant: {message}");
            return ExitCode::FAILURE;
        }
    };
    if let Err(error) = run(socket, backend) {
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

    const LOOPBACK: [&str; 6] = [
        "--model-provider",
        "llamacpp",
        "--model-base-url",
        "http://127.0.0.1:8080/v1/",
        "--model-name",
        "m",
    ];
    const CANDLE: [&str; 4] = ["--candle-model", "/m.gguf", "--candle-tokenizer", "/t.json"];

    fn with(base: &[&str], extra: &[&str]) -> Vec<String> {
        base.iter().chain(extra).map(|s| s.to_string()).collect()
    }

    fn parse(flags: Vec<String>) -> Result<Args, String> {
        parse_args(flags.into_iter())
    }

    #[test]
    fn no_flags_uses_the_default_socket_and_no_model() {
        let parsed = args(&[]).unwrap();
        assert_eq!(parsed.socket, default_assistant_socket_path());
        assert_eq!(parsed.selection, Selection::None);
        assert_eq!(parsed.loopback, None);
        assert_eq!(parsed.candle, None);
    }

    #[test]
    fn a_socket_and_a_complete_loopback_model_are_parsed() {
        let parsed = parse(with(&LOOPBACK, &["--socket", "/tmp/a.sock"])).unwrap();
        assert_eq!(parsed.socket, PathBuf::from("/tmp/a.sock"));
        assert_eq!(parsed.selection, Selection::Loopback);
        assert_eq!(
            parsed.loopback,
            Some((
                "llamacpp".into(),
                "http://127.0.0.1:8080/v1/".into(),
                "m".into()
            ))
        );
    }

    #[test]
    fn a_candle_model_alone_selects_candle_with_a_default_context() {
        let parsed = parse(with(&CANDLE, &[])).unwrap();
        assert_eq!(parsed.selection, Selection::Candle);
        assert_eq!(
            parsed.candle,
            Some(CandleArgs {
                model: "/m.gguf".into(),
                tokenizer: "/t.json".into(),
                context: DEFAULT_CANDLE_CONTEXT,
            })
        );
        let sized = parse(with(&CANDLE, &["--candle-context", "2048"])).unwrap();
        assert_eq!(sized.candle.unwrap().context, 2048);
    }

    #[test]
    fn simultaneous_mode_must_be_asked_for_by_name() {
        let both = [LOOPBACK.as_slice(), CANDLE.as_slice()].concat();
        // Both flag sets without --backend is ambiguous: refused, and the
        // message names the double-resource cost.
        let error = parse(with(&both, &[])).unwrap_err();
        assert!(error.contains("double the resources"), "{error}");
        let chosen = parse(with(&both, &["--backend", "both"])).unwrap();
        assert_eq!(chosen.selection, Selection::Both);
        // Naming one of the two picks it, but the other set must not be present.
        assert!(parse(with(&both, &["--backend", "loopback"])).is_err());
        assert!(parse(with(&both, &["--backend", "candle"])).is_err());
    }

    #[test]
    fn an_explicit_backend_needs_its_own_flags() {
        assert!(args(&["--backend", "loopback"]).is_err());
        assert!(args(&["--backend", "candle"]).is_err());
        assert!(args(&["--backend", "both"]).is_err());
        assert!(parse(with(&LOOPBACK, &["--backend", "both"])).is_err());
        assert!(parse(with(&CANDLE, &["--backend", "both"])).is_err());
        assert!(parse(with(&LOOPBACK, &["--backend", "loopback"])).is_ok());
        assert!(parse(with(&CANDLE, &["--backend", "candle"])).is_ok());
        assert!(args(&["--backend", "quantum"]).is_err());
    }

    #[test]
    fn partial_or_invalid_flags_are_errors() {
        assert!(args(&["--model-provider", "ollama"]).is_err());
        assert!(args(&["--candle-model", "/m.gguf"]).is_err());
        assert!(args(&["--candle-tokenizer", "/t.json"]).is_err());
        assert!(args(&["--candle-context", "2048"]).is_err());
        assert!(parse(with(&CANDLE, &["--candle-context", "10"])).is_err());
        assert!(parse(with(&CANDLE, &["--candle-context", "lots"])).is_err());
        assert!(args(&["--socket"]).is_err());
        assert!(args(&["--unknown"]).is_err());
    }

    #[test]
    fn no_selection_builds_the_backend_that_says_it_is_unconfigured() {
        let backend = build_backend(args(&[]).unwrap()).unwrap();
        assert_eq!(backend.name(), "none");
    }

    #[test]
    fn a_loopback_selection_builds_the_loopback_backend() {
        let backend = build_backend(parse(with(&LOOPBACK, &[])).unwrap()).unwrap();
        assert_eq!(backend.name(), "llamacpp");
    }

    #[test]
    fn a_candle_model_that_cannot_be_loaded_stops_startup() {
        let error = build_backend(parse(with(&CANDLE, &[])).unwrap())
            .err()
            .expect("the model file does not exist");
        assert!(!error.is_empty());
        let both = [LOOPBACK.as_slice(), CANDLE.as_slice()].concat();
        assert!(build_backend(parse(with(&both, &["--backend", "both"])).unwrap()).is_err());
    }
}
