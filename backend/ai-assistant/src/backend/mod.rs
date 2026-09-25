// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

//! The inference abstraction. Every assistant task depends only on
//! [`InferenceBackend`], never on a specific runtime, so `llama.cpp` (an
//! external loopback server) and, later, in-process `candle` can be selected
//! or run side by side without touching task or protocol code.

pub mod loopback;

/// One prompt to complete. `user` is always untrusted page data.
#[derive(Debug, Clone, Copy)]
pub struct Completion<'a> {
    pub system: &'a str,
    pub user: &'a str,
    pub max_tokens: u32,
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
        };
        assert_eq!(NoBackend.name(), "none");
        assert_eq!(
            NoBackend.complete(completion),
            Err("no local model is configured".to_string())
        );
    }
}
