//! mainline-gateway — optional managed gateway for Mainline. Stub.

use tracing::info;
use tracing_subscriber::EnvFilter;

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    tracing_subscriber::fmt()
        .with_env_filter(EnvFilter::from_default_env())
        .init();

    info!("mainline-gateway starting (stub)");

    // TODO:
    //   1. Subscribe to network subgraph for operator/chain/LIB updates.
    //   2. Maintain a per-chain operator pool sorted by quality score.
    //   3. Expose sf.firehose.v2 surface to consumers; proxy to selected operator.
    //   4. Run periodic Tier-2 quorum checks via Fetch.

    Err("mainline-gateway: not implemented".into())
}
