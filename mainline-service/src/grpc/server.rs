//! gRPC handlers for sf.firehose.v2 Stream/Fetch/EndpointInfo.
//!
//! Phase 0 wire design:
//!   1. Every request must carry a TAP v2 receipt in the gRPC metadata
//!      header `x-tap-receipt` (hex-encoded via `tap::encode_receipt`).
//!   2. The handlers connect to the indexer's local firehose-core endpoint
//!      (default `http://127.0.0.1:13042`) and proxy the upstream Stream /
//!      Fetch / EndpointInfo surface, computing and signing a
//!      `MainlineAttestation` per response.
//!   3. Stream.Blocks attaches each per-block attestation by appending its
//!      hex-encoded packed form to the upstream `cursor` field, using the
//!      sentinel `CURSOR_ATTESTATION_DELIMITER`. The SDK splits the cursor
//!      on this sentinel before resuming.
//!   4. Fetch.Block attaches the attestation as response metadata under
//!      `x-mainline-attestation` (unary RPCs can carry per-call metadata
//!      cleanly).
//!
//! See GRC-006 §2.2 (sf.firehose.v2 exposed unchanged), §2.4 (TAP receipt
//! per-burst pricing), §2.6 (attestation surface for verification tiers).

use std::sync::Arc;

use tokio_stream::wrappers::ReceiverStream;
use tonic::{Request, Response, Status};
use tracing::{debug, warn};

use crate::attestation::eip712::{self, AttestationDomain};
use crate::attestation::MainlineAttestation;
use crate::billing::tap::{self, ReceiptVerifier, TapDomain, TapError};
use crate::chain_adapter::ChainAdapter;
use crate::grpc::firehose::{
    endpoint_info_server::EndpointInfo as EndpointInfoSvc,
    fetch_client::FetchClient,
    fetch_server::Fetch as FetchSvc,
    stream_client::StreamClient,
    stream_server::Stream as StreamSvc,
    InfoRequest, InfoResponse, Request as FhRequest, Response as FhResponse,
    SingleBlockRequest, SingleBlockResponse,
};

/// Delimiter used to splice the hex-encoded attestation onto the upstream
/// cursor for streaming responses. The mainline-sdk strips this suffix
/// before calling `mainline_cursor::decode`.
pub const CURSOR_ATTESTATION_DELIMITER: &str = "||mainline-att||";

/// Metadata header carrying the hex-encoded attestation on unary responses
/// (Fetch.Block).
pub const ATTESTATION_METADATA_KEY: &str = "x-mainline-attestation";

/// Metadata header consumers MUST set on every Stream.Blocks / Fetch.Block
/// request, carrying a hex-encoded `TapReceiptV2` (`tap::encode_receipt`).
pub const TAP_RECEIPT_METADATA_KEY: &str = "x-tap-receipt";

/// Service-level state shared across all RPC handlers. Cheap to clone
/// because every field is either `Arc<_>` or `Copy`.
#[derive(Clone)]
pub struct MainlineService {
    /// Local firehose-core endpoint, e.g. http://127.0.0.1:13042.
    pub upstream_endpoint: String,
    /// Active chain adapter. Drives chain-id, proto type, current LIB and
    /// per-payload fingerprint decoding.
    pub adapter: Arc<dyn ChainAdapter>,
    /// TAP receipt verifier. `SignatureOnlyVerifier` for tests; the escrow
    /// verifier (issue #4) for production.
    pub verifier: Arc<dyn ReceiptVerifier>,
    /// TAP EIP-712 domain (settlement chain + GraphTallyCollector address).
    pub tap_domain: TapDomain,
    /// EIP-712 domain for MainlineAttestation (settlement chain + the
    /// deployed FirehoseDataService address).
    pub attestation_domain: AttestationDomain,
    /// Operator signing key (secp256k1, 32 bytes).
    pub operator_key: [u8; 32],
}

impl MainlineService {
    pub fn new(
        upstream_endpoint: String,
        adapter: Arc<dyn ChainAdapter>,
        verifier: Arc<dyn ReceiptVerifier>,
        tap_domain: TapDomain,
        attestation_domain: AttestationDomain,
        operator_key: [u8; 32],
    ) -> Self {
        Self {
            upstream_endpoint,
            adapter,
            verifier,
            tap_domain,
            attestation_domain,
            operator_key,
        }
    }
}

async fn verify_request_receipt<R>(
    request: &Request<R>,
    domain: &TapDomain,
    verifier: &dyn ReceiptVerifier,
) -> Result<(), Status> {
    let header = request
        .metadata()
        .get(TAP_RECEIPT_METADATA_KEY)
        .ok_or_else(|| Status::unauthenticated("missing x-tap-receipt"))?;
    let hex_str = header
        .to_str()
        .map_err(|_| Status::invalid_argument("x-tap-receipt is not ascii"))?;
    let bytes = hex::decode(hex_str)
        .map_err(|_| Status::invalid_argument("x-tap-receipt is not valid hex"))?;
    let receipt = tap::decode_receipt(&bytes)
        .map_err(|e| Status::invalid_argument(format!("malformed tap receipt: {e}")))?;
    verifier.verify(domain, &receipt).await.map_err(|e| match e {
        TapError::InvalidSignature => Status::unauthenticated("invalid tap signature"),
        TapError::InsufficientEscrow => Status::failed_precondition("insufficient escrow"),
        TapError::WrongAllocation => Status::failed_precondition("wrong allocation"),
        TapError::StaleTimestamp => Status::failed_precondition("stale receipt"),
    })?;
    Ok(())
}

