//! Per-chain adapters.
//!
//! Each adapter wraps a `firehose-core` gRPC client for a specific chain
//! and exposes a uniform interface to the rest of the service.

pub mod base;
pub mod ethereum;
pub mod solana;

use sha2::{Digest, Sha256};

#[derive(thiserror::Error, Debug)]
pub enum AdapterError {
    #[error("adapter not implemented")]
    NotImplemented,
    #[error("upstream firehose-core unavailable")]
    UpstreamUnavailable,
    #[error("decode error: {0}")]
    Decode(String),
}

#[async_trait::async_trait]
pub trait ChainAdapter: Send + Sync {
    fn chain_id(&self) -> [u8; 32];
    fn firehose_proto_type(&self) -> &'static str;

    /// Current last irreversible block, for advertiseChain() publishing.
    async fn current_lib(&self) -> Result<u64, AdapterError>;

    /// Canonical payload hash used in MainlineAttestation. Default: sha256
    /// of the raw protobuf bytes. Per §2.2, this is `sha256(payload)`.
    fn payload_hash(&self, payload: &[u8]) -> [u8; 32] {
        let mut h = Sha256::new();
        h.update(payload);
        h.finalize().into()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    struct Dummy;
    #[async_trait::async_trait]
    impl ChainAdapter for Dummy {
        fn chain_id(&self) -> [u8; 32] { [0; 32] }
        fn firehose_proto_type(&self) -> &'static str { "dummy" }
        async fn current_lib(&self) -> Result<u64, AdapterError> { Ok(0) }
    }

    #[test]
    fn payload_hash_is_stable_sha256() {
        let a = Dummy;
        let h1 = a.payload_hash(b"hello");
        let h2 = a.payload_hash(b"hello");
        let h3 = a.payload_hash(b"world");
        assert_eq!(h1, h2);
        assert_ne!(h1, h3);
        // Known sha256("hello") prefix check.
        assert_eq!(hex::encode(&h1[..4]), "2cf24dba");
    }
}
