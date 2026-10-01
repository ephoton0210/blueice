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

#[cfg(unix)]
#[path = "blueice-ai-assistant/unix.rs"]
mod unix;

#[cfg(unix)]
fn main() -> std::process::ExitCode {
    unix::main()
}

#[cfg(not(unix))]
fn main() -> std::process::ExitCode {
    eprintln!("blueice-ai-assistant requires Unix-domain socket support on this platform");
    std::process::ExitCode::FAILURE
}
