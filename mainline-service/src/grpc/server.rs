//! gRPC handlers for the three services Mainline exposes:
//!   - sf.firehose.v2.Stream      (historical + live, cursor-resumable)
//!   - sf.firehose.v2.Fetch       (single-block lookup)
//!   - sf.firehose.v2.EndpointInfo (chain/range advertisement)
//!
//! All handlers are stubs.

use tonic::{Request, Response, Status};

#[allow(dead_code)]
pub struct MainlineStreamServer {
    // TODO: hold a handle to the upstream firehose-core gRPC client,
    // the operator's signing key, the TAP receipt verifier, and the
    // active chain adapters.
}

impl MainlineStreamServer {
    pub fn new() -> Self {
        Self {}
    }

    /// Stream.Blocks — long-lived, cursor-resumable, fork-aware.
    /// Per §2.2, billing happens by egress bytes via TAP receipts.
    pub async fn blocks(
        &self,
        _request: Request<()>,
    ) -> Result<Response<()>, Status> {
        // TODO: 1. extract+verify TAP receipt from metadata
        //       2. open upstream stream against firehose-core
        //       3. for each block: sign MainlineAttestation, attach as trailer,
        //          increment byte counter, forward to consumer
        //       4. on STEP_UNDO: bill at same per-byte rate as STEP_NEW (§2.7)
        Err(Status::unimplemented("Stream.Blocks not implemented"))
    }

    /// Fetch.Block — single-block lookup. Per-block pricing (§2.4).
    pub async fn fetch_block(
        &self,
        _request: Request<()>,
    ) -> Result<Response<()>, Status> {
        Err(Status::unimplemented("Fetch.Block not implemented"))
    }

    /// EndpointInfo.Info — must be truthful and refreshed on every block (§2.2).
    pub async fn info(
        &self,
        _request: Request<()>,
    ) -> Result<Response<()>, Status> {
        Err(Status::unimplemented("EndpointInfo.Info not implemented"))
    }
}

impl Default for MainlineStreamServer {
    fn default() -> Self {
        Self::new()
    }
}
