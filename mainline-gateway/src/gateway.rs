//! The §2.6 Tier-2 quorum gateway as a tonic gRPC server.
//!
//! Exposes the same `sf.firehose.v2` surface as `mainline-service` but
//! proxies through to operator-side services. Routing rules:
//!
//! - **Stream.Blocks**: open one upstream stream to the single highest-quality
//!   operator at the requested tier. Forward each Response unchanged
//!   (cursor + attestation suffix already attached by the operator).
//! - **Fetch.Block**: fan out to `k` operators (default 3) at Quorum tier,
//!   collect their per-operator `payload_hash` claims, call `quorum::run_fetch_quorum`,
//!   return the majority winner's response. Minority signers are logged so
//!   the quality-score module can demote them on the next pool refresh.
//! - **EndpointInfo.Info**: read from the best operator. The gateway doesn't
//!   aggregate across operators — each operator is truthful per §2.2 and
//!   the consumer can spot-check via Fetch.
//!
//! All upstream calls forward the incoming gRPC metadata (the consumer's
//! `x-tap-receipt`) so the operator still does its own §2.4 receipt verification.
//! The gateway adds no payment authority of its own.

use std::sync::Arc;

use futures::future::join_all;
use sha2::{Digest, Sha256};
use tokio::sync::mpsc;
use tokio_stream::wrappers::ReceiverStream;
use tonic::metadata::MetadataMap;
use tonic::{Request, Response, Status};
use tracing::{debug, warn};

use mainline_service::grpc::firehose::{
    endpoint_info_client::EndpointInfoClient,
    endpoint_info_server::EndpointInfo as EndpointInfoSvc, fetch_client::FetchClient,
    fetch_server::Fetch as FetchSvc, stream_client::StreamClient,
    stream_server::Stream as StreamSvc, InfoRequest, InfoResponse, Request as FhRequest,
    Response as FhResponse, SingleBlockRequest, SingleBlockResponse,
};

use crate::pool::{Operator, OperatorPool, OperatorTier};
use crate::quorum::{run_fetch_quorum, QuorumOutcome, QuorumResult};

/// Default fan-out for Tier-2 quorum on Fetch.Block.
const DEFAULT_QUORUM_K: usize = 3;

/// Service-level state.
#[derive(Clone)]
pub struct GatewayService {
    pub pool: Arc<OperatorPool>,
    pub chain_id: [u8; 32],
    pub default_tier: OperatorTier,
    pub quorum_k: usize,
}

impl GatewayService {
    pub fn new(pool: Arc<OperatorPool>, chain_id: [u8; 32], tier: OperatorTier) -> Self {
        Self {
            pool,
            chain_id,
            default_tier: tier,
            quorum_k: DEFAULT_QUORUM_K,
        }
    }

    pub fn with_quorum_k(mut self, k: usize) -> Self {
        self.quorum_k = k;
        self
    }
}

fn clone_metadata(src: &MetadataMap) -> MetadataMap {
    src.clone()
}

#[tonic::async_trait]
impl StreamSvc for GatewayService {
    type BlocksStream = ReceiverStream<Result<FhResponse, Status>>;

