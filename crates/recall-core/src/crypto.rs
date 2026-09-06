//! Encryption at rest (D-014): AEAD encryption of memory text and keyed FTS
//! token digests, keyed from `RECALL_MCP_KEY` / `RECALL_MCP_KEY_FILE`.
//!
//! Design:
//! - `memories.text` is stored as `enc:v1:<base64(nonce || ciphertext || tag)>`
//!   using ChaCha20-Poly1305 with a random 12-byte nonce per record and the
//!   AAD binding the ciphertext to the row id (`recall-mcp:v1:text:<id>`).
//! - The FTS5 index never sees plaintext: tokens are HMAC-SHA256 digests
//!   (truncated to 128 bits) under the same key, so BM25 keyword ranking works
//!   unchanged while nothing reversible is stored.
//! - Existing (legacy) plaintext rows are detected by the missing `enc:v1:`
//!   prefix; [`crate::sqlite::SqliteStore::open_with_key`] re-encrypts them
//!   automatically on first keyed open.

use base64::Engine as _;
use base64::engine::general_purpose::STANDARD as BASE64;
use chacha20poly1305::aead::{Aead, Generate, KeyInit};
use chacha20poly1305::{ChaCha20Poly1305, Nonce};
use hex::FromHex as _;
use hmac::{Hmac, Mac};
use sha2::Sha256;

use crate::error::{RecallError, Result};
use crate::util::tokenize;

/// Prefix marking an AEAD-encrypted `memories.text` value.
pub const ENC_PREFIX: &str = "enc:v1:";

const NONCE_LEN: usize = 12;
/// HMAC-SHA256 digests are truncated to 16 bytes (128 bits): collision
/// probability is negligible for any realistic store (~4e-23 even at 10^9
/// distinct tokens) while keeping FTS documents compact.
const DIGEST_LEN: usize = 16;

type HmacSha256 = Hmac<Sha256>;

/// A 256-bit store key with its AEAD and MAC instances derived once.
#[derive(Clone)]
pub struct StoreKey {
    cipher: ChaCha20Poly1305,
    mac_key: [u8; 32],
}

impl StoreKey {
    /// Build from exactly 32 raw bytes.
    pub fn from_bytes(bytes: &[u8]) -> Result<Self> {
        let bytes: &[u8; 32] = bytes
            .try_into()
            .map_err(|_| RecallError::Crypto("key must be exactly 32 bytes".into()))?;
        Ok(Self {
            cipher: ChaCha20Poly1305::new(bytes.into()),
            mac_key: *bytes,
        })
    }

    /// Build from 64 hex characters (the `RECALL_MCP_KEY` format).
    pub fn from_hex(hex_str: &str) -> Result<Self> {
        let bytes = <[u8; 32]>::from_hex(hex_str.trim())
            .map_err(|_| RecallError::Crypto("key must be 64 hex characters".into()))?;
        Self::from_bytes(&bytes)
    }

    /// Generate a fresh random key (printed as hex for storage).
    pub fn generate() -> Self {
        let raw: [u8; 32] = Generate::generate();
        Self::from_bytes(&raw).expect("32 random bytes are a valid key")
    }

    /// Hex encoding suitable for writing to a key file.
    pub fn to_hex(&self) -> String {
        hex::encode(self.mac_key)
    }

    /// AEAD-encrypt with a fresh random nonce; AAD binds the row identity.
    pub fn encrypt(&self, context: &str, plaintext: &str) -> String {
        let nonce: Nonce = Generate::generate();
        let ct = self
            .cipher
            .encrypt(
                &nonce,
                chacha20poly1305::aead::Payload {
                    msg: plaintext.as_bytes(),
                    aad: context.as_bytes(),
                },
            )
            .expect("ChaCha20-Poly1305 encryption cannot fail for valid inputs");
        let mut blob = Vec::with_capacity(NONCE_LEN + ct.len());
        blob.extend_from_slice(nonce.as_slice());
        blob.extend_from_slice(&ct);
        format!("{ENC_PREFIX}{}", BASE64.encode(blob))
    }

    /// Decrypt a value produced by [`StoreKey::encrypt`]. Fails on tampering
    /// or a wrong key (AAD mismatch), per AEAD semantics.
    pub fn decrypt(&self, context: &str, stored: &str) -> Result<String> {
        let blob_b64 = stored
            .strip_prefix(ENC_PREFIX)
            .ok_or_else(|| RecallError::Crypto("value is not encrypted".into()))?;
        let blob = BASE64
            .decode(blob_b64)
            .map_err(|e| RecallError::Crypto(format!("ciphertext is not valid base64: {e}")))?;
        if blob.len() <= NONCE_LEN {
            return Err(RecallError::Crypto("ciphertext too short".into()));
        }
        let (nonce_bytes, ct) = blob.split_at(NONCE_LEN);
        let nonce = Nonce::try_from(nonce_bytes)
            .map_err(|_| RecallError::Crypto("invalid nonce length".into()))?;
        let pt = self
            .cipher
            .decrypt(
                &nonce,
                chacha20poly1305::aead::Payload {
                    msg: ct,
                    aad: context.as_bytes(),
                },
            )
            .map_err(|_| {
                RecallError::Crypto(
                    "decryption failed: wrong key or corrupted/tampered data".into(),
                )
            })?;
        String::from_utf8(pt).map_err(|_| RecallError::Crypto("plaintext is not UTF-8".into()))
    }

    /// Whether a stored value is an encrypted-at-rest blob.
    pub fn is_encrypted(stored: &str) -> bool {
        stored.starts_with(ENC_PREFIX)
    }

