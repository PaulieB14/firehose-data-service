//! End-to-end gateway integration test.
//!
//! Boots three mock operators (2 honest agree on payload H_good, 1 byzantine
//! returns H_bad), a real GatewayService that points its OperatorPool at all
//! three, and a real tonic client. Drives:
//!   - Fetch.Block: gateway fans out, runs §2.6 quorum, must return H_good.
//!   - Stream.Blocks: gateway forwards to the best operator, must yield
//!     responses unchanged.
//!   - The byzantine operator's quality score must be reduced by the
//!     gateway after a Fetch.Block disagreement.

use std::net::SocketAddr;
use std::sync::Arc;
use std::time::Duration;

use prost_types::Any;
use tokio::sync::mpsc;
use tokio::sync::oneshot;
use tokio::time::sleep;
use tokio_stream::wrappers::ReceiverStream;
use tonic::transport::{Channel, Server};
use tonic::{Request, Response, Status};

use mainline_gateway::gateway::GatewayService;
use mainline_gateway::pool::{Operator, OperatorPool, OperatorTier};

use mainline_service::grpc::firehose::{
    endpoint_info_server::{EndpointInfo as EndpointInfoSvc, EndpointInfoServer},
    fetch_client::FetchClient,
    fetch_server::{Fetch as FetchSvc, FetchServer},
    stream_client::StreamClient,
    stream_server::{Stream as StreamSvc, StreamServer},
    InfoRequest, InfoResponse, Request as FhRequest, Response as FhResponse,
    SingleBlockRequest, SingleBlockResponse,
};

struct MockOperator {
    stream_payloads: Vec<Vec<u8>>,
    fetch_payload: Vec<u8>,
}

#[tonic::async_trait]
impl StreamSvc for MockOperator {
    type BlocksStream = ReceiverStream<Result<FhResponse, Status>>;

    async fn blocks(
        &self,
        _req: Request<FhRequest>,
    ) -> Result<Response<Self::BlocksStream>, Status> {
        let (tx, rx) = mpsc::channel(8);
        let payloads = self.stream_payloads.clone();
        tokio::spawn(async move {
            for (i, p) in payloads.into_iter().enumerate() {
                let _ = tx
                    .send(Ok(FhResponse {
                        block: Some(Any {
                            type_url: "type.googleapis.com/x".into(),
                            value: p,
                        }),
                        step: 1,
                        cursor: format!("op-cursor-{i}"),
                    }))
                    .await;
            }
        });
        Ok(Response::new(ReceiverStream::new(rx)))
    }
}

#[tonic::async_trait]
impl FetchSvc for MockOperator {
    async fn block(
        &self,
        _req: Request<SingleBlockRequest>,
    ) -> Result<Response<SingleBlockResponse>, Status> {
        Ok(Response::new(SingleBlockResponse {
            block: Some(Any {
                type_url: "type.googleapis.com/x".into(),
                value: self.fetch_payload.clone(),
            }),
        }))
    }
}

#[tonic::async_trait]
impl EndpointInfoSvc for MockOperator {
    async fn info(
        &self,
        _req: Request<InfoRequest>,
    ) -> Result<Response<InfoResponse>, Status> {
        Ok(Response::new(InfoResponse {
            chain_name: "mock".to_string(),
            chain_name_aliases: vec![],
            block_id_encoding: 2,
            block_features: vec![],
            first_streamable_block_num: 0,
            first_streamable_block_id: String::new(),
        }))
    }
}

async fn bind_local() -> (SocketAddr, tokio::net::TcpListener) {
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();
    (addr, listener)
}

async fn boot_operator(payloads: Vec<Vec<u8>>, fetch_payload: Vec<u8>) -> (SocketAddr, oneshot::Sender<()>) {
    let (addr, listener) = bind_local().await;
    let stream = tokio_stream::wrappers::TcpListenerStream::new(listener);
    let mock = Arc::new(MockOperator { stream_payloads: payloads, fetch_payload });
    let (tx, rx) = oneshot::channel::<()>();
    let mock_a = mock.clone();
    tokio::spawn(async move {
        Server::builder()
            .add_service(StreamServer::from_arc(mock_a.clone()))
            .add_service(FetchServer::from_arc(mock_a.clone()))
            .add_service(EndpointInfoServer::from_arc(mock_a))
            .serve_with_incoming_shutdown(stream, async {
                rx.await.ok();
            })
            .await
            .ok();
    });
    (addr, tx)
}