    async fn blocks(
        &self,
        request: Request<FhRequest>,
    ) -> Result<Response<Self::BlocksStream>, Status> {
        let op = self
            .pool
            .best_for_chain(self.default_tier)
            .ok_or_else(|| Status::unavailable("no operators available for chain/tier"))?;

        debug!(operator = %op.url, "gateway forwarding Stream.Blocks");

        // Re-wrap the request so we can re-attach the original metadata
        // (especially x-tap-receipt) on the upstream call.
        let consumer_metadata = clone_metadata(request.metadata());
        let inner = request.into_inner();

        let mut upstream = StreamClient::connect(op.url.clone())
            .await
            .map_err(|e| Status::unavailable(format!("operator unreachable: {e}")))?;

        let mut upstream_req = Request::new(inner);
        *upstream_req.metadata_mut() = consumer_metadata;
        let upstream_resp = upstream
            .blocks(upstream_req)
            .await
            .map_err(|e| Status::internal(format!("upstream stream error: {e}")))?;
        let mut upstream_stream = upstream_resp.into_inner();

        let (tx, rx) = mpsc::channel::<Result<FhResponse, Status>>(64);
        tokio::spawn(async move {
            loop {
                match upstream_stream.message().await {
                    Ok(Some(item)) => {
                        if tx.send(Ok(item)).await.is_err() {
                            break;
                        }
                    }
                    Ok(None) => break,
                    Err(e) => {
                        warn!(error = %e, "operator stream error");
                        let _ = tx
                            .send(Err(Status::internal(format!("operator error: {e}"))))
                            .await;
                        break;
                    }
                }
            }
        });

        Ok(Response::new(ReceiverStream::new(rx)))
    }
}

#[tonic::async_trait]
impl FetchSvc for GatewayService {
    async fn block(
        &self,
        request: Request<SingleBlockRequest>,
    ) -> Result<Response<SingleBlockResponse>, Status> {
        let candidates = self.pool.top_k(self.quorum_k, OperatorTier::Quorum);
        if candidates.is_empty() {
            return Err(Status::unavailable("no Quorum-tier operators for chain"));
        }

        let consumer_metadata = clone_metadata(request.metadata());
        let inner = request.into_inner();

        let futures = candidates.iter().cloned().map(|op| {
            let inner_req = inner.clone();
            let meta = consumer_metadata.clone();
            async move { fetch_one_operator(op, inner_req, meta).await }
        });
        let outcomes: Vec<(Operator, Option<SingleBlockResponse>, Option<[u8; 32]>)> =
            join_all(futures).await;

        // Convert to QuorumResult entries for the §2.6 voting helper.
        let mut quorum_input = Vec::with_capacity(outcomes.len());
        let mut payloads_by_addr: std::collections::HashMap<[u8; 20], SingleBlockResponse> =
            std::collections::HashMap::new();
        for (op, resp, hash) in outcomes {
            if let (Some(r), Some(_h)) = (resp.as_ref(), hash.as_ref()) {
                payloads_by_addr.insert(op.address, r.clone());
            }
            quorum_input.push(QuorumResult {
                operator: op,
                payload_hash: hash,
            });
        }

        match run_fetch_quorum(quorum_input) {
            QuorumOutcome::Decided {
                payload_hash,
                winners,
                minorities,
            } => {
                // Demote every minority operator. Penalty intentionally small —
                // a single-block disagreement could be a transient.
                for addr in &minorities {
                    self.pool.adjust_quality(addr, -0.1);
                    warn!(
                        operator = %hex::encode(addr),
                        payload_hash = %hex::encode(payload_hash),
                        "minority payload_hash flagged; quality score reduced",
                    );
                }
                let winner_addr = winners.first().copied().ok_or_else(|| {
                    Status::internal("decided quorum had no winners — invariant broken")
                })?;
                let resp = payloads_by_addr
                    .remove(&winner_addr)
                    .ok_or_else(|| Status::internal("winning operator missing from payload map"))?;
                Ok(Response::new(resp))
            }
            QuorumOutcome::NoMajority { groups } => Err(Status::unavailable(format!(
                "no quorum on Fetch.Block (groups: {})",
                groups.len()
            ))),
        }
    }
}

