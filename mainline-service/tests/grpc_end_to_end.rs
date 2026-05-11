//! End-to-end gRPC integration test.
//!
//! Boots a mock `firehose-core` upstream server, a real `MainlineService`
//! pointed at the mock, and a real tonic client that talks to the service
//! over a TCP socket. Drives both `Stream.Blocks` and `Fetch.Block` round
//! trips, then verifies each attested response by recomputing
//! `payload_hash`, parsing the EIP-712 attestation off the cursor /
//! response metadata, and recovering the signer address — all the way
//! through the same wire format the SDKs use.
//!
//! What this covers that unit tests cannot:
//!   - tonic Server + ReceiverStream streaming behaviour under real
//!     network I/O.
//!   - `x-tap-receipt` and `x-mainline-attestation` metadata transport.
//!   - The cursor splice format (`||mainline-att||<hex>`) end to end.
//!   - Operator key → recovered signer roundtrip via EIP-712.
//!   - Unauthenticated path (missing receipt header).

use std::net::SocketAddr;
use std::sync::Arc;
use std::time::Duration;

use k256::ecdsa::SigningKey;
use prost_types::Any;
use sha2::{Digest, Sha256};
use sha3::Keccak256;
use tokio::sync::mpsc;
use tokio::sync::oneshot;
use tokio::time::sleep;
use tokio_stream::wrappers::ReceiverStream;
use tonic::transport::Channel;
use tonic::transport::Server;
use tonic::{Request, Response, Status};

use mainline_service::attestation::eip712::AttestationDomain;
use mainline_service::billing::tap::{
    encode_receipt, SignatureOnlyVerifier, TapDomain, TapReceiptV2,
};
use mainline_service::chain_adapter::ethereum::EthereumAdapter;
use mainline_service::grpc::firehose::{
    endpoint_info_server::{EndpointInfo as EndpointInfoSvc, EndpointInfoServer},
    fetch_client::FetchClient,
    fetch_server::{Fetch as FetchSvc, FetchServer},
    stream_client::StreamClient,
    stream_server::{Stream as StreamSvc, StreamServer},
    InfoRequest, InfoResponse, Request as FhRequest, Response as FhResponse,
    SingleBlockRequest, SingleBlockResponse,
};
use mainline_service::grpc::server::{
    CURSOR_ATTESTATION_DELIMITER, MainlineService,
    ATTESTATION_METADATA_KEY, TAP_RECEIPT_METADATA_KEY,
};

// We need a TAP signer compatible with the service's verifier. The
// in-tree `mainline-sdk/rust/src/tap_signer.rs` is byte-compatible but
// the service crate doesn't depend on it. We reimplement just the sign
// step inline using the service crate's digest function so the test
// stays self-contained.
use mainline_service::billing::tap::digest as tap_digest;
use k256::ecdsa::{signature::hazmat::PrehashSigner, RecoveryId, Signature};

fn sign_tap(receipt: &mut TapReceiptV2, key: &SigningKey, domain: &TapDomain) {
    let d = tap_digest(domain, receipt);
    let (sig, rec): (Signature, RecoveryId) = key.sign_prehash(&d).expect("sign");
    let mut out = Vec::with_capacity(65);
    out.extend_from_slice(&sig.to_bytes());
    out.push(rec.to_byte() + 27);
    receipt.signature = out;
}

fn keccak(b: &[u8]) -> [u8; 32] {
    let mut h = Keccak256::new();
    h.update(b);
    h.finalize().into()
}

/// Derive the 20-byte address from a SigningKey.
fn key_address(key: &SigningKey) -> [u8; 20] {
    let pt = key.verifying_key().to_encoded_point(false);
    let h = keccak(&pt.as_bytes()[1..]);
    let mut out = [0u8; 20];
    out.copy_from_slice(&h[12..]);
    out
}

// ── Mock firehose-core upstream ────────────────────────────────────────────

