//! Stratum job -> block header construction.

use anyhow::{anyhow, bail, Result};
use serde_json::Value;
use sha2::{Digest, Sha256};

pub fn sha256d(data: &[u8]) -> [u8; 32] {
    Sha256::digest(Sha256::digest(data)).into()
}

pub struct Job {
    pub job_id: String,
    /// Previous block hash in header byte order.
    pub prevhash: [u8; 32],
    pub coinb1: Vec<u8>,
    pub coinb2: Vec<u8>,
    pub merkle_branch: Vec<[u8; 32]>,
    pub version: u32,
    pub nbits: u32,
    pub ntime: u32,
    pub extranonce1: Vec<u8>,
    pub extranonce2_size: usize,
}

fn s(v: &Value) -> Result<&str> {
    v.as_str().ok_or_else(|| anyhow!("expected string in notify"))
}

fn hex_u32(v: &Value) -> Result<u32> {
    Ok(u32::from_str_radix(s(v)?, 16)?)
}

fn hex32(v: &Value) -> Result<[u8; 32]> {
    let b = hex::decode(s(v)?)?;
    b.try_into().map_err(|_| anyhow!("expected 32-byte hash"))
}

impl Job {
    pub fn from_notify(p: &[Value], extranonce1: Vec<u8>, extranonce2_size: usize) -> Result<Job> {
        if p.len() < 9 {
            bail!("short mining.notify");
        }
        let mut prevhash = hex32(&p[1])?;
        // Stratum sends the hash as 8 words, each byte-swapped.
        for c in prevhash.chunks_mut(4) {
            c.reverse();
        }
        let merkle_branch = p[4]
            .as_array()
            .ok_or_else(|| anyhow!("bad merkle branch"))?
            .iter()
            .map(hex32)
            .collect::<Result<Vec<_>>>()?;
        Ok(Job {
            job_id: s(&p[0])?.to_string(),
            prevhash,
            coinb1: hex::decode(s(&p[2])?)?,
            coinb2: hex::decode(s(&p[3])?)?,
            merkle_branch,
            version: hex_u32(&p[5])?,
            nbits: hex_u32(&p[6])?,
            ntime: hex_u32(&p[7])?,
            extranonce1,
            extranonce2_size,
        })
    }

    /// First 76 bytes of the header for a given extranonce2.
    pub fn header_prefix(&self, en2: &[u8]) -> [u8; 76] {
        let mut coinbase = Vec::with_capacity(self.coinb1.len() + self.coinb2.len() + 16);
        coinbase.extend_from_slice(&self.coinb1);
        coinbase.extend_from_slice(&self.extranonce1);
        coinbase.extend_from_slice(en2);
        coinbase.extend_from_slice(&self.coinb2);
        let mut root = sha256d(&coinbase);
        for b in &self.merkle_branch {
            let mut cat = [0u8; 64];
            cat[..32].copy_from_slice(&root);
            cat[32..].copy_from_slice(b);
            root = sha256d(&cat);
        }
        let mut h = [0u8; 76];
        h[0..4].copy_from_slice(&self.version.to_le_bytes());
        h[4..36].copy_from_slice(&self.prevhash);
        h[36..68].copy_from_slice(&root);
        h[68..72].copy_from_slice(&self.ntime.to_le_bytes());
        h[72..76].copy_from_slice(&self.nbits.to_le_bytes());
        h
    }
}

pub fn en2_bytes(v: u64, size: usize) -> Vec<u8> {
    let mut b = v.to_le_bytes().to_vec();
    b.resize(size, 0);
    b
}
