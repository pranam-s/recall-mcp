//! Local semantic embeddings via ONNX, behind the non-default `onnx` feature
//! (D-013). Uses `fastembed` 6.x on `ort` 2.0.0-rc.13 with ONNX Runtime loaded
//! at runtime (`ort-load-dynamic`): the windows-gnu toolchain has no ort
//! prebuilt binaries, but the official MSVC-built `onnxruntime.dll` is C ABI
//! and loads fine via `ORT_DYLIB_PATH`.
//!
//! Model download: first construction fetches the model from Hugging Face
//! (rustls) into the cache dir (`.fastembed_cache/` under the process cwd by
//! default; override with [`OnnxEmbedder::with_cache_dir`]). Later runs are
//! fully offline.
//!
//! Determinism: embeddings are produced one text per call, so batch shape is
//! constant; identical inputs yield bit-identical vectors (verified by the
//! spike and the unit tests). Cross-batch ONNX variance does not apply.

use std::sync::Mutex;

use crate::embed::Embedder;
use crate::error::{RecallError, Result};

/// Models exposed by [`OnnxEmbedder`]. All are 384-dimensional.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum OnnxModel {
    /// `all-MiniLM-L6-v2` (int8 quantized): ~23 MB download, fastembed's
    /// default general-purpose English model.
    AllMiniLmL6V2Q,
    /// `bge-small-en-v1.5` (int8 quantized): higher retrieval quality, larger
    /// download.
    BgeSmallEnV15Q,
}

impl OnnxModel {
    fn fastembed(self) -> fastembed::EmbeddingModel {
        match self {
            OnnxModel::AllMiniLmL6V2Q => fastembed::EmbeddingModel::AllMiniLML6V2Q,
            OnnxModel::BgeSmallEnV15Q => fastembed::EmbeddingModel::BGESmallENV15Q,
        }
    }

    /// Dimensionality of the model's output vectors.
    pub fn dimensions(self) -> usize {
        384
    }

    /// Stable embedder name recorded in configs and eval reports.
    pub fn as_str(self) -> &'static str {
        match self {
            OnnxModel::AllMiniLmL6V2Q => "onnx:all-minilm-l6-v2-q",
            OnnxModel::BgeSmallEnV15Q => "onnx:bge-small-en-v1.5-q",
        }
    }
}

/// [`Embedder`] backed by a local ONNX sentence-transformer (offline after the
/// first model download). Inference is serialized through a mutex; failures
/// are logged and yield a zero vector (the same "no signal" value the rest of
/// the system already handles for absent embeddings).
pub struct OnnxEmbedder {
    model: OnnxModel,
    inner: Mutex<fastembed::TextEmbedding>,
}

impl OnnxEmbedder {
    /// Default model: `all-MiniLM-L6-v2` quantized (384 dims).
    pub fn new() -> Result<Self> {
        Self::with_model(OnnxModel::AllMiniLmL6V2Q)
    }

    /// Build with an explicit model; downloads it if not cached yet.
    pub fn with_model(model: OnnxModel) -> Result<Self> {
        let inner =
            fastembed::TextEmbedding::try_new(fastembed::InitOptions::new(model.fastembed()))
                .map_err(|e| RecallError::Embedder(format!("ONNX model init failed: {e}")))?;
        Ok(Self {
            model,
            inner: Mutex::new(inner),
        })
    }

    /// Build with an explicit model and cache directory for the download.
    pub fn with_cache_dir(model: OnnxModel, cache_dir: std::path::PathBuf) -> Result<Self> {
        let options = fastembed::InitOptions::new(model.fastembed())
            .with_cache_dir(cache_dir)
            .with_show_download_progress(true);
        let inner = fastembed::TextEmbedding::try_new(options)
            .map_err(|e| RecallError::Embedder(format!("ONNX model init failed: {e}")))?;
        Ok(Self {
            model,
            inner: Mutex::new(inner),
        })
    }
}

impl Embedder for OnnxEmbedder {
    fn dimensions(&self) -> usize {
        self.model.dimensions()
    }

    fn name(&self) -> &'static str {
        self.model.as_str()
    }

    fn embed(&self, text: &str) -> Vec<f32> {
        let dim = self.model.dimensions();
        if text.trim().is_empty() {
            return vec![0.0; dim];
        }
        // One text per call keeps the batch shape constant (see module docs).
        let mut guard = self.inner.lock().unwrap_or_else(|p| p.into_inner());
        match guard.embed(vec![text], None) {
            Ok(mut vecs) if vecs.len() == 1 && vecs[0].len() == dim => {
                vecs.pop().unwrap_or_default()
            }
            Ok(vecs) => {
                tracing::error!(
                    count = vecs.len(),
                    "ONNX embedder returned unexpected shape"
                );
                vec![0.0; dim]
            }
            Err(e) => {
                tracing::error!(error = %e, "ONNX embedding failed; yielding zero vector");
                vec![0.0; dim]
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    // Model download happens once into the crate-local cache; these tests need
    // network on first run and are therefore feature-gated (`--features onnx`).
    fn embedder() -> OnnxEmbedder {
        OnnxEmbedder::with_cache_dir(
            OnnxModel::AllMiniLmL6V2Q,
            std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../.fastembed_cache"),
        )
        .expect("load ONNX model")
    }

    #[test]
    fn metadata_and_shapes() {
        let e = embedder();
        assert_eq!(e.dimensions(), 384);
        assert_eq!(e.name(), "onnx:all-minilm-l6-v2-q");
        let v = e.embed("deploy the payments service on friday");
        assert_eq!(v.len(), 384);
        let norm: f32 = v.iter().map(|x| x * x).sum::<f32>().sqrt();
        assert!(
            (norm - 1.0).abs() < 1e-4,
            "L2 norm should be ~1, got {norm}"
        );
    }

    #[test]
    fn empty_text_is_zero_vector_without_inference() {
        let e = embedder();
        assert_eq!(e.embed(""), vec![0.0f32; 384]);
        assert_eq!(e.embed("   "), vec![0.0f32; 384]);
    }

    #[test]
    fn semantic_paraphrase_beats_unrelated_and_is_deterministic() {
        let e = embedder();
        let base = e.embed("deploy the payments service on friday");
        let paraphrase = e.embed("ship the payments service at the end of the week");
        let unrelated = e.embed("the cat sat on the mat");
        let dot = |a: &[f32], b: &[f32]| -> f64 {
            a.iter()
                .zip(b)
                .map(|(x, y)| f64::from(*x) * f64::from(*y))
                .sum()
        };
        assert!(
            dot(&base, &paraphrase) > dot(&base, &unrelated),
            "semantic ordering must hold"
        );
        // Same-shape determinism: identical text, fresh call, identical vector.
        let repeat = e.embed("deploy the payments service on friday");
        assert_eq!(base, repeat);
    }
}
