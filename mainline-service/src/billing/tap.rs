//! TAP v2 receipt validation.
//!
//! Receipts are EIP-712 typed-data signed by the consumer (sender) and
//! verified by the indexer before serving content. Per §2.4, receipts are
//! signed per-burst (not per-block) to keep signature overhead off the
//! hot path. The full TAP v2 typed-data definition lives in
//! semiotic-ai/timeline-aggregation-protocol; we vendor the relevant
//! type hashes here.
//!
//! This module covers receipt structure, EIP-712 digest computation,
//! signature recovery (`recover_signer`), and an `EscrowVerifier` that
//! layers an allocation→sender + escrow lookup on top.

use std::collections::HashMap;
use std::sync::Mutex;
use std::time::{Duration, Instant};

use k256::ecdsa::{RecoveryId, Signature, VerifyingKey};
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

#[derive(thiserror::Error, Debug)]
pub enum WireDecodeError {
    #[error("truncated receipt (expected {expected} bytes, got {actual})")]
    Truncated { expected: usize, actual: usize },
    #[error("unsupported wire version: {0}")]
    UnsupportedVersion(u8),
}

/// Compact wire format consumers use to attach a TAP receipt to a gRPC
/// request via the `x-tap-receipt` metadata header. All fields big-endian:
///
///   version             (1 byte, must equal `RECEIPT_WIRE_VERSION`)
///   allocation_id       (20 bytes)
///   timestamp_ns        (8 bytes, u64 BE)
///   nonce               (8 bytes, u64 BE)
///   value               (16 bytes, u128 BE)
///   signature           (65 bytes, r||s||v)
///
/// Total: 118 bytes. The header value is the hex-encoded form so it's
/// safe to ship via gRPC metadata.
pub const RECEIPT_WIRE_VERSION: u8 = 1;
pub const RECEIPT_WIRE_LEN: usize = 1 + 20 + 8 + 8 + 16 + 65;