#[tokio::test(flavor = "multi_thread", worker_threads = 8)]
async fn fetch_block_quorum_picks_majority_payload_and_demotes_minority() {
    // Two honest operators return the same payload; one byzantine returns
    // different bytes. Gateway must return the majority payload.
    let good_payload = b"agreed-block-bytes".to_vec();
    let bad_payload = b"byzantine-block-bytes".to_vec();

    let (op_a_addr, _kill_a) = boot_operator(vec![], good_payload.clone()).await;
    let (op_b_addr, _kill_b) = boot_operator(vec![], good_payload.clone()).await;
    let (op_c_addr, _kill_c) = boot_operator(vec![], bad_payload.clone()).await;

    // Build the pool with all three known + active.
    let operators = vec![
        Operator {
            address: [0xa1; 20],
            url: format!("http://{op_a_addr}"),
            tier: OperatorTier::Quorum,
            geo_hint: 0,
            active: true,
            last_advertised_lib: 100,
            quality_score: 1.0,
        },
        Operator {
            address: [0xb2; 20],
            url: format!("http://{op_b_addr}"),
            tier: OperatorTier::Quorum,
            geo_hint: 0,
            active: true,
            last_advertised_lib: 100,
            quality_score: 1.0,
        },
        Operator {
            address: [0xc3; 20],
            url: format!("http://{op_c_addr}"),
            tier: OperatorTier::Quorum,
            geo_hint: 0,
            active: true,
            last_advertised_lib: 100,
            quality_score: 1.0,
        },
    ];
    let pool = Arc::new(OperatorPool::with_operators(operators));

    // Boot the gateway.
    let svc = GatewayService::new(pool.clone(), [0u8; 32], OperatorTier::Quorum).with_quorum_k(3);
    let (gw_addr, gw_listener) = bind_local().await;
    let gw_stream = tokio_stream::wrappers::TcpListenerStream::new(gw_listener);
    let (_gw_kill_tx, gw_kill_rx) = oneshot::channel::<()>();
    tokio::spawn(async move {
        Server::builder()
            .add_service(FetchServer::new(svc.clone()))
            .add_service(StreamServer::new(svc.clone()))
            .add_service(EndpointInfoServer::new(svc))
            .serve_with_incoming_shutdown(gw_stream, async {
                gw_kill_rx.await.ok();
            })
            .await
            .ok();
    });

    sleep(Duration::from_millis(200)).await;

    // Client hits the gateway.
    let chan = Channel::from_shared(format!("http://{gw_addr}"))
        .unwrap()
        .connect()
        .await
        .unwrap();
    let mut client = FetchClient::new(chan);
    let resp = client
        .block(Request::new(SingleBlockRequest::default()))
        .await
        .expect("fetch");

    let body = resp.into_inner();
    let payload = body.block.expect("block").value;
    assert_eq!(payload, good_payload, "majority payload returned");

    // Byzantine operator should have been demoted.
    let pool_state = pool.list();
    let bad_score = pool_state
        .iter()
        .find(|o| o.address == [0xc3; 20])
        .expect("bad op present")
        .quality_score;
    assert!(bad_score < 1.0, "byzantine operator must lose quality, got {bad_score}");
}

#[tokio::test(flavor = "multi_thread", worker_threads = 8)]
async fn stream_blocks_proxies_through_best_operator() {
    let payloads = vec![b"s0".to_vec(), b"s1".to_vec(), b"s2".to_vec()];
    let (op_addr, _kill) = boot_operator(payloads.clone(), b"unused".to_vec()).await;

    let operators = vec![Operator {
        address: [0xa1; 20],
        url: format!("http://{op_addr}"),
        tier: OperatorTier::Quorum,
        geo_hint: 0,
        active: true,
        last_advertised_lib: 100,
        quality_score: 1.0,
    }];
    let pool = Arc::new(OperatorPool::with_operators(operators));
    let svc = GatewayService::new(pool, [0u8; 32], OperatorTier::Quorum);

    let (gw_addr, gw_listener) = bind_local().await;
    let gw_stream = tokio_stream::wrappers::TcpListenerStream::new(gw_listener);
    let (_kill_tx, kill_rx) = oneshot::channel::<()>();
    tokio::spawn(async move {
        Server::builder()
            .add_service(StreamServer::new(svc.clone()))
            .add_service(FetchServer::new(svc.clone()))
            .add_service(EndpointInfoServer::new(svc))
            .serve_with_incoming_shutdown(gw_stream, async {
                kill_rx.await.ok();
            })
            .await
            .ok();
    });
    sleep(Duration::from_millis(200)).await;

    let chan = Channel::from_shared(format!("http://{gw_addr}"))
        .unwrap()
        .connect()
        .await
        .unwrap();
    let mut client = StreamClient::new(chan);
    let resp = client
        .blocks(Request::new(FhRequest::default()))
        .await
        .expect("stream");
    let mut stream = resp.into_inner();

    let mut seen = 0;
    while let Some(item) = stream.message().await.expect("recv") {
        let bytes = item.block.expect("block").value;
        assert_eq!(bytes, payloads[seen], "payload passthrough");
        seen += 1;
    }
    assert_eq!(seen, payloads.len(), "all responses delivered");
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn no_operators_returns_unavailable() {
    let pool = Arc::new(OperatorPool::with_operators(vec![]));
    let svc = GatewayService::new(pool, [0u8; 32], OperatorTier::Quorum);

    let (gw_addr, gw_listener) = bind_local().await;
    let gw_stream = tokio_stream::wrappers::TcpListenerStream::new(gw_listener);
    let (_kill_tx, kill_rx) = oneshot::channel::<()>();
    tokio::spawn(async move {
        Server::builder()
            .add_service(FetchServer::new(svc.clone()))
            .add_service(StreamServer::new(svc))
            .serve_with_incoming_shutdown(gw_stream, async {
                kill_rx.await.ok();
            })
            .await
            .ok();
    });
    sleep(Duration::from_millis(150)).await;

    let chan = Channel::from_shared(format!("http://{gw_addr}"))
        .unwrap()
        .connect()
        .await
        .unwrap();
    let mut client = FetchClient::new(chan);
    let err = client
        .block(Request::new(SingleBlockRequest::default()))
        .await
        .expect_err("empty pool must yield Unavailable");
    assert_eq!(err.code(), tonic::Code::Unavailable);
}
