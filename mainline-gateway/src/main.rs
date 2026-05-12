//! mainline-gateway — managed gateway for Mainline.
//!
//! Phase 1 deliverable per GRC-006 §2.6 Tier-2 (quorum). This binary stands
//! up:
//!   - the operator pool (refreshed from the network subgraph),
//!   - the §2.6 Tier-2 quorum + quality scoring core,
//!   - and the consumer-facing sf.firehose.v2 gRPC surface that proxies
//!     to operators with quorum-vote on Fetch.Block.
//!
//! Env vars:
//!   MAINLINE_LISTEN              gRPC listen address (default 0.0.0.0:13060)
//!   MAINLINE_NETWORK_SUBGRAPH    subgraph URL for operator discovery
//!   MAINLINE_CHAIN_ID            chain to gateway, as 0x-prefixed bytes32
//!                                (default = Ethereum mainnet = 0x...01)
//!   MAINLINE_QUORUM_K            Fetch.Block fan-out (default 3)

use std::env;
use std::net::SocketAddr;
use std::sync::Arc;

use tokio::time::sleep;
use tonic::transport::Server;
use tracing::{info, warn};
use tracing_subscriber::EnvFilter;

use mainline_gateway::gateway::GatewayService;
use mainline_gateway::pool::{OperatorPool, OperatorTier};

use mainline_service::grpc::firehose::{
    endpoint_info_server::EndpointInfoServer, fetch_server::FetchServer,
    stream_server::StreamServer,
};

async fn refresh_loop(pool: Arc<OperatorPool>, chain_id: [u8; 32]) {
    let interval = pool.refresh_interval;
    loop {
        match fetch_operators_json(&pool.network_subgraph_url).await {
            Ok(json) => match pool.replace_from_json(&json, &chain_id) {
                Ok(n) => info!(operators = n, "refreshed operator pool"),
                Err(e) => warn!(error = %e, "subgraph parse failed"),
            },
            Err(e) => warn!(error = %e, "subgraph fetch failed"),
        }
        sleep(interval).await;
    }
}

async fn fetch_operators_json(url: &str) -> Result<String, Box<dyn std::error::Error>> {
    let query = serde_json::json!({
        "query": "{ operators(where: { active: true }) { id url tier geoHint active chains { chain { id } lib } } }"
    });
    let client = reqwest::Client::new();
    let resp = client.post(url).json(&query).send().await?;
    let body = resp.text().await?;
    Ok(body)
}

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    tracing_subscriber::fmt()
        .with_env_filter(
            EnvFilter::try_from_default_env().unwrap_or_else(|_| EnvFilter::new("info")),
        )
        .init();

    let listen: SocketAddr = env::var("MAINLINE_LISTEN")
        .unwrap_or_else(|_| "0.0.0.0:13060".to_string())
        .parse()?;
    let subgraph_url = env::var("MAINLINE_NETWORK_SUBGRAPH").unwrap_or_else(|_| {
        "https://api.thegraph.com/subgraphs/name/paulieb14/mainline-network".to_string()
    });
    let chain_str = env::var("MAINLINE_CHAIN_ID").unwrap_or_else(|_| {
        "0x0000000000000000000000000000000000000000000000000000000000000001".to_string()
    });
    let mut chain_id = [0u8; 32];
    let bytes = hex::decode(chain_str.strip_prefix("0x").unwrap_or(&chain_str))?;
    if bytes.len() != 32 {
        return Err("MAINLINE_CHAIN_ID must be a 32-byte hex string".into());
    }
    chain_id.copy_from_slice(&bytes);

    let quorum_k: usize = env::var("MAINLINE_QUORUM_K")
        .ok()
        .and_then(|v| v.parse().ok())
        .unwrap_or(3);

    info!(
        %listen, %subgraph_url, chain_id = %hex::encode(chain_id), quorum_k,
        "mainline-gateway starting"
    );

    let pool = Arc::new(OperatorPool::new(subgraph_url));
    let pool_for_refresh = pool.clone();

    // Refresh loop runs in the background — keep it alive for the lifetime
    // of the binary.
    tokio::spawn(async move {
        refresh_loop(pool_for_refresh, chain_id).await;
    });

    let svc = GatewayService::new(pool, chain_id, OperatorTier::Quorum).with_quorum_k(quorum_k);

    Server::builder()
        .add_service(StreamServer::new(svc.clone()))
        .add_service(FetchServer::new(svc.clone()))
        .add_service(EndpointInfoServer::new(svc))
        .serve(listen)
        .await?;
    Ok(())
}
