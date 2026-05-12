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

/// Per-block attestation fingerprint. Anchors the three §2.6 verification
/// tiers: T2 (quorum) compares `payload_hash` across operators; T1 (proof-
/// backed) re-derives `block_hash` / `state_root` against a canonical header.
#[derive(Debug, Clone, Default)]
pub struct BlockFingerprint {
    pub block_number: u64,
    pub block_hash: [u8; 32],
    pub state_root: [u8; 32],
}

/// Mirrors the `InfoResponse.BlockIdEncoding` enum from sf.firehose.v2.
#[derive(Debug, Clone, Copy)]
#[repr(u64)]
pub enum BlockIdEncoding {
    Unset = 0,
    Hex = 1,
    ZeroXHex = 2,
    Base58 = 3,
    Base64 = 4,
    Base64Url = 5,
}

#[async_trait::async_trait]
pub trait ChainAdapter: Send + Sync {
    fn chain_id(&self) -> [u8; 32];
    fn firehose_proto_type(&self) -> &'static str;

    /// Public chain name surfaced by EndpointInfo.Info per §2.2.
    fn chain_name(&self) -> &'static str;

    /// `InfoResponse.first_streamable_block_num` for the chain (genesis for
    /// most chains; non-zero for chains with an instrumented-fork cutover).
    fn first_streamable_block(&self) -> u64 {
        0
    }

    /// Encoding hint for block ids in EndpointInfo.Info. Default 0x-hex
    /// matches Ethereum-family chains; Solana adapter overrides to base58.
    fn block_id_encoding(&self) -> BlockIdEncoding {
        BlockIdEncoding::ZeroXHex
    }

    /// Current last irreversible block, for advertiseChain() publishing.
    async fn current_lib(&self) -> Result<u64, AdapterError>;

    /// Canonical payload hash used in MainlineAttestation. Default: sha256
    /// of the raw protobuf bytes. Per §2.2, this is `sha256(payload)`.
    fn payload_hash(&self, payload: &[u8]) -> [u8; 32] {
        let mut h = Sha256::new();
        h.update(payload);
        h.finalize().into()
    }

    /// Decode the chain-specific `Block` protobuf and pull out the fields
    /// the attestation binds: block number, block hash, state root.
    ///
    /// Default returns zeros — adequate for §2.6 T2 (quorum) verification
    /// since that tier only requires `payload_hash` consensus. T1 verifiers
    /// require chain-specific overrides.
    fn fingerprint(&self, _payload: &[u8]) -> Result<BlockFingerprint, AdapterError> {
        Ok(BlockFingerprint::default())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    struct Dummy;
    #[async_trait::async_trait]
    impl ChainAdapter for Dummy {
        fn chain_id(&self) -> [u8; 32] {
            [0; 32]
        }
        fn firehose_proto_type(&self) -> &'static str {
            "dummy"
        }
        fn chain_name(&self) -> &'static str {
            "dummy"
        }
        async fn current_lib(&self) -> Result<u64, AdapterError> {
            Ok(0)
        }
    }

    #[test]
    fn payload_hash_is_stable_sha256() {
        let a = Dummy;
        let h1 = a.payload_hash(b"hello");
        let h2 = a.payload_hash(b"hello");
        let h3 = a.payload_hash(b"world");
        assert_eq!(h1, h2);
        assert_ne!(h1, h3);
        assert_eq!(hex::encode(&h1[..4]), "2cf24dba");
    }

    #[test]
    fn fingerprint_default_is_zeros() {
        let a = Dummy;
        let fp = a.fingerprint(b"any").unwrap();
        assert_eq!(fp.block_number, 0);
        assert_eq!(fp.block_hash, [0u8; 32]);
        assert_eq!(fp.state_root, [0u8; 32]);
    }
}
