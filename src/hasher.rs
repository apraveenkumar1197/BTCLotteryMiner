//! Midstate-optimised double SHA-256 over an 80-byte block header.
//!
//! The first 64 header bytes never change while scanning nonces, so they are
//! compressed once. Per nonce we run exactly two compressions (tail block of
//! hash #1 and the single block of hash #2). `sha2::compress256` uses the
//! CPU's SHA extensions when available.

use sha2::compress256;
use sha2::digest::generic_array::{typenum::U64, GenericArray};

pub const IV: [u32; 8] = [
    0x6a09e667, 0xbb67ae85, 0x3c6ef372, 0xa54ff53a, 0x510e527f, 0x9b05688c, 0x1f83d9ab, 0x5be0cd19,
];

type Block = [GenericArray<u8, U64>; 1];

pub struct Hasher {
    mid: [u32; 8],
    b2: Block,
    b3: Block,
}

impl Hasher {
    /// `prefix` is the first 76 header bytes (everything except the nonce).
    pub fn new(prefix: &[u8; 76]) -> Self {
        let mut first: Block = Default::default();
        first[0].copy_from_slice(&prefix[..64]);
        let mut mid = IV;
        compress256(&mut mid, &first);

        let mut b2: Block = Default::default();
        b2[0][..12].copy_from_slice(&prefix[64..76]);
        b2[0][16] = 0x80;
        b2[0][56..64].copy_from_slice(&(80u64 * 8).to_be_bytes());

        let mut b3: Block = Default::default();
        b3[0][32] = 0x80;
        b3[0][56..64].copy_from_slice(&(32u64 * 8).to_be_bytes());

        Hasher { mid, b2, b3 }
    }

    /// Returns the final hash as 8 big-endian words (internal byte order).
    #[inline(always)]
    pub fn hash(&mut self, nonce: u32) -> [u32; 8] {
        self.b2[0][12..16].copy_from_slice(&nonce.to_le_bytes());
        let mut s = self.mid;
        compress256(&mut s, &self.b2);
        for i in 0..8 {
            self.b3[0][i * 4..i * 4 + 4].copy_from_slice(&s[i].to_be_bytes());
        }
        let mut out = IV;
        compress256(&mut out, &self.b3);
        out
    }
}

pub fn words_to_bytes(w: &[u32; 8]) -> [u8; 32] {
    let mut b = [0u8; 32];
    for i in 0..8 {
        b[i * 4..i * 4 + 4].copy_from_slice(&w[i].to_be_bytes());
    }
    b
}

/// Difficulty-1 target = 0xFFFF * 2^208.
fn diff1() -> f64 {
    65535.0 * 2f64.powi(208)
}

/// Difficulty a hash achieves (bigger = rarer). The hash is a little-endian number.
pub fn hash_difficulty(w: &[u32; 8]) -> f64 {
    let b = words_to_bytes(w);
    let mut v = 0f64;
    for i in (0..32).rev() {
        v = v * 256.0 + b[i] as f64;
    }
    if v == 0.0 {
        f64::INFINITY
    } else {
        diff1() / v
    }
}

/// Network difficulty from the compact `nbits` field.
pub fn nbits_difficulty(nbits: u32) -> f64 {
    let exp = (nbits >> 24) as i32;
    let mant = (nbits & 0x007f_ffff) as f64;
    let target = mant * 2f64.powi(8 * (exp - 3));
    diff1() / target
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn genesis_block_hash() {
        let hdr = hex::decode(
            "0100000000000000000000000000000000000000000000000000000000000000000000003ba3edfd7a7b12b27ac72c3e67768f617fc81bc3888a51323a9fb8aa4b1e5e4a29ab5f49ffff001d1dac2b7c",
        )
        .unwrap();
        let mut prefix = [0u8; 76];
        prefix.copy_from_slice(&hdr[..76]);
        let nonce = u32::from_le_bytes(hdr[76..80].try_into().unwrap());
        let mut h = Hasher::new(&prefix);
        let mut b = words_to_bytes(&h.hash(nonce));
        b.reverse();
        assert_eq!(
            hex::encode(b),
            "000000000019d6689c085ae165831e934ff763ae46a2a6c172b3f1b60a8ce26f"
        );
    }

    #[test]
    fn genesis_difficulty_is_one() {
        assert!((nbits_difficulty(0x1d00ffff) - 1.0).abs() < 1e-9);
    }
}
