//! Consumer-side TAP v2 receipt signing.
//!
//! Mirrors the typehashes and digest computation in
//! `mainline-service::billing::tap` so receipts produced here verify against
//! the operator with no further coordination.

use k256::ecdsa::{RecoveryId, Signature, SigningKey};
use sha3::{Digest, Keccak256};

#[derive(thiserror::Error, Debug)]
pub enum SignerError {
    #[error("invalid signing key")]
    InvalidKey,
    #[error("signing failed: {0}")]
    SigningFailed(String),
}

/// EIP-712 domain. Settlement-chain (Arbitrum One / Sepolia for Phase 0)
/// plus the deployed GraphTallyCollector address.
#[derive(Clone, Copy, Debug)]
pub struct TapDomain {
    pub settlement_chain_id: u64,
    pub verifying_contract: [u8; 20],
}

#[derive(Clone, Debug)]
pub struct TapReceiptV2 {
    pub allocation_id: [u8; 20],
    pub timestamp_ns: u64,
    pub nonce: u64,
    pub value: u128,
    /// Populated by [`sign`]; empty before signing.
    pub signature: Vec<u8>,
}

// Typehashes — must stay byte-identical to mainline-service::billing::tap.
const TAP_DOMAIN_TYPEHASH: &[u8] =
    b"EIP712Domain(string name,string version,uint256 chainId,address verifyingContract)";
const TAP_RECEIPT_TYPEHASH: &[u8] =
    b"Receipt(address allocation_id,uint64 timestamp_ns,uint64 nonce,uint128 value)";
const TAP_DOMAIN_NAME: &[u8] = b"TAP";
const TAP_DOMAIN_VERSION: &[u8] = b"2";

fn keccak256(b: &[u8]) -> [u8; 32] {
    let mut h = Keccak256::new();
    h.update(b);
    h.finalize().into()
}

pub fn domain_separator(d: &TapDomain) -> [u8; 32] {
    let mut buf = Vec::with_capacity(32 * 5);
    buf.extend_from_slice(&keccak256(TAP_DOMAIN_TYPEHASH));
    buf.extend_from_slice(&keccak256(TAP_DOMAIN_NAME));
    buf.extend_from_slice(&keccak256(TAP_DOMAIN_VERSION));
    let mut chain = [0u8; 32];
    chain[24..].copy_from_slice(&d.settlement_chain_id.to_be_bytes());
    buf.extend_from_slice(&chain);
    let mut addr = [0u8; 32];
    addr[12..].copy_from_slice(&d.verifying_contract);
    buf.extend_from_slice(&addr);
    keccak256(&buf)
}

pub fn struct_hash(r: &TapReceiptV2) -> [u8; 32] {
    let mut buf = Vec::with_capacity(32 * 5);
    buf.extend_from_slice(&keccak256(TAP_RECEIPT_TYPEHASH));
    let mut alloc = [0u8; 32];
    alloc[12..].copy_from_slice(&r.allocation_id);
    buf.extend_from_slice(&alloc);
    let mut ts = [0u8; 32];
    ts[24..].copy_from_slice(&r.timestamp_ns.to_be_bytes());
    buf.extend_from_slice(&ts);
    let mut nonce = [0u8; 32];
    nonce[24..].copy_from_slice(&r.nonce.to_be_bytes());
    buf.extend_from_slice(&nonce);
    let mut val = [0u8; 32];
    val[16..].copy_from_slice(&r.value.to_be_bytes());
    buf.extend_from_slice(&val);
    keccak256(&buf)
}

pub fn digest(d: &TapDomain, r: &TapReceiptV2) -> [u8; 32] {
    let mut buf = Vec::with_capacity(2 + 32 + 32);
    buf.extend_from_slice(&[0x19, 0x01]);
    buf.extend_from_slice(&domain_separator(d));
    buf.extend_from_slice(&struct_hash(r));
    keccak256(&buf)
}

/// Sign a receipt in place. Returns the 65-byte (r || s || v) signature with
/// `v` in legacy Ethereum form (27/28).
pub fn sign(
    domain: &TapDomain,
    receipt: &mut TapReceiptV2,
    sender_key: &[u8; 32],
) -> Result<Vec<u8>, SignerError> {
    use k256::ecdsa::signature::hazmat::PrehashSigner;
    let key = SigningKey::from_bytes(sender_key.into()).map_err(|_| SignerError::InvalidKey)?;
    let d = digest(domain, receipt);
    let (sig, rec_id): (Signature, RecoveryId) =
        key.sign_prehash(&d).map_err(|e| SignerError::SigningFailed(e.to_string()))?;
    let mut out = Vec::with_capacity(65);
    out.extend_from_slice(&sig.to_bytes());
    out.push(rec_id.to_byte() + 27);
    receipt.signature = out.clone();
    Ok(out)
}

/// Wire format mirroring `mainline-service::billing::tap::encode_receipt`.
pub const RECEIPT_WIRE_VERSION: u8 = 1;
pub const RECEIPT_WIRE_LEN: usize = 1 + 20 + 8 + 8 + 16 + 65;

pub fn encode_wire(r: &TapReceiptV2) -> Vec<u8> {
    let mut out = Vec::with_capacity(RECEIPT_WIRE_LEN);
    out.push(RECEIPT_WIRE_VERSION);
    out.extend_from_slice(&r.allocation_id);
    out.extend_from_slice(&r.timestamp_ns.to_be_bytes());
    out.extend_from_slice(&r.nonce.to_be_bytes());
    out.extend_from_slice(&r.value.to_be_bytes());
    let mut sig = [0u8; 65];
    let n = r.signature.len().min(65);
    sig[..n].copy_from_slice(&r.signature[..n]);
    out.extend_from_slice(&sig);
    out
}

/// Convenience: produce the value to set on the `x-tap-receipt` gRPC
/// metadata header — hex-encoded wire form.
pub fn encode_header(receipt: &TapReceiptV2) -> String {
    hex::encode(encode_wire(receipt))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn sign_emits_65_byte_v27_or_v28_signature() {
        let d = TapDomain { settlement_chain_id: 42161, verifying_contract: [0xcc; 20] };
        let mut r = TapReceiptV2 {
            allocation_id: [0xaa; 20],
            timestamp_ns: 1,
            nonce: 1,
            value: 1,
            signature: vec![],
        };
        let key = [0x11u8; 32];
        let sig = sign(&d, &mut r, &key).expect("sign");
        assert_eq!(sig.len(), 65);
        assert!(sig[64] == 27 || sig[64] == 28);
        assert_eq!(r.signature, sig);
    }

    #[test]
    fn header_encoding_is_hex_of_wire() {
        let d = TapDomain { settlement_chain_id: 42161, verifying_contract: [0xcc; 20] };
        let mut r = TapReceiptV2 {
            allocation_id: [0xaa; 20],
            timestamp_ns: 1,
            nonce: 1,
            value: 1,
            signature: vec![],
        };
        sign(&d, &mut r, &[0x11; 32]).unwrap();
        let header = encode_header(&r);
        assert_eq!(header.len(), RECEIPT_WIRE_LEN * 2);
        assert!(header.chars().all(|c| c.is_ascii_hexdigit()));
    }
}
