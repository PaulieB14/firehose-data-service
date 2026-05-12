//! Solana adapter. Wraps a firesol / firehose-solana endpoint.
//!
//! Note from GRC-006 §6: Solana merged-blocks are ~61 GiB compressed per
//! day (~22 TiB/year). Storage and bandwidth profile is materially heavier
//! than EVM chains.

use super::{AdapterError, BlockIdEncoding, ChainAdapter};

pub struct SolanaAdapter {
    pub upstream_endpoint: String,
}

impl SolanaAdapter {
    pub const CHAIN_NAME: &'static str = "solana-mainnet";

    pub fn new(upstream_endpoint: impl Into<String>) -> Self {
        Self {
            upstream_endpoint: upstream_endpoint.into(),
        }
    }

    /// Solana has no EIP-155 chain id. We use a Mainline-namespaced identifier:
    /// 0x534f4c ("SOL") in the high three bytes, zero pad. Format will be
    /// finalized in the chain-registration RFC.
    fn chain_id_bytes() -> [u8; 32] {
        let mut id = [0u8; 32];
        id[0] = b'S';
        id[1] = b'O';
        id[2] = b'L';
        id
    }
}

#[async_trait::async_trait]
impl ChainAdapter for SolanaAdapter {
    fn chain_id(&self) -> [u8; 32] {
        Self::chain_id_bytes()
    }
    fn firehose_proto_type(&self) -> &'static str {
        "sf.solana.type.v1.Block"
    }
    fn chain_name(&self) -> &'static str {
        Self::CHAIN_NAME
    }
    fn block_id_encoding(&self) -> BlockIdEncoding {
        BlockIdEncoding::Base58
    }

    async fn current_lib(&self) -> Result<u64, AdapterError> {
        Err(AdapterError::NotImplemented)
    }
}