/// Build a signed attestation for one payload + cursor. Returns `(hex_att,
/// cursor_with_attestation_suffix)` so callers can pick the format that
/// fits their RPC shape.
fn build_attestation_and_cursor(
    adapter: &dyn ChainAdapter,
    domain: &AttestationDomain,
    operator_key: &[u8; 32],
    payload_bytes: &[u8],
    upstream_cursor: &str,
) -> Result<(String, String), Status> {
    let fingerprint = adapter.fingerprint(payload_bytes).unwrap_or_default();
    let payload_hash = adapter.payload_hash(payload_bytes);

    let mut attestation = MainlineAttestation::new(
        adapter.chain_id(),
        fingerprint.block_number,
        fingerprint.block_hash,
        fingerprint.state_root,
        payload_hash,
        upstream_cursor.as_bytes().to_vec(),
    );

    eip712::sign(domain, &mut attestation, operator_key)
        .map_err(|e| Status::internal(format!("attestation signing failed: {e:?}")))?;

    let encoded = encode_attestation_hex(&attestation);
    let augmented_cursor = format!("{upstream_cursor}{CURSOR_ATTESTATION_DELIMITER}{encoded}");
    Ok((encoded, augmented_cursor))
}

/// Deterministic packed encoding for the attestation. Big-endian, concatenated:
///
///   chain_id            (32 bytes)
///   block_number        (8 bytes, u64 BE)
///   block_hash          (32 bytes)
///   state_root          (32 bytes)
///   payload_hash        (32 bytes)
///   signature           (65 bytes, r||s||v)
///
/// Total 201 bytes; hex-encoded to 402 hex chars.
pub fn encode_attestation(att: &MainlineAttestation) -> Vec<u8> {
    let mut out = Vec::with_capacity(201);
    out.extend_from_slice(&att.chain_id);
    out.extend_from_slice(&att.block_number.to_be_bytes());
    out.extend_from_slice(&att.block_hash);
    out.extend_from_slice(&att.state_root);
    out.extend_from_slice(&att.payload_hash);
    out.extend_from_slice(&att.indexer_sig);
    out
}

pub fn encode_attestation_hex(att: &MainlineAttestation) -> String {
    hex::encode(encode_attestation(att))
}

#[tonic::async_trait]
impl StreamSvc for MainlineService {
    type BlocksStream = ReceiverStream<Result<FhResponse, Status>>;

