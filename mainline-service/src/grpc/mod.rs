//! gRPC surface — re-exports the upstream sf.firehose.v2 protobuf contract
//! unchanged. Per GRC-006 §2.2 this is non-negotiable.
//!
//! The actual proto files should be vendored from
//! https://github.com/streamingfast/proto and compiled via tonic_build in
//! build.rs (not yet wired).

pub mod server;
