// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

//! The inference abstraction. Every assistant task depends only on
//! [`InferenceBackend`], never on a specific runtime, so `llama.cpp` (an
//! external loopback server) and, later, in-process `candle` can be selected
//! or run side by side without touching task or protocol code.

pub mod loopback;
pub mod race;

use std::sync::atomic::{AtomicBool, Ordering};

/// A cooperative cancellation flag. A backend that can stop early (a
/// generation loop) checks it between steps; one that cannot (a blocking HTTP
/// call) checks it before it starts and otherwise finishes on its own, its
/// answer simply unused.
#[derive(Debug, Default)]
pub struct CancelToken(AtomicBool);

impl CancelToken {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn cancel(&self) {
        self.0.store(true, Ordering::Relaxed);
    }

    pub fn is_cancelled(&self) -> bool {
        self.0.load(Ordering::Relaxed)
    }
}

/// One prompt to complete. `user` is always untrusted page data.
#[derive(Debug, Clone, Copy)]
pub struct Completion<'a> {
    pub system: &'a str,
    pub user: &'a str,
    pub max_tokens: u32,
    /// Set by a caller that may stop caring (see [`race::Race`]); `None` means
    /// the completion is never cancelled.
    pub cancel: Option<&'a CancelToken>,
}

impl Completion<'_> {
    pub fn is_cancelled(&self) -> bool {
        self.cancel.is_some_and(CancelToken::is_cancelled)
    }
}

pub trait InferenceBackend: Send + Sync {
    /// A short stable label for logs and status (`"llamacpp"`, `"none"`).
    fn name(&self) -> &str;
    /// Returns the model's reply text. Every failure is an `Err(reason)`; the
    /// caller reports it as a failed task and `core` keeps the original page.
    fn complete(&self, completion: Completion<'_>) -> Result<String, String>;
}

/// Used when no local model is configured: every task fails with a clear
/// reason instead of the process refusing to start.
pub struct NoBackend;

impl InferenceBackend for NoBackend {
    fn name(&self) -> &str {
        "none"
    }

    fn complete(&self, _completion: Completion<'_>) -> Result<String, String> {
        Err("no local model is configured".to_string())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn no_backend_fails_every_completion_with_a_reason() {
        let completion = Completion {
            system: "s",
            user: "u",
            max_tokens: 1,
            cancel: None,
        };
        assert_eq!(NoBackend.name(), "none");
        assert_eq!(
            NoBackend.complete(completion),
            Err("no local model is configured".to_string())
        );
    }

    #[test]
    fn a_cancel_token_is_visible_through_the_completion() {
        let token = CancelToken::new();
        let completion = Completion {
            system: "s",
            user: "u",
            max_tokens: 1,
            cancel: Some(&token),
        };
        assert!(!completion.is_cancelled());
        token.cancel();
        assert!(completion.is_cancelled());
        let uncancellable = Completion {
            cancel: None,
            ..completion
        };
        assert!(!uncancellable.is_cancelled());
    }
}
