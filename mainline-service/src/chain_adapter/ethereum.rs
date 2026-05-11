//! Ethereum L1 adapter. Wraps a firehose-ethereum endpoint.

use super::{AdapterError, ChainAdapter};

pub struct EthereumAdapter {
    pub upstream_endpoint: String,
}

impl EthereumAdapter {
    pub const CHAIN_NAME: &'static str = "ethereum-mainnet";
    pub const EIP155_CHAIN_ID: u64 = 1;

    pub fn new(upstream_endpoint: impl Into<String>) -> Self {
        Self { upstream_endpoint: upstream_endpoint.into() }
    }

    fn chain_id_bytes() -> [u8; 32] {
        let mut id = [0u8; 32];
        id[24..].copy_from_slice(&Self::EIP155_CHAIN_ID.to_be_bytes());
        id
    }
}

#[async_trait::async_trait]
impl ChainAdapter for EthereumAdapter {
    fn chain_id(&self) -> [u8; 32] { Self::chain_id_bytes() }
    fn firehose_proto_type(&self) -> &'static str { "sf.ethereum.type.v2.Block" }

    async fn current_lib(&self) -> Result<u64, AdapterError> {
        // TODO: open an EndpointInfo.Info call against self.upstream_endpoint.
        Err(AdapterError::NotImplemented)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn chain_id_encodes_1_in_last_byte() {
        let a = EthereumAdapter::new("http://localhost:13042");
        let id = a.chain_id();
        assert_eq!(id[31], 1);
        assert!(id[..31].iter().all(|b| *b == 0));
    }
}
