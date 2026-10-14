// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

//! The `candle` adapter (cargo feature `candle`): Hugging Face's pure-Rust ML
//! framework running a quantized GGUF chat model inside this process
//! (`phase-7-local-ai/PLAN.md`, step C2). Deliberately thin -- prompt
//! construction, the generation loop, and cancellation are in
//! [`super::in_process`] and are tested without weights; this file only
//! translates between those traits and candle/tokenizers.
//!
//! Only the Qwen3 dense architecture with its ChatML template is supported. A
//! model of any other architecture is refused when it is loaded, because a
//! different chat template would be mis-prompted silently rather than fail.
//!
//! The model runs on the CPU. No weights ship with the repository, so the
//! loader is exercised for real only by an `#[ignore]`d test gated on
//! `BLUEICE_CANDLE_GGUF` and `BLUEICE_CANDLE_TOKENIZER`.

use super::in_process::{InProcessBackend, TextCodec, TokenModel};
use candle_core::quantized::gguf_file;
use candle_core::{Device, Tensor, D};
use candle_transformers::models::quantized_qwen3::ModelWeights;
use std::path::Path;
use tokenizers::Tokenizer;

/// The only architecture whose ChatML template this adapter implements.
const SUPPORTED_ARCHITECTURE: &str = "qwen3";
/// The token that ends an assistant turn in ChatML.
const END_OF_TURN: &str = "<|im_end|>";
/// Also ends generation when a model emits it instead.
const END_OF_TEXT: &str = "<|endoftext|>";

struct Qwen3 {
    weights: ModelWeights,
    device: Device,
}

fn describe(error: impl std::fmt::Display) -> String {
    error.to_string()
}

impl TokenModel for Qwen3 {
    fn reset(&mut self) {
        self.weights.clear_kv_cache();
    }

    fn next_token(&mut self, tokens: &[u32], offset: usize) -> Result<u32, String> {
        let input = Tensor::new(tokens, &self.device)
            .and_then(|tensor| tensor.unsqueeze(0))
            .map_err(describe)?;
        // `forward` returns the logits of the last position, shape [1, vocab].
        let logits = self.weights.forward(&input, offset).map_err(describe)?;
        logits
            .squeeze(0)
            .and_then(|logits| logits.argmax(D::Minus1))
            .and_then(|index| index.to_scalar::<u32>())
            .map_err(describe)
    }
}

struct HuggingFaceCodec(Tokenizer);

impl TextCodec for HuggingFaceCodec {
    fn encode(&self, text: &str) -> Result<Vec<u32>, String> {
        self.0
            .encode(text, false)
            .map(|encoding| encoding.get_ids().to_vec())
            .map_err(describe)
    }

    fn decode(&self, ids: &[u32]) -> Result<String, String> {
        self.0.decode(ids, true).map_err(describe)
    }
}