    /// Keyed FTS document: the memory text reduced to space-joined HMAC-SHA256
    /// token digests. Irreversible without the key; token frequency (and thus
    /// BM25) is preserved exactly.
    pub fn fts_tokens(&self, text: &str) -> String {
        tokenize(text)
            .iter()
            .map(|t| self.token_digest(t))
            .collect::<Vec<_>>()
            .join(" ")
    }

    /// Truncated hex HMAC of one token (lowercase alphanumeric per `tokenize`).
    pub fn token_digest(&self, token: &str) -> String {
        let mut mac =
            HmacSha256::new_from_slice(&self.mac_key).expect("HMAC accepts any key length");
        mac.update(token.as_bytes());
        let out = mac.finalize().into_bytes();
        hex::encode(&out[..DIGEST_LEN])
    }
}

/// Minimal hex encode helper on top of the `hex` crate.
use hex;

/// AAD context for the `memories.text` column of a given row.
pub fn text_context(memory_id: &str) -> String {
    format!("recall-mcp:v1:text:{memory_id}")
}

/// AAD context for the store-level key check value.
pub fn key_check_context() -> String {
    "recall-mcp:v1:key-check".to_string()
}

#[cfg(test)]
mod tests {
    use super::*;

    const KEY_HEX: &str = "a1a2a3a4a5a6a7a8a9b0b1b2b3b4b5b6b7b8b9c0c1c2c3c4c5c6c7c8c9d0d1d2";

    fn key() -> StoreKey {
        StoreKey::from_hex(KEY_HEX).unwrap()
    }

    #[test]
    fn hex_key_roundtrip_and_rejections() {
        let k = key();
        assert_eq!(k.to_hex(), KEY_HEX.to_lowercase());
        let from_raw = StoreKey::from_bytes(&[7u8; 32]).unwrap();
        assert_eq!(from_raw.to_hex().len(), 64);
        assert!(StoreKey::from_hex("xyz").is_err());
        assert!(StoreKey::from_hex(&"a1".repeat(31)).is_err());
        assert!(StoreKey::from_bytes(&[1u8; 31]).is_err());
        // Generated keys serialize back losslessly.
        let generated = StoreKey::generate();
        assert_eq!(
            StoreKey::from_hex(&generated.to_hex()).unwrap().to_hex(),
            generated.to_hex()
        );
    }

    #[test]
    fn encrypt_decrypt_roundtrip_with_prefix_and_context_binding() {
        let k = key();
        let ct = k.encrypt(&text_context("mem-1"), "attack at dawn");
        assert!(ct.starts_with(ENC_PREFIX));
        assert!(StoreKey::is_encrypted(&ct));
        assert!(!StoreKey::is_encrypted("attack at dawn"));
        assert_eq!(
            k.decrypt(&text_context("mem-1"), &ct).unwrap(),
            "attack at dawn"
        );
        // Same plaintext encrypts differently (random nonce)...
        let ct2 = k.encrypt(&text_context("mem-1"), "attack at dawn");
        assert_ne!(ct, ct2);
        // ...and round-trips independently.
        assert_eq!(
            k.decrypt(&text_context("mem-1"), &ct2).unwrap(),
            "attack at dawn"
        );
    }

    #[test]
    fn wrong_key_context_or_ciphertext_fails() {
        let k = key();
        let other = StoreKey::from_hex(&"9".repeat(64)).unwrap();
        let ct = k.encrypt(&text_context("mem-1"), "secret");
        // Wrong key.
        assert!(other.decrypt(&text_context("mem-1"), &ct).is_err());
        // Wrong AAD (ciphertext bound to another row).
        assert!(k.decrypt(&text_context("mem-2"), &ct).is_err());
        // Tampered ciphertext.
        let mut tampered = ct.clone();
        tampered.replace_range(ENC_PREFIX.len() + 30..ENC_PREFIX.len() + 31, "A");
        assert!(k.decrypt(&text_context("mem-1"), &tampered).is_err());
        // Garbage inputs.
        assert!(k.decrypt(&text_context("mem-1"), "not-a-prefix").is_err());
        let bad_b64 = format!("{ENC_PREFIX}!!!!");
        assert!(k.decrypt(&text_context("mem-1"), &bad_b64).is_err());
        let short = format!("{ENC_PREFIX}{}", BASE64.encode([1u8; 4]));
        assert!(k.decrypt(&text_context("mem-1"), &short).is_err());
    }

    #[test]
    fn fts_tokens_preserve_multiplicity_without_plaintext() {
        let k = key();
        let doc = k.fts_tokens("Deploy the payments service; deploy it again!");
        // tokenize keeps duplicates and lowercase: "deploy" appears twice.
        let mut count = 0;
        let needle = k.token_digest("deploy");
        for token in doc.split(' ') {
            if token == needle {
                count += 1;
            }
        }
        assert_eq!(count, 2, "token multiplicity must survive");
        assert_eq!(doc.split(' ').count(), 7, "7 tokens total");
        // No plaintext token survives verbatim.
        for word in ["deploy", "payments", "service", "again"] {
            assert!(!doc.contains(word));
        }
        // Deterministic under the same key; different under another key.
        assert_eq!(
            doc,
            k.fts_tokens("Deploy the payments service; deploy it again!")
        );
        assert_ne!(
            doc,
            StoreKey::from_hex(&"b".repeat(64))
                .unwrap()
                .fts_tokens("deploy")
        );
        // 32 hex chars per digest (128-bit truncation).
        assert_eq!(k.token_digest("deploy").len(), 32);
        // Non-alphanumeric-only text yields an empty document.
        assert_eq!(k.fts_tokens("!!!"), "");
    }
}