struct MockUpstream {
    /// Block payloads to emit on Stream.Blocks. Each becomes one Response.
    stream_payloads: Vec<Vec<u8>>,
    /// Payload to return on Fetch.Block.
    fetch_payload: Vec<u8>,
}

#[tonic::async_trait]
impl StreamSvc for MockUpstream {
    type BlocksStream = ReceiverStream<Result<FhResponse, Status>>;

    async fn blocks(
        &self,
        _request: Request<FhRequest>,
    ) -> Result<Response<Self::BlocksStream>, Status> {
        let (tx, rx) = mpsc::channel(8);
        let payloads = self.stream_payloads.clone();
        tokio::spawn(async move {
            for (i, payload) in payloads.into_iter().enumerate() {
                let resp = FhResponse {
                    block: Some(Any {
                        type_url: "type.googleapis.com/sf.ethereum.type.v2.Block".to_string(),
                        value: payload,
                    }),
                    step: 1, // STEP_NEW
                    cursor: format!("upstream-cursor-{i}"),
                };
                if tx.send(Ok(resp)).await.is_err() {
                    break;
                }
            }
        });
        Ok(Response::new(ReceiverStream::new(rx)))
    }
}

#[tonic::async_trait]
impl FetchSvc for MockUpstream {
    async fn block(
        &self,
        _request: Request<SingleBlockRequest>,
    ) -> Result<Response<SingleBlockResponse>, Status> {
        Ok(Response::new(SingleBlockResponse {
            block: Some(Any {
                type_url: "type.googleapis.com/sf.ethereum.type.v2.Block".to_string(),
                value: self.fetch_payload.clone(),
            }),
        }))
    }
}

#[tonic::async_trait]
impl EndpointInfoSvc for MockUpstream {
    async fn info(
        &self,
        _request: Request<InfoRequest>,
    ) -> Result<Response<InfoResponse>, Status> {
        Ok(Response::new(InfoResponse {
            chain_name: "mock-ethereum".to_string(),
            chain_name_aliases: vec![],
            block_id_encoding: 2,
            block_features: vec![],
            first_streamable_block_num: 0,
            first_streamable_block_id: String::new(),
        }))
    }
}

// ── Test harness ───────────────────────────────────────────────────────────

struct Harness {
    /// Address the MainlineService listens on (TCP).
    pub service_addr: SocketAddr,
    /// Operator's EIP-712 signing key + derived address. Use the address as
    /// `expected_signer` when verifying attestations.
    pub operator_key: SigningKey,
    pub operator_addr: [u8; 20],
    /// TAP domain the service was configured with — receipts must use it.
    pub tap_domain: TapDomain,
    /// Cancel handles so the test can shut everything down.
    _upstream_shutdown: oneshot::Sender<()>,
    _service_shutdown: oneshot::Sender<()>,
}

async fn bind_local() -> (SocketAddr, tokio::net::TcpListener) {
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0")
        .await
        .expect("bind");
    let addr = listener.local_addr().expect("addr");
    (addr, listener)
}

