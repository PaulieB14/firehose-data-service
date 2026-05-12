//! Base adapter. Same geth-firehose patch as Ethereum L1.

use super::{AdapterError, ChainAdapter};

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
}
