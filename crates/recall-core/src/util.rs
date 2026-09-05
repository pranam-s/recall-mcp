//! Tokenization, hashing and vector math shared by the embedder and the store.

/// FNV-1a 64-bit hash of a byte slice — deterministic across platforms.
pub fn fnv1a64(bytes: &[u8]) -> u64 {
    let mut hash: u64 = 0xcbf2_9ce4_8422_2325;
    for b in bytes {
        hash ^= u64::from(*b);
        hash = hash.wrapping_mul(0x0000_0100_0000_01b3);
    }
    hash
}

/// Lowercase alphanumeric tokens, order preserved, duplicates kept
/// (later weighted by feature hashing anyway).
pub fn tokenize(text: &str) -> Vec<String> {
    text.split(|c: char| !c.is_alphanumeric())
        .filter(|t| !t.is_empty())
        .map(|t| t.to_lowercase())
        .collect()
}

/// Cosine similarity in `[-1, 1]`. Zero-length or mismatched vectors yield 0.0.
pub fn cosine(a: &[f32], b: &[f32]) -> f64 {
    if a.is_empty() || a.len() != b.len() {
        return 0.0;
    }
    let mut dot = 0.0f64;
    let mut norm_a = 0.0f64;
    let mut norm_b = 0.0f64;
    for (x, y) in a.iter().zip(b.iter()) {
        let (x, y) = (f64::from(*x), f64::from(*y));
        dot += x * y;
        norm_a += x * x;
        norm_b += y * y;
    }
    if norm_a == 0.0 || norm_b == 0.0 {
        return 0.0;
    }
    (dot / (norm_a.sqrt() * norm_b.sqrt())).clamp(-1.0, 1.0)
}

/// Encode an f32 vector as little-endian bytes for SQLite BLOB storage.
pub fn f32_to_blob(vec: &[f32]) -> Vec<u8> {
    vec.iter().flat_map(|f| f.to_le_bytes()).collect()
}

/// Decode a BLOB back into f32s. `None` when the byte length is not a multiple of 4.
pub fn blob_to_f32(bytes: &[u8]) -> Option<Vec<f32>> {
    if bytes.len() % 4 != 0 {
        return None;
    }
    Some(
        bytes
            .chunks_exact(4)
            .map(|c| f32::from_le_bytes([c[0], c[1], c[2], c[3]]))
            .collect(),
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn fnv1a64_is_deterministic_and_separated() {
        let a = fnv1a64(b"rust");
        assert_eq!(a, fnv1a64(b"rust"));
        assert_ne!(a, fnv1a64(b"rest"));
        // Known FNV-1a vector: "" -> offset basis.
        assert_eq!(fnv1a64(b""), 0xcbf29ce484222325);
    }

    #[test]
    fn tokenize_lowercases_and_drops_punctuation() {
        assert_eq!(
            tokenize("Hello, World! rust-tool"),
            vec!["hello", "world", "rust", "tool"]
        );
        assert!(tokenize("  ,, ").is_empty());
        assert!(tokenize("").is_empty());
        assert_eq!(tokenize("Ünïcode_Ω"), vec!["ünïcode", "ω"]);
    }

    #[test]
    fn cosine_handles_edge_shapes() {
        assert_eq!(cosine(&[], &[]), 0.0);
        assert_eq!(cosine(&[1.0], &[1.0, 2.0]), 0.0);
        assert_eq!(cosine(&[0.0, 0.0], &[1.0, 1.0]), 0.0);
        assert!((cosine(&[1.0, 0.0], &[1.0, 0.0]) - 1.0).abs() < 1e-9);
        assert!((cosine(&[1.0, 0.0], &[0.0, 2.0])).abs() < 1e-9);
        assert!((cosine(&[1.0, 0.0], &[-1.0, 0.0]) + 1.0).abs() < 1e-9);
    }

    #[test]
    fn blob_roundtrip() {
        let v = vec![0.25f32, -1.5, 3.0e10, f32::MIN];
        let blob = f32_to_blob(&v);
        assert_eq!(blob.len(), 16);
        assert_eq!(blob_to_f32(&blob).unwrap(), v);
        assert_eq!(blob_to_f32(&[1, 2, 3]), None);
        assert_eq!(blob_to_f32(&[]).unwrap(), Vec::<f32>::new());
    }
}
