//! mainline-service — indexer-side daemon for GRC-006 (Mainline).

use std::env;
use std::net::SocketAddr;
use tonic::transport::Server;
use tracing::info;
use tracing_subscriber::EnvFilter;

use mainline_service::grpc::firehose::{
    stream_server::StreamServer,
    fetch_server::FetchServer,
    endpoint_info_server::EndpointInfoServer,
};
use mainline_service::grpc::server::MainlineService;

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    tracing_subscriber::fmt()
        .with_env_filter(EnvFilter::try_from_default_env().unwrap_or_else(|_| EnvFilter::new("info")))
        .init();

    let listen: SocketAddr = env::var("MAINLINE_LISTEN")
        .unwrap_or_else(|_| "0.0.0.0:13050".to_string())
        .parse()?;
    let upstream = env::var("MAINLINE_UPSTREAM").unwrap_or_else(|_| "http://127.0.0.1:13042".to_string());

    // Placeholder chain_id (Ethereum mainnet) + zero key. Real config wiring is a TODO.
    let mut chain_id = [0u8; 32];
    chain_id[31] = 1;
    let key = [0u8; 32];

    let svc = MainlineService::new(upstream.clone(), chain_id, key);

    info!("mainline-service listening on {listen}, upstream={upstream}");

    Server::builder()
        .add_service(StreamServer::new(svc.clone_for_stream()))
        .add_service(FetchServer::new(svc.clone_for_fetch()))
        .add_service(EndpointInfoServer::new(svc))
        .serve(listen)
        .await?;

    Ok(())
}
