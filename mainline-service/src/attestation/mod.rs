//! MainlineAttestation — EIP-712 signed gRPC trailer attached to every
//! served block. See GRC-006 §2.2 and §2.6.

pub mod eip712;

#[derive(Debug, Clone)]
pub struct MainlineAttestation {
    pub chain_id: [u8; 32],
    pub block_number: u64,
    pub block_hash: [u8; 32],
    pub state_root: [u8; 32],
    pub payload_hash: [u8; 32],
    pub cursor: Vec<u8>,
    /// EIP-712 signature (r || s || v, 65 bytes) over the typed-data hash.
    /// Empty when newly constructed; populated by `eip712::sign`.
    pub indexer_sig: Vec<u8>,
}

impl MainlineAttestation {
    pub fn new(
        chain_id: [u8; 32],
        block_number: u64,
        block_hash: [u8; 32],
        state_root: [u8; 32],
        payload_hash: [u8; 32],
        cursor: Vec<u8>,
    ) -> Self {
        Self {
            chain_id,
            block_number,
            block_hash,
            state_root,
            payload_hash,
            cursor,
            indexer_sig: Vec::new(),
        }
    }
}
