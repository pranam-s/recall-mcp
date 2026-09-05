//! Embedding abstraction: deterministic hash embedder by default, pluggable API
//! embedders behind features (ADR-003).

use crate::error::{RecallError, Result};
use crate::util::{fnv1a64, tokenize};

/// Turns text into a fixed-length dense vector.
pub trait Embedder: Send + Sync {
    fn dimensions(&self) -> usize;
    fn name(&self) -> &'static str;
    /// Embed `text`; never panics. Empty/whitespace text yields a zero vector.
    fn embed(&self, text: &str) -> Vec<f32>;
}

/// Feature-hashing embedder: deterministic, offline, no model download (ADR-003).
///
/// Each token is FNV-1a hashed to a bucket index and a sign; accumulated values
/// are L2-normalized. Same text always yields the same vector, on any platform.
#[derive(Debug, Clone)]
pub struct HashEmbedder {
    dim: usize,
}

impl HashEmbedder {
    /// Default 256-dimensional embedder.
    pub fn new_256() -> Self {
        Self { dim: 256 }
    }

    /// Custom dimensionality; must be non-zero.
    pub fn new(dim: usize) -> Result<Self> {
        if dim == 0 {
            return Err(RecallError::InvalidInput("embedding dimension must be > 0".into()));
        }
        Ok(Self { dim })
    }
}

impl Default for HashEmbedder {
    fn default() -> Self {
        Self::new_256()
    }
}

impl Embedder for HashEmbedder {
    fn dimensions(&self) -> usize {
        self.dim
    }

    fn name(&self) -> &'static str {
        "hash-256"
    }

    fn embed(&self, text: &str) -> Vec<f32> {
        let mut buckets = vec![0.0f32; self.dim];
        for token in tokenize(text) {
            let h = fnv1a64(token.as_bytes());
            let idx = (h % self.dim as u64) as usize;
            let sign = if h >> 63 & 1 == 1 { 1.0f32 } else { -1.0 };
            buckets[idx] += sign;
        }
        let norm = buckets.iter().map(|v| {
            let v = f64::from(*v);
            v * v
        }).sum::<f64>().sqrt();
        if norm == 0.0 {
            return buckets;
        }
        buckets.iter().map(|v| v / norm as f32).collect()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn deterministic_256_dims_by_default() {
        let e = HashEmbedder::default();
        assert_eq!(e.dimensions(), 256);
        assert_eq!(e.name(), "hash-256");
        let a = e.embed("deploy the service on friday");
        let b = e.embed("deploy the service on friday");
        assert_eq!(a.len(), 256);
        assert_eq!(a, b);
    }

    #[test]
    fn different_text_differs_but_stays_normalized() {
        let e = HashEmbedder::new_256();
        let a = e.embed("kubernetes rollout");
        let b = e.embed("database migration");
        assert_ne!(a, b);
        let norm: f64 = a.iter().map(|v| (*v as f64) * (*v as f64)).sum::<f64>().sqrt();
        assert!((norm - 1.0).abs() < 1e-5, "L2 norm should be 1, got {norm}");
        // Unit vectors: cosine of identical text is 1, and components are bounded.
        assert!(a.iter().all(|v| v.abs() <= 1.0));
    }

    #[test]
    fn empty_and_whitespace_are_zero_vectors() {
        let e = HashEmbedder::new_256();
        for text in ["", "   ", "!!!"] {
            let v = e.embed(text);
            assert!(v.iter().all(|f| *f == 0.0), "{text:?} should embed to zero");
        }
    }

    #[test]
    fn case_and_punctuation_invariant() {
        let e = HashEmbedder::new_256();
        assert_eq!(e.embed("Postgres WAL"), e.embed("postgres wal"));
    }

    #[test]
    fn custom_dim_and_zero_dim_error() {
        assert_eq!(HashEmbedder::new(64).expect("64 dims").dimensions(), 64);
        let err = HashEmbedder::new(0).unwrap_err();
        assert!(err.to_string().contains("dimension must be > 0"));
    }

    #[test]
    fn dim_one_still_works() {
        let e = HashEmbedder::new(1).expect("1 dim");
        assert_eq!(e.embed("hello").len(), 1);
    }
}
