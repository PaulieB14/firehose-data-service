//! Base adapter. Same geth-firehose patch as Ethereum L1, so the per-block
//! fingerprint decode is identical — we delegate to the shared EVM helper.

use super::ethereum::decode_evm_block_fingerprint;
use super::{AdapterError, BlockFingerprint, ChainAdapter};

pub struct BaseAdapter {
    pub upstream_endpoint: String,
}

impl BaseAdapter {
    pub const CHAIN_NAME: &'static str = "base-mainnet";
    pub const EIP155_CHAIN_ID: u64 = 8453;

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
impl ChainAdapter for BaseAdapter {
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

    /// Base is a geth-derivative OP-Stack chain; its firehose payload is
    /// the same `sf.ethereum.type.v2.Block` as Ethereum L1. Reuse the
    /// shared helper so T1 disputes can land on Base without a separate
    /// proto path.
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
    fn chain_id_encodes_8453_in_last_two_bytes() {
        let a = BaseAdapter::new("http://localhost:13044");
        let id = a.chain_id();
        // 8453 = 0x2105 → last byte 0x05, second-to-last 0x21
        assert_eq!(id[30], 0x21);
        assert_eq!(id[31], 0x05);
        assert!(id[..30].iter().all(|b| *b == 0));
    }

    #[test]
    fn chain_name_is_base_mainnet() {
        let a = BaseAdapter::new("x");
        assert_eq!(a.chain_name(), "base-mainnet");
    }

    #[test]
    fn fingerprint_decodes_via_shared_evm_helper() {
        let block = Block {
            hash: vec![0xba; 32],
            number: 13_000_000,
            header: Some(BlockHeader {
                state_root: vec![0x5e; 32],
            }),
        };
        let payload = block.encode_to_vec();

        let fp = BaseAdapter::new("x").fingerprint(&payload).expect("decode");
        assert_eq!(fp.block_number, 13_000_000);
        assert_eq!(fp.block_hash, [0xba; 32]);
        assert_eq!(fp.state_root, [0x5e; 32]);
    }

    #[test]
    fn fingerprint_rejects_garbage_bytes() {
        let garbage = vec![0xff; 64];
        let result = BaseAdapter::new("x").fingerprint(&garbage);
        assert!(matches!(result, Err(AdapterError::Decode(_))));
    }
}