async fn boot_harness(stream_payloads: Vec<Vec<u8>>, fetch_payload: Vec<u8>) -> Harness {
    // 1. Boot the mock upstream firehose-core on an ephemeral port.
    let (upstream_addr, upstream_listener) = bind_local().await;
    let upstream_stream = tokio_stream::wrappers::TcpListenerStream::new(upstream_listener);
    let mock = MockUpstream { stream_payloads, fetch_payload };
    let mock_arc = Arc::new(mock);
    let (upstream_tx, upstream_rx) = oneshot::channel::<()>();
    let upstream_mock = mock_arc.clone();
    tokio::spawn(async move {
        Server::builder()
            .add_service(StreamServer::from_arc(upstream_mock.clone()))
            .add_service(FetchServer::from_arc(upstream_mock.clone()))
            .add_service(EndpointInfoServer::from_arc(upstream_mock))
            .serve_with_incoming_shutdown(upstream_stream, async {
                upstream_rx.await.ok();
            })
            .await
            .ok();
    });

    // 2. Build a real MainlineService pointing at the mock.
    let operator_key = SigningKey::from_bytes(&[0x55u8; 32].into()).expect("key");
    let operator_addr = key_address(&operator_key);

    let tap_domain = TapDomain {
        settlement_chain_id: 421614,
        verifying_contract: [0xcc; 20],
    };
    let attestation_domain = AttestationDomain {
        settlement_chain_id: 421614,
        verifying_contract: [0xab; 20],
    };

    let mut op_bytes = [0u8; 32];
    op_bytes.copy_from_slice(&operator_key.to_bytes());

    let svc = MainlineService::new(
        format!("http://{upstream_addr}"),
        Arc::new(EthereumAdapter::new(format!("http://{upstream_addr}"))),
        Arc::new(SignatureOnlyVerifier),
        tap_domain,
        attestation_domain,
        op_bytes,
    );

    // 3. Boot the service on another ephemeral port.
    let (service_addr, service_listener) = bind_local().await;
    let service_stream = tokio_stream::wrappers::TcpListenerStream::new(service_listener);
    let (service_tx, service_rx) = oneshot::channel::<()>();
    tokio::spawn(async move {
        Server::builder()
            .add_service(StreamServer::new(svc.clone()))
            .add_service(FetchServer::new(svc.clone()))
            .add_service(EndpointInfoServer::new(svc))
            .serve_with_incoming_shutdown(service_stream, async {
                service_rx.await.ok();
            })
            .await
            .ok();
    });

    // Both servers need a beat to start accepting connections.
    sleep(Duration::from_millis(200)).await;

    Harness {
        service_addr,
        operator_key,
        operator_addr,
        tap_domain,
        _upstream_shutdown: upstream_tx,
        _service_shutdown: service_tx,
    }
}

/// Build + sign a TAP receipt and hex-encode it for the metadata header.
fn make_receipt_header(harness: &Harness) -> String {
    use std::time::{SystemTime, UNIX_EPOCH};
    let now_ns = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap()
        .as_nanos() as u64;
    let payer_key = SigningKey::from_bytes(&[0x66u8; 32].into()).unwrap();
    let mut receipt = TapReceiptV2 {
        allocation_id: [0xaa; 20],
        timestamp_ns: now_ns,
        nonce: 1,
        value: 1_000_000,
        signature: vec![],
    };
    sign_tap(&mut receipt, &payer_key, &harness.tap_domain);
    let _ = &harness.operator_key; // silence dead-code for symmetry
    hex::encode(encode_receipt(&receipt))
}

/// Parse the packed attestation bytes back into (chain_id, block_number,
/// block_hash, state_root, payload_hash, sig).
fn parse_packed_attestation(bytes: &[u8]) -> ([u8; 32], u64, [u8; 32], [u8; 32], [u8; 32], [u8; 65]) {
    assert!(bytes.len() >= 201, "attestation packed must be ≥201 bytes");
    let mut chain_id = [0u8; 32];
    chain_id.copy_from_slice(&bytes[0..32]);
    let mut bn = [0u8; 8];
    bn.copy_from_slice(&bytes[32..40]);
    let mut block_hash = [0u8; 32];
    block_hash.copy_from_slice(&bytes[40..72]);
    let mut state_root = [0u8; 32];
    state_root.copy_from_slice(&bytes[72..104]);
    let mut payload_hash = [0u8; 32];
    payload_hash.copy_from_slice(&bytes[104..136]);
    let mut sig = [0u8; 65];
    sig.copy_from_slice(&bytes[136..201]);
    (chain_id, u64::from_be_bytes(bn), block_hash, state_root, payload_hash, sig)
}

