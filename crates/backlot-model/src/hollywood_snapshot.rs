//! What a card carries a copy of: the files it references, their
//! content hashed at propose time and copied into the card's own
//! directory under Hollywood's ledger. Apply reads the copy, so the
//! card executes what the person reviewed; a source file that moved
//! since makes the card stale, like the other base checks.

use serde::{Deserialize, Serialize};

/// One copied file: the path the proposer named (absolute, resolved at
/// propose time) and the content hash it had then.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct SnapshotFile {
    /// The proposer's own path — absolute, so the staleness check never
    /// depends on whoever reads the card later.
    pub source: String,
    /// SHA-256 of the source at propose time, lowercase hex.
    pub sha256: String,
}

impl SnapshotFile {
    pub fn new(source: impl Into<String>, sha256: impl Into<String>) -> Self {
        SnapshotFile {
            source: source.into(),
            sha256: sha256.into(),
        }
    }
}

/// The files a card copied into its own directory. Absent on logs
/// written before snapshots existed and on cards that reference no
/// file.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct CardSnapshot {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub cast: Option<SnapshotFile>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub brief: Option<SnapshotFile>,
}

impl CardSnapshot {
    /// Every referenced file as `(label, entry)`, in a stable order.
    pub fn entries(&self) -> impl Iterator<Item = (&'static str, &SnapshotFile)> {
        [("cast", self.cast.as_ref()), ("brief", self.brief.as_ref())]
            .into_iter()
            .filter_map(|(label, entry)| entry.map(|e| (label, e)))
    }

    /// `Some(reason)` for the first source file whose content no longer
    /// hashes to what the card recorded — `read` answers the source's
    /// current bytes, `None` when it is gone.
    pub fn stale(&self, read: impl Fn(&str) -> Option<Vec<u8>>) -> Option<String> {
        self.entries().find_map(|(label, entry)| {
            let now = read(&entry.source)?;
            let got = crate::hollywood_snapshot::sha256_hex(&now);
            (got != entry.sha256)
                .then(|| format!("the {label} file changed after this card was proposed"))
        })
    }
}

/// SHA-256 (FIPS 180-4) raw 32-byte digest.
pub fn sha256(data: &[u8]) -> [u8; 32] {
    const K: [u32; 64] = [
        0x428a2f98, 0x71374491, 0xb5c0fbcf, 0xe9b5dba5, 0x3956c25b, 0x59f111f1, 0x923f82a4,
        0xab1c5ed5, 0xd807aa98, 0x12835b01, 0x243185be, 0x550c7dc3, 0x72be5d74, 0x80deb1fe,
        0x9bdc06a7, 0xc19bf174, 0xe49b69c1, 0xefbe4786, 0x0fc19dc6, 0x240ca1cc, 0x2de92c6f,
        0x4a7484aa, 0x5cb0a9dc, 0x76f988da, 0x983e5152, 0xa831c66d, 0xb00327c8, 0xbf597fc7,
        0xc6e00bf3, 0xd5a79147, 0x06ca6351, 0x14292967, 0x27b70a85, 0x2e1b2138, 0x4d2c6dfc,
        0x53380d13, 0x650a7354, 0x766a0abb, 0x81c2c92e, 0x92722c85, 0xa2bfe8a1, 0xa81a664b,
        0xc24b8b70, 0xc76c51a3, 0xd192e819, 0xd6990624, 0xf40e3585, 0x106aa070, 0x19a4c116,
        0x1e376c08, 0x2748774c, 0x34b0bcb5, 0x391c0cb3, 0x4ed8aa4a, 0x5b9cca4f, 0x682e6ff3,
        0x748f82ee, 0x78a5636f, 0x84c87814, 0x8cc70208, 0x90befffa, 0xa4506ceb, 0xbef9a3f7,
        0xc67178f2,
    ];
    let mut h: [u32; 8] = [
        0x6a09e667, 0xbb67ae85, 0x3c6ef372, 0xa54ff53a, 0x510e527f, 0x9b05688c, 0x1f83d9ab,
        0x5be0cd19,
    ];
    let bit_len = (data.len() as u64).wrapping_mul(8);
    let mut msg = data.to_vec();
    msg.push(0x80);
    while msg.len() % 64 != 56 {
        msg.push(0);
    }
    msg.extend_from_slice(&bit_len.to_be_bytes());
    for chunk in msg.as_chunks::<64>().0 {
        let mut w = [0u32; 64];
        for (i, word) in chunk.as_chunks::<4>().0.iter().enumerate() {
            w[i] = u32::from_be_bytes(*word);
        }
        for i in 16..64 {
            let s0 = w[i - 15].rotate_right(7) ^ w[i - 15].rotate_right(18) ^ (w[i - 15] >> 3);
            let s1 = w[i - 2].rotate_right(17) ^ w[i - 2].rotate_right(19) ^ (w[i - 2] >> 10);
            w[i] = w[i - 16]
                .wrapping_add(s0)
                .wrapping_add(w[i - 7])
                .wrapping_add(s1);
        }
        let mut a = h;
        for i in 0..64 {
            let s1 = a[4].rotate_right(6) ^ a[4].rotate_right(11) ^ a[4].rotate_right(25);
            let ch = (a[4] & a[5]) ^ (!a[4] & a[6]);
            let t1 = a[7]
                .wrapping_add(s1)
                .wrapping_add(ch)
                .wrapping_add(K[i])
                .wrapping_add(w[i]);
            let s0 = a[0].rotate_right(2) ^ a[0].rotate_right(13) ^ a[0].rotate_right(22);
            let maj = (a[0] & a[1]) ^ (a[0] & a[2]) ^ (a[1] & a[2]);
            let t2 = s0.wrapping_add(maj);
            a[7] = a[6];
            a[6] = a[5];
            a[5] = a[4];
            a[4] = a[3].wrapping_add(t1);
            a[3] = a[2];
            a[2] = a[1];
            a[1] = a[0];
            a[0] = t1.wrapping_add(t2);
        }
        for (slot, val) in h.iter_mut().zip(a) {
            *slot = slot.wrapping_add(val);
        }
    }
    let mut out = [0u8; 32];
    for (dst, word) in out.as_chunks_mut::<4>().0.iter_mut().zip(h) {
        *dst = word.to_be_bytes();
    }
    out
}

/// SHA-256 (FIPS 180-4) as lowercase hex — the same digest the confirm
/// tokens use, kept here so the model can hash a snapshot itself.
pub fn sha256_hex(data: &[u8]) -> String {
    let mut out = String::with_capacity(64);
    for byte in sha256(data) {
        use std::fmt::Write as _;
        let _ = write!(out, "{byte:02x}");
    }
    out
}
