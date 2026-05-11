//! Ethereum L1 adapter. Wraps a firehose-ethereum endpoint. Stub.

use super::{AdapterError, ChainAdapter};

pub struct EthereumAdapter {
    pub upstream_endpoint: String,
}

impl ChainAdapter for EthereumAdapter {
    fn chain_id(&self) -> [u8; 32] {
        let mut id = [0u8; 32];
        id[31] = 1; // mainnet
        id
    }

    fn firehose_proto_type(&self) -> &'static str {
        "sf.ethereum.type.v2.Block"
    }

    fn current_lib(&self) -> Result<u64, AdapterError> {
        Err(AdapterError::NotImplemented)
    }
}
