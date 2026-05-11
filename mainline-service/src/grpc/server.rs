//! gRPC handlers for sf.firehose.v2 Stream/Fetch/EndpointInfo.
//!
//! Implementation strategy:
//!   1. Hold a tonic Client to the local firehose-core endpoint (port 13042).
//!   2. On Stream.Blocks: open upstream stream, verify TAP receipt from
//!      metadata, for each Response{block,step,cursor} sign a
//!      MainlineAttestation and pass the response through.
//!   3. On Fetch.Block: proxy to upstream, sign attestation, return.
//!   4. On EndpointInfo.Info: read from local chain adapter; truthful per §2.2.
//!
//! Stubs below have real signatures + the right return types, ready for
//! handler bodies. The generated tonic traits live in
//! `crate::grpc::firehose` after build.rs runs.

use tonic::{Request, Response, Status};
use tokio_stream::wrappers::ReceiverStream;

use crate::grpc::firehose::{
    stream_server::Stream as StreamSvc,
    fetch_server::Fetch as FetchSvc,
    endpoint_info_server::EndpointInfo as EndpointInfoSvc,
    Request as FhRequest, Response as FhResponse,
    SingleBlockRequest, SingleBlockResponse,
    InfoRequest, InfoResponse,
};

pub struct MainlineService {
    /// Local firehose-core endpoint, e.g. http://127.0.0.1:13042
    pub upstream_endpoint: String,
    /// Chain id this service instance is bound to.
    pub chain_id: [u8; 32],
    /// Operator signing key.
    pub operator_key: [u8; 32],
}

impl MainlineService {
    pub fn new(upstream_endpoint: String, chain_id: [u8; 32], operator_key: [u8; 32]) -> Self {
        Self { upstream_endpoint, chain_id, operator_key }
    }
}

#[tonic::async_trait]
impl StreamSvc for MainlineService {
    type BlocksStream = ReceiverStream<Result<FhResponse, Status>>;

    async fn blocks(
        &self,
        _request: Request<FhRequest>,
    ) -> Result<Response<Self::BlocksStream>, Status> {
        // TODO:
        //   1. let receipt = crate::billing::tap::extract_receipt(request.metadata())?;
        //      crate::billing::tap::verify(&receipt)?;
        //   2. let mut upstream = firehose_client::FirehoseClient::connect(self.upstream_endpoint.clone()).await?;
        //   3. let inner = request.into_inner();
        //   4. let mut upstream_stream = upstream.blocks(inner).await?.into_inner();
        //   5. let (tx, rx) = tokio::sync::mpsc::channel(64);
        //      tokio::spawn(async move {
        //          while let Some(item) = upstream_stream.message().await.transpose() {
        //              let signed = sign_and_attach(item, &self.operator_key, self.chain_id);
        //              tx.send(signed).await.ok();
        //          }
        //      });
        //      Ok(Response::new(ReceiverStream::new(rx)))
        Err(Status::unimplemented("Stream.Blocks not yet implemented"))
    }
}

#[tonic::async_trait]
impl FetchSvc for MainlineService {
    async fn block(
        &self,
        _request: Request<SingleBlockRequest>,
    ) -> Result<Response<SingleBlockResponse>, Status> {
        Err(Status::unimplemented("Fetch.Block not yet implemented"))
    }
}

#[tonic::async_trait]
impl EndpointInfoSvc for MainlineService {
    async fn info(
        &self,
        _request: Request<InfoRequest>,
    ) -> Result<Response<InfoResponse>, Status> {
        // TODO: read chain_name, first_streamable_block, encoding from the
        // active chain adapter.
        Err(Status::unimplemented("EndpointInfo.Info not yet implemented"))
    }
}