pub fn encode_receipt(r: &TapReceiptV2) -> Vec<u8> {
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

pub fn decode_receipt(bytes: &[u8]) -> Result<TapReceiptV2, WireDecodeError> {
    if bytes.len() < RECEIPT_WIRE_LEN {
        return Err(WireDecodeError::Truncated {
            expected: RECEIPT_WIRE_LEN,
            actual: bytes.len(),
        });
    }
    if bytes[0] != RECEIPT_WIRE_VERSION {
        return Err(WireDecodeError::UnsupportedVersion(bytes[0]));
    }
    let mut p = 1usize;
    let mut allocation_id = [0u8; 20];
    allocation_id.copy_from_slice(&bytes[p..p + 20]);
    p += 20;
    let mut ts = [0u8; 8];
    ts.copy_from_slice(&bytes[p..p + 8]);
    p += 8;
    let mut nonce = [0u8; 8];
    nonce.copy_from_slice(&bytes[p..p + 8]);
    p += 8;
    let mut value = [0u8; 16];
    value.copy_from_slice(&bytes[p..p + 16]);
    p += 16;
    let signature = bytes[p..p + 65].to_vec();
    Ok(TapReceiptV2 {
        allocation_id,
        timestamp_ns: u64::from_be_bytes(ts),
        nonce: u64::from_be_bytes(nonce),
        value: u128::from_be_bytes(value),
        signature,
    })
}

#[async_trait::async_trait]
pub trait ReceiptVerifier: Send + Sync {
    async fn verify(&self, domain: &TapDomain, receipt: &TapReceiptV2) -> Result<(), TapError>;
}

/// Recover the secp256k1 signer address from a 65-byte (r || s || v) signature
/// over the given prehash, returning the lowercased 20-byte Ethereum address.
/// `v` may be either the legacy form (`27`/`28`) or the raw recovery id
/// (`0`/`1`). EIP-2098 compact signatures are NOT supported (TAP v2 uses
/// 65-byte form).
pub fn recover_signer(prehash: &[u8; 32], sig: &[u8]) -> Result<[u8; 20], TapError> {
    if sig.len() != 65 {
        return Err(TapError::InvalidSignature);
    }
    let raw_v = sig[64];
    let rec_byte = if raw_v >= 27 { raw_v - 27 } else { raw_v };
    let rec_id = RecoveryId::try_from(rec_byte).map_err(|_| TapError::InvalidSignature)?;

    let mut rs = [0u8; 64];
    rs.copy_from_slice(&sig[..64]);
    let signature = Signature::from_slice(&rs).map_err(|_| TapError::InvalidSignature)?;

    let verifying = VerifyingKey::recover_from_prehash(prehash, &signature, rec_id)
        .map_err(|_| TapError::InvalidSignature)?;

    let pubkey_bytes = verifying.to_encoded_point(false);
    let pubkey = pubkey_bytes.as_bytes();
    // First byte is the 0x04 uncompressed tag; the address is the keccak256
    // of the remaining 64 bytes, last 20 bytes.
    let hash = keccak256(&pubkey[1..]);
    let mut addr = [0u8; 20];
    addr.copy_from_slice(&hash[12..]);
    Ok(addr)
}

/// Stateless verifier that only checks the 65-byte signature shape. Use this
/// in tests and as a baseline that callers can stack escrow checks on top of.
pub struct SignatureOnlyVerifier;

#[async_trait::async_trait]
impl ReceiptVerifier for SignatureOnlyVerifier {
    async fn verify(&self, _domain: &TapDomain, receipt: &TapReceiptV2) -> Result<(), TapError> {
        if receipt.signature.len() != 65 {
            return Err(TapError::InvalidSignature);
        }
        Ok(())
    }
}

/// Resolves an allocation id to its payer (sender) address and current
/// available escrow balance. In production this is backed by an HTTP client
/// against the Mainline network subgraph (issue #5); for tests we ship an
/// `InMemoryAllocationLookup`.
#[async_trait::async_trait]
pub trait AllocationLookup: Send + Sync {
    async fn lookup(&self, allocation_id: &[u8; 20]) -> Result<AllocationInfo, TapError>;
}

#[derive(Debug, Clone)]
pub struct AllocationInfo {
    /// The address expected to sign receipts for this allocation.
    pub payer: [u8; 20],
    /// Available escrow balance in GRT wei, last observed.
    pub escrow_available: u128,
}

/// In-memory `AllocationLookup` for tests + local devloops.
pub struct InMemoryAllocationLookup {
    inner: Mutex<HashMap<[u8; 20], AllocationInfo>>,
}

impl InMemoryAllocationLookup {
    pub fn new() -> Self {
        Self { inner: Mutex::new(HashMap::new()) }
    }
    pub fn insert(&self, allocation_id: [u8; 20], info: AllocationInfo) {
        self.inner.lock().unwrap().insert(allocation_id, info);
    }
}

impl Default for InMemoryAllocationLookup {
    fn default() -> Self { Self::new() }
}

#[async_trait::async_trait]
impl AllocationLookup for InMemoryAllocationLookup {
    async fn lookup(&self, allocation_id: &[u8; 20]) -> Result<AllocationInfo, TapError> {
        self.inner
            .lock()
            .unwrap()
            .get(allocation_id)
            .cloned()
            .ok_or(TapError::WrongAllocation)
    }
}

/// Production-grade verifier: signature recovery + signer/payer match + escrow
/// presence + staleness window. Backed by any `AllocationLookup`; wrap the
/// HTTP-backed implementation in `CachingAllocationLookup` for prod.
pub struct EscrowVerifier<L: AllocationLookup> {
    pub lookup: L,
    /// Maximum age allowed between the receipt's `timestamp_ns` and `now()`.
    /// Default 5 minutes per §2.4 RAV cadence.
    pub max_age: Duration,
    /// Clock used to age-check receipts. Tests inject a fixed clock.
    pub now_ns: fn() -> u64,
}

impl<L: AllocationLookup> EscrowVerifier<L> {
    pub fn new(lookup: L) -> Self {
        Self { lookup, max_age: Duration::from_secs(300), now_ns: system_now_ns }
    }
}

fn system_now_ns() -> u64 {
    use std::time::SystemTime;
    SystemTime::now()
        .duration_since(SystemTime::UNIX_EPOCH)
        .map(|d| d.as_nanos() as u64)
        .unwrap_or(0)
}

#[async_trait::async_trait]
impl<L: AllocationLookup> ReceiptVerifier for EscrowVerifier<L> {
    async fn verify(&self, domain: &TapDomain, receipt: &TapReceiptV2) -> Result<(), TapError> {
        // 1. Shape check.
        if receipt.signature.len() != 65 {
            return Err(TapError::InvalidSignature);
        }

        // 2. Staleness window.
        let now_ns = (self.now_ns)();
        let receipt_ns = receipt.timestamp_ns;
        let age_ns = now_ns.saturating_sub(receipt_ns);
        if age_ns > self.max_age.as_nanos() as u64 {
            return Err(TapError::StaleTimestamp);
        }
        // Future-dated receipts (within a 60s skew) are tolerated; further out
        // is rejected to prevent a misconfigured clock from accepting a year
        // of receipts.
        if receipt_ns > now_ns + 60_000_000_000 {
            return Err(TapError::StaleTimestamp);
        }

        // 3. Recover signer.
        let prehash = digest(domain, receipt);
        let signer = recover_signer(&prehash, &receipt.signature)?;

        // 4. Allocation lookup + sender match + escrow.
        let info = self.lookup.lookup(&receipt.allocation_id).await?;
        if signer != info.payer {
            return Err(TapError::InvalidSignature);
        }
        if (receipt.value as u128) > info.escrow_available {
            return Err(TapError::InsufficientEscrow);
        }

        Ok(())
    }
}

/// Cache wrapper for any `AllocationLookup`. Escrow balances change slowly
/// (RAV cadence is per-burst, not per-block), so a TTL of seconds is plenty.
pub struct CachingAllocationLookup<L: AllocationLookup> {
    inner: L,
    ttl: Duration,
    cache: Mutex<HashMap<[u8; 20], (Instant, AllocationInfo)>>,
}

impl<L: AllocationLookup> CachingAllocationLookup<L> {
    pub fn new(inner: L, ttl: Duration) -> Self {
        Self { inner, ttl, cache: Mutex::new(HashMap::new()) }
    }
}

#[async_trait::async_trait]
impl<L: AllocationLookup> AllocationLookup for CachingAllocationLookup<L> {
    async fn lookup(&self, allocation_id: &[u8; 20]) -> Result<AllocationInfo, TapError> {
        if let Some((stamped, info)) = self.cache.lock().unwrap().get(allocation_id).cloned() {
            if stamped.elapsed() <= self.ttl {
                return Ok(info);
            }
        }
        let fresh = self.inner.lookup(allocation_id).await?;
        self.cache
            .lock()
            .unwrap()
            .insert(*allocation_id, (Instant::now(), fresh.clone()));
        Ok(fresh)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    use k256::ecdsa::{signature::hazmat::PrehashSigner, SigningKey};

    fn sign_test_receipt(key: &SigningKey, prehash: &[u8; 32]) -> Vec<u8> {
        let (sig, rec_id): (Signature, RecoveryId) =
            key.sign_prehash(prehash).expect("sign");
        let mut out = Vec::with_capacity(65);
        out.extend_from_slice(&sig.to_bytes());
        out.push(rec_id.to_byte() + 27);
        out
    }

    fn key_to_address(key: &SigningKey) -> [u8; 20] {
        let verifying = key.verifying_key();
        let pt = verifying.to_encoded_point(false);
        let hash = keccak256(&pt.as_bytes()[1..]);
        let mut addr = [0u8; 20];
        addr.copy_from_slice(&hash[12..]);
        addr
    }

    fn fixed_now_ns_2026() -> u64 {
        // 2026-05-11T00:00:00Z in ns since epoch.
        1_778_976_000_000_000_000
    }

    #[test]
    fn recover_signer_matches_key_address() {
        let key = SigningKey::from_bytes(&[0x42; 32].into()).unwrap();
        let expected = key_to_address(&key);
        let domain = TapDomain { settlement_chain_id: 42161, verifying_contract: [0xcc; 20] };
        let mut receipt = TapReceiptV2 {
            allocation_id: [0xaa; 20],
            timestamp_ns: fixed_now_ns_2026(),
            nonce: 1,
            value: 1,
            signature: vec![],
        };
        let prehash = digest(&domain, &receipt);
        receipt.signature = sign_test_receipt(&key, &prehash);
        let recovered = recover_signer(&prehash, &receipt.signature).expect("recover");
        assert_eq!(recovered, expected);
    }

    #[tokio::test]
    async fn escrow_verifier_accepts_valid_receipt() {
        let key = SigningKey::from_bytes(&[0x01; 32].into()).unwrap();
        let payer = key_to_address(&key);
        let lookup = InMemoryAllocationLookup::new();
        lookup.insert([0xaa; 20], AllocationInfo { payer, escrow_available: 10_000_000_000 });

        let domain = TapDomain { settlement_chain_id: 42161, verifying_contract: [0xcc; 20] };
        let mut receipt = TapReceiptV2 {
            allocation_id: [0xaa; 20],
            timestamp_ns: fixed_now_ns_2026(),
            nonce: 1,
            value: 100,
            signature: vec![],
        };
        let prehash = digest(&domain, &receipt);
        receipt.signature = sign_test_receipt(&key, &prehash);

        let verifier = EscrowVerifier {
            lookup,
            max_age: Duration::from_secs(300),
            now_ns: fixed_now_ns_2026,
        };
        verifier.verify(&domain, &receipt).await.expect("should pass");
    }

    #[tokio::test]
    async fn escrow_verifier_rejects_wrong_signer() {
        let signing = SigningKey::from_bytes(&[0x01; 32].into()).unwrap();
        let other = SigningKey::from_bytes(&[0x02; 32].into()).unwrap();
        let payer = key_to_address(&other); // allocation expects `other` to sign

        let lookup = InMemoryAllocationLookup::new();
        lookup.insert([0xaa; 20], AllocationInfo { payer, escrow_available: 10_000_000_000 });

        let domain = TapDomain { settlement_chain_id: 42161, verifying_contract: [0xcc; 20] };
        let mut receipt = TapReceiptV2 {
            allocation_id: [0xaa; 20],
            timestamp_ns: fixed_now_ns_2026(),
            nonce: 1,
            value: 100,
            signature: vec![],
        };
        let prehash = digest(&domain, &receipt);
        receipt.signature = sign_test_receipt(&signing, &prehash);

        let verifier = EscrowVerifier {
            lookup,
            max_age: Duration::from_secs(300),
            now_ns: fixed_now_ns_2026,
        };
        assert!(matches!(
            verifier.verify(&domain, &receipt).await,
            Err(TapError::InvalidSignature)
        ));
    }

    #[tokio::test]
    async fn escrow_verifier_rejects_unknown_allocation() {
        let key = SigningKey::from_bytes(&[0x01; 32].into()).unwrap();
        let lookup = InMemoryAllocationLookup::new(); // empty
        let domain = TapDomain { settlement_chain_id: 42161, verifying_contract: [0xcc; 20] };
        let mut receipt = TapReceiptV2 {
            allocation_id: [0xaa; 20],
            timestamp_ns: fixed_now_ns_2026(),
            nonce: 1,
            value: 100,
            signature: vec![],
        };
        let prehash = digest(&domain, &receipt);
        receipt.signature = sign_test_receipt(&key, &prehash);

        let verifier = EscrowVerifier {
            lookup,
            max_age: Duration::from_secs(300),
            now_ns: fixed_now_ns_2026,
        };
        assert!(matches!(
            verifier.verify(&domain, &receipt).await,
            Err(TapError::WrongAllocation)
        ));
    }

    #[tokio::test]
    async fn escrow_verifier_rejects_insufficient_escrow() {
        let key = SigningKey::from_bytes(&[0x01; 32].into()).unwrap();
        let payer = key_to_address(&key);
        let lookup = InMemoryAllocationLookup::new();
        lookup.insert([0xaa; 20], AllocationInfo { payer, escrow_available: 50 });

        let domain = TapDomain { settlement_chain_id: 42161, verifying_contract: [0xcc; 20] };
        let mut receipt = TapReceiptV2 {
            allocation_id: [0xaa; 20],
            timestamp_ns: fixed_now_ns_2026(),
            nonce: 1,
            value: 100, // > escrow_available
            signature: vec![],
        };
        let prehash = digest(&domain, &receipt);
        receipt.signature = sign_test_receipt(&key, &prehash);

        let verifier = EscrowVerifier {
            lookup,
            max_age: Duration::from_secs(300),
            now_ns: fixed_now_ns_2026,
        };
        assert!(matches!(
            verifier.verify(&domain, &receipt).await,
            Err(TapError::InsufficientEscrow)
        ));
    }

    #[tokio::test]
    async fn escrow_verifier_rejects_stale_receipt() {
        let key = SigningKey::from_bytes(&[0x01; 32].into()).unwrap();
        let payer = key_to_address(&key);
        let lookup = InMemoryAllocationLookup::new();
        lookup.insert([0xaa; 20], AllocationInfo { payer, escrow_available: 10_000_000_000 });

        let domain = TapDomain { settlement_chain_id: 42161, verifying_contract: [0xcc; 20] };
        // Receipt timestamp is 1 hour BEFORE fixed_now_ns_2026.
        let mut receipt = TapReceiptV2 {
            allocation_id: [0xaa; 20],
            timestamp_ns: fixed_now_ns_2026() - 3_600_000_000_000,
            nonce: 1,
            value: 100,
            signature: vec![],
        };
        let prehash = digest(&domain, &receipt);
        receipt.signature = sign_test_receipt(&key, &prehash);

        let verifier = EscrowVerifier {
            lookup,
            max_age: Duration::from_secs(300),
            now_ns: fixed_now_ns_2026,
        };
        assert!(matches!(
            verifier.verify(&domain, &receipt).await,
            Err(TapError::StaleTimestamp)
        ));
    }

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
    fn wire_roundtrip() {
        let r = TapReceiptV2 {
            allocation_id: [0xaa; 20],
            timestamp_ns: 1_700_000_000_000_000_000,
            nonce: 42,
            value: 9_876_543_210_000_000_000u128,
            signature: vec![7u8; 65],
        };
        let bytes = encode_receipt(&r);
        assert_eq!(bytes.len(), RECEIPT_WIRE_LEN);
        let r2 = decode_receipt(&bytes).expect("decode");
        assert_eq!(r2.allocation_id, r.allocation_id);
        assert_eq!(r2.timestamp_ns, r.timestamp_ns);
        assert_eq!(r2.nonce, r.nonce);
        assert_eq!(r2.value, r.value);
        assert_eq!(r2.signature, r.signature);
    }

    #[test]
    fn wire_rejects_truncated() {
        let r = TapReceiptV2 {
            allocation_id: [0u8; 20],
            timestamp_ns: 0,
            nonce: 0,
            value: 0,
            signature: vec![0u8; 65],
        };
        let bytes = encode_receipt(&r);
        let truncated = &bytes[..bytes.len() - 1];
        assert!(matches!(
            decode_receipt(truncated),
            Err(WireDecodeError::Truncated { .. })
        ));
    }

    #[test]
    fn wire_rejects_wrong_version() {
        let mut bytes = encode_receipt(&TapReceiptV2 {
            allocation_id: [0u8; 20],
            timestamp_ns: 0,
            nonce: 0,
            value: 0,
            signature: vec![0u8; 65],
        });
        bytes[0] = 99;
        assert!(matches!(
            decode_receipt(&bytes),
            Err(WireDecodeError::UnsupportedVersion(99))
        ));
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