/// Recover the signer address from an EIP-712 attestation.
fn verify_attestation_signer(
    domain: &AttestationDomain,
    chain_id: [u8; 32],
    block_number: u64,
    block_hash: [u8; 32],
    state_root: [u8; 32],
    payload_hash: [u8; 32],
    sig: &[u8; 65],
) -> [u8; 20] {
    // Inline the same EIP-712 hashing the service uses.
    fn k(b: &[u8]) -> [u8; 32] {
        let mut h = Keccak256::new();
        h.update(b);
        h.finalize().into()
    }
    let domain_typehash = k(
        b"EIP712Domain(string name,string version,uint256 chainId,address verifyingContract)",
    );
    let name_hash = k(b"Mainline");
    let version_hash = k(b"1");
    let mut chain_word = [0u8; 32];
    chain_word[24..].copy_from_slice(&domain.settlement_chain_id.to_be_bytes());
    let mut contract_word = [0u8; 32];
    contract_word[12..].copy_from_slice(&domain.verifying_contract);
    let mut ds_buf = Vec::with_capacity(160);
    ds_buf.extend_from_slice(&domain_typehash);
    ds_buf.extend_from_slice(&name_hash);
    ds_buf.extend_from_slice(&version_hash);
    ds_buf.extend_from_slice(&chain_word);
    ds_buf.extend_from_slice(&contract_word);
    let ds = k(&ds_buf);

    let att_typehash = k(b"MainlineAttestation(bytes32 chainId,uint64 blockNumber,bytes32 blockHash,bytes32 stateRoot,bytes32 payloadHash)");
    let mut bn_word = [0u8; 32];
    bn_word[24..].copy_from_slice(&block_number.to_be_bytes());
    let mut sh_buf = Vec::with_capacity(192);
    sh_buf.extend_from_slice(&att_typehash);
    sh_buf.extend_from_slice(&chain_id);
    sh_buf.extend_from_slice(&bn_word);
    sh_buf.extend_from_slice(&block_hash);
    sh_buf.extend_from_slice(&state_root);
    sh_buf.extend_from_slice(&payload_hash);
    let sh = k(&sh_buf);

    let mut d_buf = Vec::with_capacity(66);
    d_buf.extend_from_slice(&[0x19, 0x01]);
    d_buf.extend_from_slice(&ds);
    d_buf.extend_from_slice(&sh);
    let prehash = k(&d_buf);

    let raw_v = sig[64];
    let rec = if raw_v >= 27 { raw_v - 27 } else { raw_v };
    let rec_id = k256::ecdsa::RecoveryId::try_from(rec).expect("rec id");
    let signature = k256::ecdsa::Signature::from_slice(&sig[..64]).expect("sig");
    let vk = k256::ecdsa::VerifyingKey::recover_from_prehash(&prehash, &signature, rec_id)
        .expect("recover");
    let pt = vk.to_encoded_point(false);
    let h = k(&pt.as_bytes()[1..]);
    let mut out = [0u8; 20];
    out.copy_from_slice(&h[12..]);
    out
}

