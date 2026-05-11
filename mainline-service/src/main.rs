//! mainline-service — indexer-side daemon for GRC-006 (Mainline).

use std::env;
use std::net::SocketAddr;
use std::sync::Arc;

use tonic::transport::Server;
use tracing::info;
use tracing_subscriber::EnvFilter;

use mainline_service::attestation::eip712::AttestationDomain;
use mainline_service::billing::tap::{SignatureOnlyVerifier, TapDomain};
use mainline_service::chain_adapter::ChainAdapter;
use mainline_service::chain_adapter::base::BaseAdapter;
use mainline_service::chain_adapter::ethereum::EthereumAdapter;
use mainline_service::chain_adapter::solana::SolanaAdapter;
use mainline_service::grpc::firehose::{
    endpoint_info_server::EndpointInfoServer, fetch_server::FetchServer,
    stream_server::StreamServer,
};
use mainline_service::grpc::server::MainlineService;

fn parse_address_20(s: &str) -> Result<[u8; 20], Box<dyn std::error::Error>> {
    let bytes = hex::decode(s.strip_prefix("0x").unwrap_or(s))?;
    if bytes.len() != 20 {
        return Err(format!("expected 20-byte address, got {}", bytes.len()).into());
    }
    let mut out = [0u8; 20];
    out.copy_from_slice(&bytes);
    Ok(out)
}

fn parse_key_32(s: &str) -> Result<[u8; 32], Box<dyn std::error::Error>> {
    let bytes = hex::decode(s.strip_prefix("0x").unwrap_or(s))?;
    if bytes.len() != 32 {
        return Err(format!("expected 32-byte key, got {}", bytes.len()).into());
    }
    let mut out = [0u8; 32];
    out.copy_from_slice(&bytes);
    Ok(out)
}

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    tracing_subscriber::fmt()
        .with_env_filter(
            EnvFilter::try_from_default_env().unwrap_or_else(|_| EnvFilter::new("info")),
        )
        .init();

    let listen: SocketAddr = env::var("MAINLINE_LISTEN")
        .unwrap_or_else(|_| "0.0.0.0:13050".to_string())
        .parse()?;
    let upstream =
        env::var("MAINLINE_UPSTREAM").unwrap_or_else(|_| "http://127.0.0.1:13042".to_string());
    let chain = env::var("MAINLINE_CHAIN").unwrap_or_else(|_| "ethereum".to_string());

    let settlement_chain_id: u64 = env::var("MAINLINE_SETTLEMENT_CHAIN_ID")
        .unwrap_or_else(|_| "421614".to_string()) // Arbitrum Sepolia by default (Phase 0).
        .parse()?;

    let firehose_data_service_addr = env::var("MAINLINE_FDS_ADDRESS")
        .as_deref()
        .map(parse_address_20)
        .unwrap_or_else(|_| Ok([0u8; 20]))?;
    let tally_collector_addr = env::var("MAINLINE_GRAPH_TALLY_COLLECTOR")
        .as_deref()
        .map(parse_address_20)
        .unwrap_or_else(|_| Ok([0u8; 20]))?;
    let operator_key = env::var("MAINLINE_OPERATOR_KEY")
        .as_deref()
        .map(parse_key_32)
        .unwrap_or_else(|_| Ok([0u8; 32]))?;

    let adapter: Arc<dyn ChainAdapter> = match chain.as_str() {
        "ethereum" => Arc::new(EthereumAdapter::new(upstream.clone())),
        "base" => Arc::new(BaseAdapter::new(upstream.clone())),
        "solana" => Arc::new(SolanaAdapter::new(upstream.clone())),
        other => return Err(format!("unsupported MAINLINE_CHAIN={other}").into()),
    };

    let tap_domain = TapDomain {
        settlement_chain_id,
        verifying_contract: tally_collector_addr,
    };
    let attestation_domain = AttestationDomain {
        settlement_chain_id,
        verifying_contract: firehose_data_service_addr,
    };

    let svc = MainlineService::new(
        upstream.clone(),
        adapter,
        Arc::new(SignatureOnlyVerifier),
        tap_domain,
        attestation_domain,
        operator_key,
    );

    info!(
        listen = %listen,
        upstream = %upstream,
        chain = %chain,
        "mainline-service starting"
    );

    Server::builder()
        .add_service(StreamServer::new(svc.clone()))
        .add_service(FetchServer::new(svc.clone()))
        .add_service(EndpointInfoServer::new(svc))
        .serve(listen)
        .await?;

    Ok(())
}