    async fn blocks(
        &self,
        request: Request<FhRequest>,
    ) -> Result<Response<Self::BlocksStream>, Status> {
        verify_request_receipt(&request, &self.tap_domain, self.verifier.as_ref()).await?;

        let inner = request.into_inner();

        let mut upstream = StreamClient::connect(self.upstream_endpoint.clone())
            .await
            .map_err(|e| Status::unavailable(format!("upstream firehose-core unreachable: {e}")))?;

        let mut upstream_stream = upstream
            .blocks(inner)
            .await
            .map_err(|e| Status::internal(format!("upstream Stream.Blocks failed: {e}")))?
            .into_inner();

        let (tx, rx) = tokio::sync::mpsc::channel::<Result<FhResponse, Status>>(64);

        let adapter = self.adapter.clone();
        let attestation_domain = AttestationDomain {
            settlement_chain_id: self.attestation_domain.settlement_chain_id,
            verifying_contract: self.attestation_domain.verifying_contract,
        };
        let operator_key = self.operator_key;

        tokio::spawn(async move {
            loop {
                match upstream_stream.message().await {
                    Ok(Some(mut response)) => {
                        let payload_bytes = response
                            .block
                            .as_ref()
                            .map(|any| any.value.clone())
                            .unwrap_or_default();
                        match build_attestation_and_cursor(
                            adapter.as_ref(),
                            &attestation_domain,
                            &operator_key,
                            &payload_bytes,
                            &response.cursor,
                        ) {
                            Ok((_hex, augmented_cursor)) => {
                                response.cursor = augmented_cursor;
                                if tx.send(Ok(response)).await.is_err() {
                                    break; // consumer dropped
                                }
                            }
                            Err(status) => {
                                let _ = tx.send(Err(status)).await;
                                break;
                            }
                        }
                    }
                    Ok(None) => break, // upstream stream done
                    Err(e) => {
                        warn!(error = %e, "upstream stream error");
                        let _ = tx
                            .send(Err(Status::internal(format!("upstream error: {e}"))))
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
impl FetchSvc for MainlineService {
    async fn block(
        &self,
        request: Request<SingleBlockRequest>,
    ) -> Result<Response<SingleBlockResponse>, Status> {
        verify_request_receipt(&request, &self.tap_domain, self.verifier.as_ref()).await?;

        let inner = request.into_inner();

        let mut upstream = FetchClient::connect(self.upstream_endpoint.clone())
            .await
            .map_err(|e| Status::unavailable(format!("upstream firehose-core unreachable: {e}")))?;

        let upstream_response = upstream
            .block(inner)
            .await
            .map_err(|e| Status::internal(format!("upstream Fetch.Block failed: {e}")))?
            .into_inner();

        let payload_bytes = upstream_response
            .block
            .as_ref()
            .map(|any| any.value.clone())
            .unwrap_or_default();

        let (hex_att, _augmented_cursor) = build_attestation_and_cursor(
            self.adapter.as_ref(),
            &self.attestation_domain,
            &self.operator_key,
            &payload_bytes,
            "",
        )?;

        let mut resp = Response::new(upstream_response);
        resp.metadata_mut().insert(
            ATTESTATION_METADATA_KEY,
            hex_att
                .parse()
                .map_err(|_| Status::internal("attestation hex is invalid metadata value"))?,
        );
        debug!("Fetch.Block: attestation attached");
        Ok(resp)
    }
}

#[tonic::async_trait]
impl EndpointInfoSvc for MainlineService {
    async fn info(
        &self,
        _request: Request<InfoRequest>,
    ) -> Result<Response<InfoResponse>, Status> {
        // Per §2.2 the InfoResponse fields are taken from the chain adapter.
        // We deliberately do not surface the operator-internal `current_lib`
        // here: LIB advertisement lives on-chain (FirehoseDataService.advertiseChain)
        // and in the cursor; mixing it into InfoResponse would let operators
        // publish two different views, which §2.5 forbids.
        let info = InfoResponse {
            chain_name: self.adapter.chain_name().to_string(),
            chain_name_aliases: vec![],
            block_id_encoding: self.adapter.block_id_encoding() as u64,
            block_features: vec![],
            first_streamable_block_num: self.adapter.first_streamable_block(),
            first_streamable_block_id: String::new(),
        };
        Ok(Response::new(info))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::attestation::eip712::AttestationDomain;
    use crate::billing::tap::{SignatureOnlyVerifier, TapDomain, TapReceiptV2};
    use crate::chain_adapter::ethereum::EthereumAdapter;

    fn test_service() -> MainlineService {
        MainlineService::new(
            "http://127.0.0.1:13042".to_string(),
            Arc::new(EthereumAdapter::new("http://127.0.0.1:13042")),
            Arc::new(SignatureOnlyVerifier),
            TapDomain { settlement_chain_id: 42161, verifying_contract: [0xcc; 20] },
            AttestationDomain { settlement_chain_id: 42161, verifying_contract: [0xab; 20] },
            [0x11; 32],
        )
    }

    #[test]
    fn attestation_packed_length_is_201() {
        let mut att = MainlineAttestation::new(
            [1u8; 32], 99, [2u8; 32], [3u8; 32], [4u8; 32], vec![],
        );
        att.indexer_sig = vec![5u8; 65];
        let bytes = encode_attestation(&att);
        assert_eq!(bytes.len(), 201);
        assert_eq!(hex::encode(&bytes).len(), 402);
    }

    #[test]
    fn build_attestation_appends_cursor_delimiter() {
        let svc = test_service();
        let (hex_att, cursor) = build_attestation_and_cursor(
            svc.adapter.as_ref(),
            &svc.attestation_domain,
            &svc.operator_key,
            b"payload-bytes",
            "upstream-cursor",
        )
        .expect("attestation");
        assert!(cursor.starts_with("upstream-cursor"));
        assert!(cursor.contains(CURSOR_ATTESTATION_DELIMITER));
        assert!(cursor.ends_with(&hex_att));
    }

    #[tokio::test]
    async fn missing_receipt_is_unauthenticated() {
        let svc = test_service();
        let req = Request::new(FhRequest::default());
        let result = verify_request_receipt(&req, &svc.tap_domain, svc.verifier.as_ref()).await;
        let status = result.expect_err("expected error");
        assert_eq!(status.code(), tonic::Code::Unauthenticated);
    }

    #[tokio::test]
    async fn valid_receipt_passes_signature_only_verifier() {
        let svc = test_service();
        let receipt = TapReceiptV2 {
            allocation_id: [0xaa; 20],
            timestamp_ns: 1,
            nonce: 1,
            value: 1,
            signature: vec![0x42; 65],
        };
        let bytes = tap::encode_receipt(&receipt);
        let mut req = Request::new(FhRequest::default());
        req.metadata_mut().insert(
            TAP_RECEIPT_METADATA_KEY,
            hex::encode(&bytes).parse().unwrap(),
        );
        let result = verify_request_receipt(&req, &svc.tap_domain, svc.verifier.as_ref()).await;
        assert!(result.is_ok(), "verify failed: {result:?}");
    }
}
