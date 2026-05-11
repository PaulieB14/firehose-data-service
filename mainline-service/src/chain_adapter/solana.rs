//! Solana adapter. Wraps a firesol / firehose-solana endpoint. Stub.
//!
//! Note from GRC-006 §6: Solana merged-blocks are ~61 GiB compressed per
//! day (~22 TiB/year). Storage and bandwidth profile is materially heavier
//! than EVM chains.

use super::{AdapterError, ChainAdapter};

pub struct SolanaAdapter {
    pub upstream_endpoint: String,
}

impl ChainAdapter for SolanaAdapter {
    fn chain_id(&self) -> [u8; 32] {
        // Solana doesn't have an EIP-155 chain id; use a Mainline-assigned
        // namespace identifier. Final encoding TBD in a real PR.
        let mut id = [0u8; 32];
        id[0..7].copy_from_slice(b"solana_");
        id
    }

    fn firehose_proto_type(&self) -> &'static str {
        "sf.solana.type.v1.Block"
    }

    fn current_lib(&self) -> Result<u64, AdapterError> {
        Err(AdapterError::NotImplemented)
    }
}
