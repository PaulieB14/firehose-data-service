//! mainline-service — indexer-side daemon for GRC-006 (Mainline).
//!
//! Stub entrypoint. See README.md.

use tracing::info;
use tracing_subscriber::EnvFilter;

mod attestation;
mod billing;
mod chain_adapter;
mod grpc;

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    tracing_subscriber::fmt()
        .with_env_filter(EnvFilter::from_default_env())
        .init();

    info!("mainline-service starting (stub)");
    info!("see https://github.com/PaulieB14/firehose-data-service for status");

    // TODO:
    //   1. Load config (operator key, upstream firehose-core endpoint, chain registry).
    //   2. Start gRPC server exposing sf.firehose.v2 on configured port.
    //   3. Spawn LIB-advertisement task that calls advertiseChain() periodically.
    //   4. Spawn TAP RAV aggregation task.

    Err("mainline-service: not implemented".into())
}
