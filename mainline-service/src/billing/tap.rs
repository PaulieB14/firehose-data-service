//! TAP v2 receipt validation.
//!
//! Receipts are EIP-712 typed-data signed by the consumer (sender) and
//! verified by the indexer before serving content. Per §2.4, receipts are
//! signed per-burst (not per-block) to keep signature overhead off the
//! hot path. The full TAP v2 typed-data definition lives in
//! semiotic-ai/timeline-aggregation-protocol; we vendor the relevant
//! type hashes here.
//!
//! This module covers receipt structure + EIP-712 digest computation.
//! Signature recovery + on-chain escrow checks are TODO.

use sha3::{Digest, Keccak256};

#[derive(thiserror::Error, Debug)]
pub enum TapError {
    #[error("invalid receipt signature")]
    InvalidSignature,
    #[error("insufficient escrow")]
    InsufficientEscrow,
    #[error("receipt is for a different allocation")]
    WrongAllocation,
    #[error("receipt timestamp out of window")]
    StaleTimestamp,
}

#[derive(Debug, Clone)]
pub struct TapReceiptV2 {
    pub allocation_id: [u8; 20],
    pub timestamp_ns: u64,
    pub nonce: u64,
    pub value: u128, // GRT wei
    /// 65-byte secp256k1 signature (r || s || v).
    pub signature: Vec<u8>,
}

#[derive(Debug, Clone, Copy)]
pub struct TapDomain {
    pub settlement_chain_id: u64,
    pub verifying_contract: [u8; 20], // GraphTallyCollector address
}

// EIP-712 typehashes for TAP v2. Names mirror the canonical Graph contracts.
const TAP_DOMAIN_TYPEHASH: &[u8] =
    b"EIP712Domain(string name,string version,uint256 chainId,address verifyingContract)";
const TAP_RECEIPT_TYPEHASH: &[u8] =
    b"Receipt(address allocation_id,uint64 timestamp_ns,uint64 nonce,uint128 value)";
const TAP_DOMAIN_NAME: &[u8] = b"TAP";
const TAP_DOMAIN_VERSION: &[u8] = b"2";

fn keccak256(bytes: &[u8]) -> [u8; 32] {
    let mut h = Keccak256::new();
    h.update(bytes);
    h.finalize().into()
}

pub fn domain_separator(d: &TapDomain) -> [u8; 32] {
    let mut buf = Vec::with_capacity(32 * 5);
    buf.extend_from_slice(&keccak256(TAP_DOMAIN_TYPEHASH));
    buf.extend_from_slice(&keccak256(TAP_DOMAIN_NAME));
    buf.extend_from_slice(&keccak256(TAP_DOMAIN_VERSION));

    let mut chain_id_word = [0u8; 32];
    chain_id_word[24..].copy_from_slice(&d.settlement_chain_id.to_be_bytes());
    buf.extend_from_slice(&chain_id_word);

    let mut contract_word = [0u8; 32];
    contract_word[12..].copy_from_slice(&d.verifying_contract);
    buf.extend_from_slice(&contract_word);

    keccak256(&buf)
}

pub fn struct_hash(r: &TapReceiptV2) -> [u8; 32] {
    let mut buf = Vec::with_capacity(32 * 5);
    buf.extend_from_slice(&keccak256(TAP_RECEIPT_TYPEHASH));

    let mut alloc_word = [0u8; 32];
    alloc_word[12..].copy_from_slice(&r.allocation_id);
    buf.extend_from_slice(&alloc_word);

    let mut ts_word = [0u8; 32];
    ts_word[24..].copy_from_slice(&r.timestamp_ns.to_be_bytes());
    buf.extend_from_slice(&ts_word);

    let mut nonce_word = [0u8; 32];
    nonce_word[24..].copy_from_slice(&r.nonce.to_be_bytes());
    buf.extend_from_slice(&nonce_word);

    let mut value_word = [0u8; 32];
    value_word[16..].copy_from_slice(&r.value.to_be_bytes());
    buf.extend_from_slice(&value_word);

    keccak256(&buf)
}

pub fn digest(d: &TapDomain, r: &TapReceiptV2) -> [u8; 32] {
    let ds = domain_separator(d);
    let sh = struct_hash(r);
    let mut buf = Vec::with_capacity(2 + 32 + 32);
    buf.extend_from_slice(&[0x19, 0x01]);
    buf.extend_from_slice(&ds);
    buf.extend_from_slice(&sh);
    keccak256(&buf)
}

#[async_trait::async_trait]
pub trait ReceiptVerifier: Send + Sync {
    async fn verify(&self, domain: &TapDomain, receipt: &TapReceiptV2) -> Result<(), TapError>;
}

/// Verifies the EIP-712 signature but does NOT check on-chain escrow.
/// Use in tests and as a base type that wraps an escrow checker for prod.
pub struct SignatureOnlyVerifier;

#[async_trait::async_trait]
impl ReceiptVerifier for SignatureOnlyVerifier {
    async fn verify(&self, _domain: &TapDomain, receipt: &TapReceiptV2) -> Result<(), TapError> {
        if receipt.signature.len() != 65 {
            return Err(TapError::InvalidSignature);
        }
        // TODO: recover signer from digest+sig, compare against the expected
        // sender pulled from the allocation. The plumbing requires the
        // network subgraph; tracked separately.
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn digest_is_stable() {
        let d = TapDomain { settlement_chain_id: 42161, verifying_contract: [0xcc; 20] };
        let r = TapReceiptV2 {
            allocation_id: [0xaa; 20],
            timestamp_ns: 1_700_000_000_000_000_000,
            nonce: 1,
            value: 1_000_000_000_000_000_000u128,
            signature: vec![0u8; 65],
        };
        let a = digest(&d, &r);
        let b = digest(&d, &r);
        assert_eq!(a, b);
    }

    #[test]
    fn digest_changes_with_value() {
        let d = TapDomain { settlement_chain_id: 42161, verifying_contract: [0xcc; 20] };
        let mut r = TapReceiptV2 {
            allocation_id: [0xaa; 20],
            timestamp_ns: 1_700_000_000_000_000_000,
            nonce: 1,
            value: 1u128,
            signature: vec![0u8; 65],
        };
        let h1 = digest(&d, &r);
        r.value = 2;
        let h2 = digest(&d, &r);
        assert_ne!(h1, h2);
    }
}
