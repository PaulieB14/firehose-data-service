//! Per-block attestation parse + EIP-712 verify.
//!
//! Mirrors `mainline-service::grpc::server::encode_attestation` plus the
//! EIP-712 domain in `mainline-service::attestation::eip712`. Operators
//! splice the hex-encoded attestation onto the stream cursor via
//! `||mainline-att||`; unary `Fetch.Block` callers receive it in the
//! `x-mainline-attestation` metadata header.

use k256::ecdsa::{RecoveryId, Signature, VerifyingKey};
use sha3::{Digest, Keccak256};

/// Same sentinel as `mainline-service::grpc::server::CURSOR_ATTESTATION_DELIMITER`.
pub const CURSOR_ATTESTATION_DELIMITER: &str = "||mainline-att||";

/// Length of the wire-format attestation in bytes.
pub const PACKED_ATTESTATION_LEN: usize = 32 + 8 + 32 + 32 + 32 + 65;

#[derive(thiserror::Error, Debug)]
pub enum AttestationVerifyError {
    #[error("attestation hex is not parseable")]
    BadHex,
    #[error("attestation truncated (expected {expected} bytes, got {actual})")]
    Truncated { expected: usize, actual: usize },
    #[error("attestation signature did not recover")]
    BadSignature,
    #[error("attestation signer {recovered:?} does not match expected {expected:?}")]
    SignerMismatch {
        recovered: [u8; 20],
        expected: [u8; 20],
    },
    #[error("attestation payload_hash mismatch")]
    PayloadHashMismatch,
    #[error("cursor does not carry an attestation suffix")]
    NoAttestationSuffix,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MainlineAttestation {
    pub chain_id: [u8; 32],
    pub block_number: u64,
    pub block_hash: [u8; 32],
    pub state_root: [u8; 32],
    pub payload_hash: [u8; 32],
    pub signature: [u8; 65],
}

#[derive(Clone, Copy, Debug)]
pub struct AttestationDomain {
    pub settlement_chain_id: u64,
    pub verifying_contract: [u8; 20],
}

const EIP712_DOMAIN_TYPEHASH: &[u8] =
    b"EIP712Domain(string name,string version,uint256 chainId,address verifyingContract)";
const MAINLINE_ATTESTATION_TYPEHASH: &[u8] =
    b"MainlineAttestation(bytes32 chainId,uint64 blockNumber,bytes32 blockHash,bytes32 stateRoot,bytes32 payloadHash)";
const DOMAIN_NAME: &[u8] = b"Mainline";
const DOMAIN_VERSION: &[u8] = b"1";

fn keccak256(b: &[u8]) -> [u8; 32] {
    let mut h = Keccak256::new();
    h.update(b);
    h.finalize().into()
}

/// Parse the wire-format attestation. See `mainline-service::grpc::server::encode_attestation`.
pub fn parse_packed(bytes: &[u8]) -> Result<MainlineAttestation, AttestationVerifyError> {
    if bytes.len() < PACKED_ATTESTATION_LEN {
        return Err(AttestationVerifyError::Truncated {
            expected: PACKED_ATTESTATION_LEN,
            actual: bytes.len(),
        });
    }
    let mut p = 0usize;
    let mut chain_id = [0u8; 32];
    chain_id.copy_from_slice(&bytes[p..p + 32]);
    p += 32;
    let mut bn = [0u8; 8];
    bn.copy_from_slice(&bytes[p..p + 8]);
    p += 8;
    let mut block_hash = [0u8; 32];
    block_hash.copy_from_slice(&bytes[p..p + 32]);
    p += 32;
    let mut state_root = [0u8; 32];
    state_root.copy_from_slice(&bytes[p..p + 32]);
    p += 32;
    let mut payload_hash = [0u8; 32];
    payload_hash.copy_from_slice(&bytes[p..p + 32]);
    p += 32;
    let mut signature = [0u8; 65];
    signature.copy_from_slice(&bytes[p..p + 65]);
    Ok(MainlineAttestation {
        chain_id,
        block_number: u64::from_be_bytes(bn),
        block_hash,
        state_root,
        payload_hash,
        signature,
    })
}

pub fn parse_hex(s: &str) -> Result<MainlineAttestation, AttestationVerifyError> {
    let bytes = hex::decode(s).map_err(|_| AttestationVerifyError::BadHex)?;
    parse_packed(&bytes)
}

/// Strip the `||mainline-att||<hex>` suffix from a Stream.Blocks cursor and
/// return the inner cursor + parsed attestation.
pub fn split_cursor(cursor: &str) -> Result<(String, MainlineAttestation), AttestationVerifyError> {
    let idx = cursor
        .rfind(CURSOR_ATTESTATION_DELIMITER)
        .ok_or(AttestationVerifyError::NoAttestationSuffix)?;
    let inner = &cursor[..idx];
    let hex_part = &cursor[idx + CURSOR_ATTESTATION_DELIMITER.len()..];
    let att = parse_hex(hex_part)?;
    Ok((inner.to_string(), att))
}

fn domain_separator(d: &AttestationDomain) -> [u8; 32] {
    let mut buf = Vec::with_capacity(32 * 5);
    buf.extend_from_slice(&keccak256(EIP712_DOMAIN_TYPEHASH));
    buf.extend_from_slice(&keccak256(DOMAIN_NAME));
    buf.extend_from_slice(&keccak256(DOMAIN_VERSION));
    let mut chain = [0u8; 32];
    chain[24..].copy_from_slice(&d.settlement_chain_id.to_be_bytes());
    buf.extend_from_slice(&chain);
    let mut addr = [0u8; 32];
    addr[12..].copy_from_slice(&d.verifying_contract);
    buf.extend_from_slice(&addr);
    keccak256(&buf)
}

fn struct_hash(a: &MainlineAttestation) -> [u8; 32] {
    let mut buf = Vec::with_capacity(32 * 6);
    buf.extend_from_slice(&keccak256(MAINLINE_ATTESTATION_TYPEHASH));
    buf.extend_from_slice(&a.chain_id);
    let mut bn = [0u8; 32];
    bn[24..].copy_from_slice(&a.block_number.to_be_bytes());
    buf.extend_from_slice(&bn);
    buf.extend_from_slice(&a.block_hash);
    buf.extend_from_slice(&a.state_root);
    buf.extend_from_slice(&a.payload_hash);
    keccak256(&buf)
}

fn digest(d: &AttestationDomain, a: &MainlineAttestation) -> [u8; 32] {
    let mut buf = Vec::with_capacity(2 + 32 + 32);
    buf.extend_from_slice(&[0x19, 0x01]);
    buf.extend_from_slice(&domain_separator(d));
    buf.extend_from_slice(&struct_hash(a));
    keccak256(&buf)
}

/// Verify an attestation against the expected operator signing address.
///
/// `payload_hash_expected` is the consumer-side recomputation
/// (`sha256(any.value)` for the corresponding firehose Response). When it
/// matches the attestation's `payload_hash` AND the recovered signer matches
/// `expected_signer`, the block is verified.
pub fn verify_attestation(
    domain: &AttestationDomain,
    attestation: &MainlineAttestation,
    expected_signer: &[u8; 20],
    payload_hash_expected: Option<&[u8; 32]>,
) -> Result<(), AttestationVerifyError> {
    if let Some(expected) = payload_hash_expected {
        if expected != &attestation.payload_hash {
            return Err(AttestationVerifyError::PayloadHashMismatch);
        }
    }
    let prehash = digest(domain, attestation);

    let raw_v = attestation.signature[64];
    let rec_byte = if raw_v >= 27 { raw_v - 27 } else { raw_v };
    let rec_id =
        RecoveryId::try_from(rec_byte).map_err(|_| AttestationVerifyError::BadSignature)?;
    let sig = Signature::from_slice(&attestation.signature[..64])
        .map_err(|_| AttestationVerifyError::BadSignature)?;
    let vk = VerifyingKey::recover_from_prehash(&prehash, &sig, rec_id)
        .map_err(|_| AttestationVerifyError::BadSignature)?;
    let pt = vk.to_encoded_point(false);
    let h = keccak256(&pt.as_bytes()[1..]);
    let mut recovered = [0u8; 20];
    recovered.copy_from_slice(&h[12..]);
    if recovered != *expected_signer {
        return Err(AttestationVerifyError::SignerMismatch {
            recovered,
            expected: *expected_signer,
        });
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use k256::ecdsa::signature::hazmat::PrehashSigner;
    use k256::ecdsa::SigningKey;

    fn key_to_address(key: &SigningKey) -> [u8; 20] {
        let pt = key.verifying_key().to_encoded_point(false);
        let h = keccak256(&pt.as_bytes()[1..]);
        let mut out = [0u8; 20];
        out.copy_from_slice(&h[12..]);
        out
    }

    fn signed_attestation(key: &SigningKey, payload_hash: [u8; 32]) -> MainlineAttestation {
        let domain = AttestationDomain {
            settlement_chain_id: 42161,
            verifying_contract: [0xab; 20],
        };
        let mut a = MainlineAttestation {
            chain_id: [1u8; 32],
            block_number: 100,
            block_hash: [2u8; 32],
            state_root: [3u8; 32],
            payload_hash,
            signature: [0u8; 65],
        };
        let pre = digest(&domain, &a);
        let (sig, rec): (Signature, RecoveryId) = key.sign_prehash(&pre).unwrap();
        let mut full = [0u8; 65];
        full[..64].copy_from_slice(&sig.to_bytes());
        full[64] = rec.to_byte() + 27;
        a.signature = full;
        a
    }

    #[test]
    fn roundtrip_packed() {
        let key = SigningKey::from_bytes(&[0x77; 32].into()).unwrap();
        let a = signed_attestation(&key, [0xdd; 32]);
        let bytes = {
            let mut out = Vec::new();
            out.extend_from_slice(&a.chain_id);
            out.extend_from_slice(&a.block_number.to_be_bytes());
            out.extend_from_slice(&a.block_hash);
            out.extend_from_slice(&a.state_root);
            out.extend_from_slice(&a.payload_hash);
            out.extend_from_slice(&a.signature);
            out
        };
        let parsed = parse_packed(&bytes).unwrap();
        assert_eq!(parsed, a);
    }

    #[test]
    fn verify_happy_path() {
        let key = SigningKey::from_bytes(&[0x77; 32].into()).unwrap();
        let payload_hash = [0xdd; 32];
        let a = signed_attestation(&key, payload_hash);
        let signer = key_to_address(&key);
        let domain = AttestationDomain {
            settlement_chain_id: 42161,
            verifying_contract: [0xab; 20],
        };
        verify_attestation(&domain, &a, &signer, Some(&payload_hash)).expect("verify");
    }

    #[test]
    fn verify_rejects_wrong_signer() {
        let key = SigningKey::from_bytes(&[0x77; 32].into()).unwrap();
        let a = signed_attestation(&key, [0xdd; 32]);
        let domain = AttestationDomain {
            settlement_chain_id: 42161,
            verifying_contract: [0xab; 20],
        };
        let other = [0xff; 20];
        let result = verify_attestation(&domain, &a, &other, Some(&[0xdd; 32]));
        assert!(matches!(
            result,
            Err(AttestationVerifyError::SignerMismatch { .. })
        ));
    }

    #[test]
    fn verify_rejects_payload_hash_mismatch() {
        let key = SigningKey::from_bytes(&[0x77; 32].into()).unwrap();
        let a = signed_attestation(&key, [0xdd; 32]);
        let signer = key_to_address(&key);
        let domain = AttestationDomain {
            settlement_chain_id: 42161,
            verifying_contract: [0xab; 20],
        };
        let result = verify_attestation(&domain, &a, &signer, Some(&[0x00; 32]));
        assert!(matches!(
            result,
            Err(AttestationVerifyError::PayloadHashMismatch)
        ));
    }

    #[test]
    fn split_cursor_strips_suffix() {
        let key = SigningKey::from_bytes(&[0x77; 32].into()).unwrap();
        let a = signed_attestation(&key, [0xdd; 32]);
        let packed_hex = {
            let mut out = Vec::new();
            out.extend_from_slice(&a.chain_id);
            out.extend_from_slice(&a.block_number.to_be_bytes());
            out.extend_from_slice(&a.block_hash);
            out.extend_from_slice(&a.state_root);
            out.extend_from_slice(&a.payload_hash);
            out.extend_from_slice(&a.signature);
            hex::encode(out)
        };
        let cursor = format!("inner-cursor{CURSOR_ATTESTATION_DELIMITER}{packed_hex}");
        let (inner, parsed) = split_cursor(&cursor).expect("split");
        assert_eq!(inner, "inner-cursor");
        assert_eq!(parsed, a);
    }
}