/// Loads a Qwen3 GGUF model and its `tokenizer.json`. `context` is the window
/// in tokens to allow (prompt and answer together); a smaller window bounds
/// memory and latency.
pub fn load(gguf: &Path, tokenizer: &Path, context: usize) -> Result<InProcessBackend, String> {
    let mut file = std::fs::File::open(gguf)
        .map_err(|error| format!("cannot open the model file: {error}"))?;
    let content = gguf_file::Content::read(&mut file).map_err(describe)?;
    let architecture = content
        .metadata
        .get("general.architecture")
        .and_then(|value| value.to_string().ok())
        .cloned()
        .unwrap_or_default();
    if architecture != SUPPORTED_ARCHITECTURE {
        return Err(format!(
            "the candle backend supports only {SUPPORTED_ARCHITECTURE} models, not {architecture:?}"
        ));
    }
    let device = Device::Cpu;
    let weights = ModelWeights::from_gguf(content, &mut file, &device).map_err(describe)?;
    let tokenizer = Tokenizer::from_file(tokenizer).map_err(describe)?;
    let mut stops = vec![tokenizer
        .token_to_id(END_OF_TURN)
        .ok_or_else(|| format!("the tokenizer has no {END_OF_TURN} token"))?];
    stops.extend(tokenizer.token_to_id(END_OF_TEXT));
    Ok(InProcessBackend::new(
        "candle",
        Box::new(Qwen3 { weights, device }),
        Box::new(HuggingFaceCodec(tokenizer)),
        stops,
        context,
        true,
    ))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::backend::{Completion, InferenceBackend};
    use std::path::PathBuf;

    #[test]
    fn a_missing_model_file_is_reported_not_panicked() {
        let error = load(
            Path::new("/nonexistent/model.gguf"),
            Path::new("/nonexistent/tokenizer.json"),
            2048,
        )
        .err()
        .expect("must fail");
        assert!(error.contains("cannot open the model file"), "{error}");
    }

    #[test]
    fn a_file_that_is_not_gguf_is_refused() {
        let path = std::env::temp_dir().join(format!("not-gguf-{}.bin", std::process::id()));
        std::fs::write(&path, b"this is not a GGUF file at all").unwrap();
        let outcome = load(&path, Path::new("/nonexistent/tokenizer.json"), 2048);
        let _ = std::fs::remove_file(&path);
        assert!(outcome.is_err());
    }

    /// Writes a GGUF file that has metadata but no tensors, enough to reach the
    /// loader's architecture check.
    fn gguf_with_architecture(architecture: &str) -> PathBuf {
        use std::sync::atomic::{AtomicU64, Ordering};
        static COUNTER: AtomicU64 = AtomicU64::new(0);
        let path = std::env::temp_dir().join(format!(
            "arch-{}-{}.gguf",
            std::process::id(),
            COUNTER.fetch_add(1, Ordering::Relaxed)
        ));
        let value = gguf_file::Value::String(architecture.to_string());
        let mut file = std::fs::File::create(&path).unwrap();
        gguf_file::write(&mut file, &[("general.architecture", &value)], &[]).unwrap();
        path
    }

    #[test]
    fn an_architecture_whose_chat_template_is_not_implemented_is_refused_at_load() {
        for architecture in ["llama", "qwen2", "qwen35", ""] {
            let path = gguf_with_architecture(architecture);
            let error = load(&path, Path::new("/nonexistent/tokenizer.json"), 2048)
                .err()
                .expect("must be refused");
            let _ = std::fs::remove_file(&path);
            assert!(
                error.contains("supports only qwen3"),
                "{architecture}: {error}"
            );
        }
    }

    #[test]
    fn a_qwen3_file_missing_its_metadata_is_an_error_not_a_panic() {
        let path = gguf_with_architecture("qwen3");
        let outcome = load(&path, Path::new("/nonexistent/tokenizer.json"), 2048);
        let _ = std::fs::remove_file(&path);
        assert!(outcome.is_err());
    }

    /// Real weights, real tokenizer, real generation. None ships with the
    /// repository, so this only runs when pointed at a Qwen3 GGUF:
    /// `BLUEICE_CANDLE_GGUF=... BLUEICE_CANDLE_TOKENIZER=... cargo test
    /// -p blueice-ai-assistant --features candle -- --ignored`.
    #[test]
    #[ignore = "needs a Qwen3 GGUF model and its tokenizer.json"]
    fn a_real_qwen3_model_answers_a_prompt() {
        let gguf =
            PathBuf::from(std::env::var("BLUEICE_CANDLE_GGUF").expect("BLUEICE_CANDLE_GGUF"));
        let tokenizer = PathBuf::from(
            std::env::var("BLUEICE_CANDLE_TOKENIZER").expect("BLUEICE_CANDLE_TOKENIZER"),
        );
        let backend = load(&gguf, &tokenizer, 2048).expect("load the model");
        let answer = backend
            .complete(Completion {
                system: "Answer with one word.",
                user: "What color is the sky on a clear day?",
                max_tokens: 16,
                cancel: None,
            })
            .expect("generate");
        assert!(!answer.trim().is_empty());
    }
}
