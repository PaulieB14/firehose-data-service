//! MainlineAttestation — EIP-712 signed gRPC trailer attached to every
//! served block. See GRC-006 §2.2 and §2.6.
//!
//! Anchors all three verification tiers:
//!   - Tier 1 (Phase 3): compared against canonical chain header proof
//!   - Tier 2: payload_hash compared across operators
//!   - Tier 3: retained by consumers as a tamper-evident audit log

pub mod eip712;

#[derive(Debug, Clone)]
pub struct MainlineAttestation {
    pub chain_id: [u8; 32],
    pub block_number: u64,
    pub block_hash: [u8; 32],
    pub state_root: [u8; 32],
    pub payload_hash: [u8; 32],
    pub cursor: Vec<u8>,
    /// EIP-712 signature over (chain_id, block_number, block_hash, payload_hash).
    /// Populated by `eip712::sign`. Empty when constructed.
    pub indexer_sig: Vec<u8>,
}
