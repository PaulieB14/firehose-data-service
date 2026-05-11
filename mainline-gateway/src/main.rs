//! mainline-gateway — optional managed gateway for Mainline.
//!
//! Phase 1 deliverable per GRC-006 §2.6 Tier-2 (quorum). This binary stands
//! up the discovery + scoring + quorum core; the actual sf.firehose.v2 RPC
//! surface that proxies to operators is left as a follow-on issue (the
//! quorum/pool/quality logic is reusable in any transport).

use std::env;

use tokio::time::sleep;
use tracing::{info, warn};
use tracing_subscriber::EnvFilter;

use mainline_gateway::pool::OperatorPool;

async fn refresh_loop(pool: &OperatorPool, chain_id: [u8; 32]) {
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
        .with_env_filter(EnvFilter::try_from_default_env().unwrap_or_else(|_| EnvFilter::new("info")))
        .init();

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

    info!(%subgraph_url, chain_id = %hex::encode(chain_id), "mainline-gateway starting");

    let pool = OperatorPool::new(subgraph_url);
    refresh_loop(&pool, chain_id).await;
    Ok(())
}
