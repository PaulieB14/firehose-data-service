//! mainline-service crate root. Exposes modules so `main.rs` can wire them
//! and so integration tests can reach into internals.

pub mod attestation;
pub mod billing;
pub mod chain_adapter;
pub mod grpc;

use crate::grpc::server::MainlineService;

impl MainlineService {
    // Cheap clones: the inner fields are Copy or cheap to clone.
    pub fn clone_for_stream(&self) -> Self {
        Self {
            upstream_endpoint: self.upstream_endpoint.clone(),
            chain_id: self.chain_id,
            operator_key: self.operator_key,
        }
    }
    pub fn clone_for_fetch(&self) -> Self {
        self.clone_for_stream()
    }
}
