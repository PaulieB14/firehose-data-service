//! Per-chain adapters. Each adapter wraps a `firehose-core` gRPC client
//! for a specific chain and exposes a uniform interface to the rest of
//! the service.
//!
//! The adapter is responsible for:
//!   - Opening live + historical streams against the upstream endpoint
//!   - Computing the canonical `payload_hash` for the chain's protobuf type
//!   - Reporting the current LIB for `advertiseChain` calls

pub mod base;
pub mod ethereum;
pub mod solana;

#[derive(thiserror::Error, Debug)]
pub enum AdapterError {
    #[error("adapter not implemented")]
    NotImplemented,
    #[error("upstream firehose-core unavailable")]
    UpstreamUnavailable,
}

pub trait ChainAdapter: Send + Sync {
    fn chain_id(&self) -> [u8; 32];
    fn firehose_proto_type(&self) -> &'static str;
    fn current_lib(&self) -> Result<u64, AdapterError>;
}
