//! mainline-service crate root. Exposes modules so `main.rs` can wire them
//! and so integration tests can reach into internals.

pub mod attestation;
pub mod billing;
pub mod chain_adapter;
pub mod grpc;

pub use grpc::server::MainlineService;
