use anyhow::{Context, Result};
use serde::Serialize;
use sha2::{Digest, Sha256};

use super::super::bundle::{FieldFingerprint, MAX_PROJECTION_TEXT_BYTES};

const DIGEST_PREFIX: &str = "sha256:";

pub(crate) struct StableDigest(Sha256);

impl StableDigest {
    pub(crate) fn new(domain: &str) -> Self {
        let mut hasher = Sha256::new();
        update_length_prefixed(&mut hasher, domain.as_bytes());
        Self(hasher)
    }

    pub(crate) fn bytes(&mut self, value: &[u8]) {
        update_length_prefixed(&mut self.0, value);
    }

    pub(crate) fn text(&mut self, value: &str) {
        self.bytes(value.as_bytes());
    }

    pub(crate) fn usize(&mut self, value: usize) {
        // Fixed-width encoding keeps digests identical on 32-bit and 64-bit hosts.
        self.bytes(&(value as u64).to_le_bytes());
    }

    pub(crate) fn finish(self) -> String {
        format!("{DIGEST_PREFIX}{:x}", self.0.finalize())
    }

    pub(crate) fn finish_bytes(self) -> [u8; 32] {
        self.0.finalize().into()
    }
}

fn update_length_prefixed(hasher: &mut Sha256, value: &[u8]) {
    hasher.update((value.len() as u64).to_le_bytes());
    hasher.update(value);
}

#[derive(Default)]
pub(crate) struct MultisetDigest {
    count: u64,
    xor: [u8; 32],
    sums: [u64; 4],
}

impl MultisetDigest {
    pub(crate) fn add(&mut self, value: [u8; 32]) {
        self.count = self.count.wrapping_add(1);
        for (target, byte) in self.xor.iter_mut().zip(value) {
            *target ^= byte;
        }
        let (lanes, remainder) = value.as_chunks::<8>();
        debug_assert!(remainder.is_empty());
        for (index, lane) in lanes.iter().enumerate() {
            self.sums[index] = self.sums[index].wrapping_add(u64::from_le_bytes(*lane));
        }
    }

    pub(crate) fn finish(self, domain: &str) -> String {
        let mut digest = StableDigest::new(domain);
        digest.bytes(&self.count.to_le_bytes());
        digest.bytes(&self.xor);
        for sum in self.sums {
            digest.bytes(&sum.to_le_bytes());
        }
        digest.finish()
    }
}

pub(super) fn fingerprint(bytes: &[u8]) -> FieldFingerprint {
    FieldFingerprint {
        bytes: bytes.len(),
        digest: sha256_bytes(bytes),
    }
}

pub(super) fn sha256_json<T: Serialize>(value: &T) -> Result<String> {
    Ok(sha256_bytes(
        &serde_json::to_vec(value).context("serialize deterministic portrait projection")?,
    ))
}

pub(crate) fn sha256_bytes(bytes: &[u8]) -> String {
    format!("{DIGEST_PREFIX}{:x}", Sha256::digest(bytes))
}

pub(crate) fn truncate_projection_text(value: &str) -> String {
    if value.len() <= MAX_PROJECTION_TEXT_BYTES {
        return value.to_string();
    }
    let mut end = MAX_PROJECTION_TEXT_BYTES;
    while !value.is_char_boundary(end) {
        end -= 1;
    }
    value[..end].to_string()
}

pub(super) fn valid_digest(value: &str) -> bool {
    value.len() == DIGEST_PREFIX.len() + 64
        && value.starts_with(DIGEST_PREFIX)
        && value[DIGEST_PREFIX.len()..]
            .bytes()
            .all(|byte| byte.is_ascii_hexdigit() && !byte.is_ascii_uppercase())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn utf8_truncation_respects_boundaries() {
        let value = "你".repeat(200);
        let truncated = truncate_projection_text(&value);
        assert!(truncated.len() <= MAX_PROJECTION_TEXT_BYTES);
        assert!(value.starts_with(&truncated));
    }

    #[test]
    fn digest_validation_requires_canonical_lowercase_sha256() {
        let valid = sha256_bytes(b"portrait");
        assert!(valid_digest(&valid));
        assert!(!valid_digest(&valid.to_uppercase()));
        assert!(!valid_digest("sha256:abc"));
    }

    #[test]
    fn stable_digest_uses_fixed_width_lengths_and_multiset_is_order_independent() {
        let mut first = StableDigest::new("row");
        first.text("a");
        let mut second = StableDigest::new("row");
        second.text("b");
        let mut forward = MultisetDigest::default();
        forward.add(first.finish_bytes());
        forward.add(second.finish_bytes());
        let mut reverse = MultisetDigest::default();
        let mut second = StableDigest::new("row");
        second.text("b");
        reverse.add(second.finish_bytes());
        let mut first = StableDigest::new("row");
        first.text("a");
        reverse.add(first.finish_bytes());
        assert_eq!(forward.finish("set"), reverse.finish("set"));
    }
}
