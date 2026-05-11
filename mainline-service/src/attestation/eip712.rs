//! EIP-712 domain + signing for MainlineAttestation.
//!
//! Domain:
//!   name:              "Mainline"
//!   version:           "1"
//!   chainId:           settlement chain (e.g. Arbitrum One = 42161)
//!   verifyingContract: deployed FirehoseDataService address
//!
//! Struct:
//!   MainlineAttestation(
//!     bytes32 chainId,
//!     uint64 blockNumber,
//!     bytes32 blockHash,
//!     bytes32 stateRoot,
//!     bytes32 payloadHash
//!   )
//!
//! NOTE: `cursor` is intentionally excluded from the signed hash. A cursor
//! is opaque session state and changes shape across operators; signing it
//! would break cursor portability (§2.7). The four bound fields above are
//! sufficient to anchor all three verification tiers (§2.6).

use k256::ecdsa::{signature::hazmat::PrehashSigner, RecoveryId, Signature, SigningKey};
use sha3::{Digest, Keccak256};

use super::MainlineAttestation;

#[derive(thiserror::Error, Debug)]
pub enum AttestationError {
    #[error("invalid signing key")]
    InvalidKey,
    #[error("signing failed: {0}")]
    SigningFailed(String),
}

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

fn keccak256(bytes: &[u8]) -> [u8; 32] {
    let mut h = Keccak256::new();
    h.update(bytes);
    h.finalize().into()
}

/// EIP-712 domain separator hash.
pub fn domain_separator(domain: &AttestationDomain) -> [u8; 32] {
    let domain_typehash = keccak256(EIP712_DOMAIN_TYPEHASH);
    let name_hash = keccak256(DOMAIN_NAME);
    let version_hash = keccak256(DOMAIN_VERSION);

    let mut buf = Vec::with_capacity(32 * 5);
    buf.extend_from_slice(&domain_typehash);
    buf.extend_from_slice(&name_hash);
    buf.extend_from_slice(&version_hash);

    let mut chain_id_word = [0u8; 32];
    chain_id_word[24..].copy_from_slice(&domain.settlement_chain_id.to_be_bytes());
    buf.extend_from_slice(&chain_id_word);

    let mut contract_word = [0u8; 32];
    contract_word[12..].copy_from_slice(&domain.verifying_contract);
    buf.extend_from_slice(&contract_word);

    keccak256(&buf)
}

/// EIP-712 struct hash over the attestation fields.
pub fn struct_hash(att: &MainlineAttestation) -> [u8; 32] {
    let type_hash = keccak256(MAINLINE_ATTESTATION_TYPEHASH);

    let mut block_num_word = [0u8; 32];
    block_num_word[24..].copy_from_slice(&att.block_number.to_be_bytes());

    let mut buf = Vec::with_capacity(32 * 6);
    buf.extend_from_slice(&type_hash);
    buf.extend_from_slice(&att.chain_id);
    buf.extend_from_slice(&block_num_word);
    buf.extend_from_slice(&att.block_hash);
    buf.extend_from_slice(&att.state_root);
    buf.extend_from_slice(&att.payload_hash);

    keccak256(&buf)
}

/// Final EIP-712 digest = keccak256(0x1901 || domainSeparator || structHash).
pub fn digest(domain: &AttestationDomain, att: &MainlineAttestation) -> [u8; 32] {
    let ds = domain_separator(domain);
    let sh = struct_hash(att);
    let mut buf = Vec::with_capacity(2 + 32 + 32);
    buf.extend_from_slice(&[0x19, 0x01]);
    buf.extend_from_slice(&ds);
    buf.extend_from_slice(&sh);
    keccak256(&buf)
}

/// Sign the attestation in-place. Returns the 65-byte (r||s||v) signature.
pub fn sign(
    domain: &AttestationDomain,
    att: &mut MainlineAttestation,
    signing_key: &[u8; 32],
) -> Result<Vec<u8>, AttestationError> {
    let key = SigningKey::from_bytes(signing_key.into()).map_err(|_| AttestationError::InvalidKey)?;
    let d = digest(domain, att);

    let (sig, rec_id): (Signature, RecoveryId) = key
        .sign_prehash_recoverable(&d)
        .map_err(|e| AttestationError::SigningFailed(e.to_string()))?;

    let mut out = Vec::with_capacity(65);
    out.extend_from_slice(&sig.to_bytes());
    // Ethereum v = recId + 27
    out.push(rec_id.to_byte() + 27);

    att.indexer_sig = out.clone();
    Ok(out)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn domain_separator_is_stable() {
        let domain = AttestationDomain {
            settlement_chain_id: 42161,
            verifying_contract: [0xab; 20],
        };
        let a = domain_separator(&domain);
        let b = domain_separator(&domain);
        assert_eq!(a, b);
    }

    #[test]
    fn struct_hash_changes_with_block_number() {
        let mut att = MainlineAttestation::new([1u8; 32], 100, [2u8; 32], [3u8; 32], [4u8; 32], vec![]);
        let h1 = struct_hash(&att);
        att.block_number = 101;
        let h2 = struct_hash(&att);
        assert_ne!(h1, h2);
    }

    #[test]
    fn sign_with_test_key_produces_65_byte_sig() {
        let domain = AttestationDomain {
            settlement_chain_id: 42161,
            verifying_contract: [0xab; 20],
        };
        let mut att = MainlineAttestation::new([1u8; 32], 100, [2u8; 32], [3u8; 32], [4u8; 32], vec![]);
        // Deterministic non-zero key for testing.
        let key = [0x11u8; 32];
        let sig = sign(&domain, &mut att, &key).expect("sign failed");
        assert_eq!(sig.len(), 65);
        assert_eq!(att.indexer_sig, sig);
    }
}