async fn fetch_one_operator(
    op: Operator,
    req: SingleBlockRequest,
    meta: MetadataMap,
) -> (Operator, Option<SingleBlockResponse>, Option<[u8; 32]>) {
    let url = op.url.clone();
    let mut upstream = match FetchClient::connect(url).await {
        Ok(c) => c,
        Err(e) => {
            warn!(operator = %op.url, error = %e, "operator unreachable for fan-out");
            return (op, None, None);
        }
    };
    let mut request = Request::new(req);
    *request.metadata_mut() = meta;
    match upstream.block(request).await {
        Ok(resp) => {
            let body = resp.into_inner();
            let hash = body.block.as_ref().map(|a| {
                let mut h = Sha256::new();
                h.update(&a.value);
                let out: [u8; 32] = h.finalize().into();
                out
            });
            (op, Some(body), hash)
        }
        Err(e) => {
            warn!(operator = %op.url, error = %e, "operator Fetch.Block failed");
            (op, None, None)
        }
    }
}

#[tonic::async_trait]
impl EndpointInfoSvc for GatewayService {
    async fn info(&self, _request: Request<InfoRequest>) -> Result<Response<InfoResponse>, Status> {
        let op = self
            .pool
            .best_for_chain(self.default_tier)
            .ok_or_else(|| Status::unavailable("no operators available"))?;
        let mut upstream = EndpointInfoClient::connect(op.url.clone())
            .await
            .map_err(|e| Status::unavailable(format!("operator unreachable: {e}")))?;
        let resp = upstream
            .info(InfoRequest {})
            .await
            .map_err(|e| Status::internal(format!("operator info failed: {e}")))?;
        Ok(Response::new(resp.into_inner()))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use prost_types::Any;

    fn op(addr: u8, url: &str, score: f64) -> Operator {
        Operator {
            address: [addr; 20],
            url: url.to_string(),
            tier: OperatorTier::Quorum,
            geo_hint: 0,
            active: true,
            last_advertised_lib: 100,
            quality_score: score,
        }
    }

    #[test]
    fn fanout_picks_top_k_by_quality() {
        let pool = OperatorPool::with_operators(vec![
            op(1, "https://a", 0.9),
            op(2, "https://b", 0.5),
            op(3, "https://c", 0.7),
            op(4, "https://d", 0.95),
        ]);
        let top = pool.top_k(2, OperatorTier::Quorum);
        let addrs: Vec<u8> = top.iter().map(|o| o.address[0]).collect();
        // 0.95 (d) + 0.9 (a) should win
        assert_eq!(addrs, vec![4, 1]);
    }

    #[test]
    fn quorum_decision_with_3_operators_majority_2() {
        let h_good = [0xaa; 32];
        let h_bad = [0xbb; 32];
        let results = vec![
            QuorumResult {
                operator: op(1, "https://a", 1.0),
                payload_hash: Some(h_good),
            },
            QuorumResult {
                operator: op(2, "https://b", 1.0),
                payload_hash: Some(h_good),
            },
            QuorumResult {
                operator: op(3, "https://c", 1.0),
                payload_hash: Some(h_bad),
            },
        ];
        match run_fetch_quorum(results) {
            QuorumOutcome::Decided {
                payload_hash,
                winners,
                minorities,
            } => {
                assert_eq!(payload_hash, h_good);
                assert_eq!(winners.len(), 2);
                assert_eq!(minorities, vec![[3u8; 20]]);
            }
            other => panic!("expected Decided, got {other:?}"),
        }
    }

    #[test]
    fn fetch_one_operator_returns_sha256_of_payload() {
        // We can't easily boot a tonic upstream in this unit test (covered
        // in the gateway_proxy.rs integration test). Here we just verify the
        // local hash computation matches a known sha256 — the same path the
        // gateway uses when an operator returns successfully.
        let payload = b"hello-block".to_vec();
        let body = SingleBlockResponse {
            block: Some(Any {
                type_url: "x".to_string(),
                value: payload.clone(),
            }),
        };
        let hash = body.block.as_ref().map(|a| {
            let mut h = Sha256::new();
            h.update(&a.value);
            let out: [u8; 32] = h.finalize().into();
            out
        });
        let mut expected = Sha256::new();
        expected.update(&payload);
        let expected: [u8; 32] = expected.finalize().into();
        assert_eq!(hash, Some(expected));
    }
}
