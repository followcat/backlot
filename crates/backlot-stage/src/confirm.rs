//! Confirm tokens for destructive verbs over the request queue
//! (design §2.3): a dry run returns `sha256` of its report's canonical
//! JSON, and the commit must echo that exact token.

use backlot_model::req_results::{ERR_NEEDS_REVIEW, ERR_STALE_PLAN};
use serde::Serialize;

use crate::cli::Fail;

/// The token for a report: SHA-256 over its compact JSON. Struct field
/// order and `BTreeMap` ordering make the encoding canonical.
pub(crate) fn token<T: Serialize>(report: &T) -> Result<String, Fail> {
    let bytes =
        serde_json::to_vec(report).map_err(|e| Fail::refused(format!("report encode: {e}")))?;
    Ok(sha256_hex(&bytes))
}

/// Require the person's token: missing means no review, a mismatch
/// means the studio moved underneath what they saw.
pub(crate) fn verify<T: Serialize>(report: &T, given: Option<&str>) -> Result<(), Fail> {
    let Some(given) = given else {
        return Err(Fail::refused(ERR_NEEDS_REVIEW));
    };
    if given != token(report)? {
        return Err(Fail::refused(ERR_STALE_PLAN));
    }
    Ok(())
}

/// SHA-256 (FIPS 180-4) as lowercase hex — the model's one
/// implementation, shared so a card's snapshot hash and a confirm token
/// can never drift.
pub(crate) fn sha256_hex(data: &[u8]) -> String {
    backlot_model::hollywood::sha256_hex(data)
}

#[cfg(test)]
mod tests {
    use super::sha256_hex;

    #[test]
    fn sha256_matches_the_standard_vectors() {
        assert_eq!(
            sha256_hex(b""),
            "e3b0c44298fc1c149afbf4c8996fb92427ae41e4649b934ca495991b7852b855"
        );
        assert_eq!(
            sha256_hex(b"abc"),
            "ba7816bf8f01cfea414140de5dae2223b00361a396177a9cb410ff61f20015ad"
        );
        assert_eq!(
            sha256_hex(b"abcdbcdecdefdefgefghfghighijhijkijkljklmklmnlmnomnopnopq"),
            "248d6a61d20638b8e5c026930c3e6039a33ce45964ff2167f6ecedd419db06c1"
        );
    }
}
