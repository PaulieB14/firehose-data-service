//! Arbitrum One adapter. Arbitrum nodes (Nitro) are geth-derivative and
//! firehose-arbitrum re-uses the same `sf.ethereum.type.v2.Block` payload
//! that L1 emits — so the fingerprint decode is identical to Ethereum.

use super::ethereum::decode_evm_block_fingerprint;
use super::{AdapterError, BlockFingerprint, ChainAdapter};

pub struct ArbitrumAdapter {
    pub upstream_endpoint: String,
}

impl ArbitrumAdapter {
    pub const CHAIN_NAME: &'static str = "arbitrum-one";
    pub const EIP155_CHAIN_ID: u64 = 42161;

    pub fn new(upstream_endpoint: impl Into<String>) -> Self {
        Self {
            upstream_endpoint: upstream_endpoint.into(),
        }
    }

    fn chain_id_bytes() -> [u8; 32] {
        let mut id = [0u8; 32];
        id[24..].copy_from_slice(&Self::EIP155_CHAIN_ID.to_be_bytes());
        id
    }
}

#[async_trait::async_trait]
impl ChainAdapter for ArbitrumAdapter {
    fn chain_id(&self) -> [u8; 32] {
        Self::chain_id_bytes()
    }
    fn firehose_proto_type(&self) -> &'static str {
        "sf.ethereum.type.v2.Block"
    }
    fn chain_name(&self) -> &'static str {
        Self::CHAIN_NAME
    }

    async fn current_lib(&self) -> Result<u64, AdapterError> {
        Err(AdapterError::NotImplemented)
    }

    /// Arbitrum One ships geth-format `Block` payloads via firehose-arbitrum.
    /// Reuse the shared helper so T1 disputes can land on Arbitrum without
    /// a separate proto path. Note: Arbitrum's `state_root` is the
    /// post-execution L2 state root, not L1's — same field, different chain
    /// semantics; verifiers downstream are responsible for sourcing the
    /// matching canonical header.
    fn fingerprint(&self, payload: &[u8]) -> Result<BlockFingerprint, AdapterError> {
        decode_evm_block_fingerprint(payload)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::grpc::ethereum_type::{Block, BlockHeader};
    use prost::Message;

    #[test]
    fn chain_id_encodes_42161_in_last_three_bytes() {
        let a = ArbitrumAdapter::new("http://localhost:13045");
        let id = a.chain_id();
        // 42161 = 0x00A4B1 → last byte 0xb1, then 0xa4, then 0x00.
        assert_eq!(id[31], 0xb1);
        assert_eq!(id[30], 0xa4);
        assert_eq!(id[29], 0x00);
        assert!(id[..29].iter().all(|b| *b == 0));
    }

    #[test]
    fn chain_name_is_arbitrum_one() {
        let a = ArbitrumAdapter::new("x");
        assert_eq!(a.chain_name(), "arbitrum-one");
    }

    #[test]
    fn fingerprint_decodes_via_shared_evm_helper() {
        let block = Block {
            hash: vec![0xa1; 32],
            number: 250_000_000, // Arbitrum block heights run high
            header: Some(BlockHeader {
                state_root: vec![0x42; 32],
            }),
        };
        let payload = block.encode_to_vec();

        let fp = ArbitrumAdapter::new("x")
            .fingerprint(&payload)
            .expect("decode");
        assert_eq!(fp.block_number, 250_000_000);
        assert_eq!(fp.block_hash, [0xa1; 32]);
        assert_eq!(fp.state_root, [0x42; 32]);
    }

    #[test]
    fn fingerprint_rejects_garbage_bytes() {
        let garbage = vec![0xff; 64];
        let result = ArbitrumAdapter::new("x").fingerprint(&garbage);
        assert!(matches!(result, Err(AdapterError::Decode(_))));
    }
}