// ── Tests ──────────────────────────────────────────────────────────────────

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn stream_blocks_attests_every_response_and_signer_matches_operator() {
    let payloads = vec![b"block-payload-0".to_vec(), b"block-payload-1".to_vec(), b"block-payload-2".to_vec()];
    let harness = boot_harness(payloads.clone(), b"unused".to_vec()).await;

    let endpoint = format!("http://{}", harness.service_addr);
    let channel = Channel::from_shared(endpoint).unwrap().connect().await.expect("connect");
    let mut client = StreamClient::new(channel);

    let mut req = Request::new(FhRequest {
        start_block_num: 0,
        cursor: String::new(),
        stop_block_num: 0,
        final_blocks_only: vec![],
        transforms: vec![],
    });
    let receipt_hex = make_receipt_header(&harness);
    req.metadata_mut()
        .insert(TAP_RECEIPT_METADATA_KEY, receipt_hex.parse().unwrap());

    let resp = client.blocks(req).await.expect("blocks");
    let mut stream = resp.into_inner();

    let mut seen = 0;
    while let Some(item) = stream.message().await.expect("recv") {
        let payload_bytes = item.block.as_ref().expect("block").value.clone();
        assert_eq!(payload_bytes, payloads[seen], "payload pass-through");

        // Cursor splice format check.
        let cursor = item.cursor;
        assert!(
            cursor.contains(CURSOR_ATTESTATION_DELIMITER),
            "cursor missing attestation suffix: {cursor}"
        );
        let parts: Vec<&str> = cursor.split(CURSOR_ATTESTATION_DELIMITER).collect();
        assert_eq!(parts.len(), 2, "exactly one delimiter expected");
        assert_eq!(parts[0], format!("upstream-cursor-{seen}"), "upstream cursor preserved");

        let att_bytes = hex::decode(parts[1]).expect("hex");
        let (chain_id, _bn, _bh, _sr, payload_hash, sig) = parse_packed_attestation(&att_bytes);

        // Payload hash recomputed by consumer.
        let mut h = Sha256::new();
        h.update(&payload_bytes);
        let expected_hash: [u8; 32] = h.finalize().into();
        assert_eq!(payload_hash, expected_hash, "payload_hash mismatch");

        // Chain id matches Ethereum-mainnet encoding (last byte = 1).
        assert_eq!(chain_id[31], 1);
        assert!(chain_id[..31].iter().all(|b| *b == 0));

        // Recover signer; must equal operator address.
        let recovered = verify_attestation_signer(
            &AttestationDomain {
                settlement_chain_id: 421614,
                verifying_contract: [0xab; 20],
            },
            chain_id,
            _bn,
            _bh,
            _sr,
            payload_hash,
            &sig,
        );
        assert_eq!(recovered, harness.operator_addr, "signer mismatch");
        seen += 1;
    }
    assert_eq!(seen, payloads.len(), "all responses delivered");
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn fetch_block_returns_attestation_in_metadata() {
    let payload = b"fetched-block-bytes".to_vec();
    let harness = boot_harness(vec![], payload.clone()).await;

    let endpoint = format!("http://{}", harness.service_addr);
    let channel = Channel::from_shared(endpoint).unwrap().connect().await.expect("connect");
    let mut client = FetchClient::new(channel);

    let mut req = Request::new(SingleBlockRequest {
        reference: None,
        transforms: vec![],
    });
    let receipt_hex = make_receipt_header(&harness);
    req.metadata_mut()
        .insert(TAP_RECEIPT_METADATA_KEY, receipt_hex.parse().unwrap());

    let resp = client.block(req).await.expect("fetch");
    let metadata = resp.metadata().clone();
    let inner = resp.into_inner();
    assert_eq!(
        inner.block.expect("block").value,
        payload,
        "payload pass-through"
    );

    let header = metadata
        .get(ATTESTATION_METADATA_KEY)
        .expect("attestation header missing");
    let att_bytes = hex::decode(header.to_str().unwrap()).expect("hex");
    let (chain_id, _bn, _bh, _sr, payload_hash, sig) = parse_packed_attestation(&att_bytes);

    // Consumer-side payload_hash recomputation matches what the operator signed.
    let mut h = Sha256::new();
    h.update(&payload);
    let expected: [u8; 32] = h.finalize().into();
    assert_eq!(payload_hash, expected);

    let recovered = verify_attestation_signer(
        &AttestationDomain {
            settlement_chain_id: 421614,
            verifying_contract: [0xab; 20],
        },
        chain_id,
        _bn,
        _bh,
        _sr,
        payload_hash,
        &sig,
    );
    assert_eq!(recovered, harness.operator_addr);
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn missing_receipt_returns_unauthenticated() {
    let harness = boot_harness(vec![b"x".to_vec()], b"y".to_vec()).await;
    let endpoint = format!("http://{}", harness.service_addr);
    let channel = Channel::from_shared(endpoint).unwrap().connect().await.expect("connect");
    let mut client = StreamClient::new(channel);

    let req = Request::new(FhRequest::default());
    let err = client.blocks(req).await.expect_err("should fail without receipt");
    assert_eq!(err.code(), tonic::Code::Unauthenticated, "got: {err:?}");
}
