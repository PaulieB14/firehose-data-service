//! Base adapter. Same geth-firehose patch as Ethereum L1. Stub.

use super::{AdapterError, ChainAdapter};

pub struct BaseAdapter {
    pub upstream_endpoint: String,
}

impl ChainAdapter for BaseAdapter {
    fn chain_id(&self) -> [u8; 32] {
        let mut id = [0u8; 32];
        id[30..32].copy_from_slice(&8453u16.to_be_bytes()); // base mainnet
        id
    }

    fn firehose_proto_type(&self) -> &'static str {
        "sf.ethereum.type.v2.Block"
    }

    fn current_lib(&self) -> Result<u64, AdapterError> {
        Err(AdapterError::NotImplemented)
    }
}
