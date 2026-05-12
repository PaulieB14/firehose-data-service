//! Runnable consumer-side example: stream blocks from a Mainline operator,
//! verify each attestation, exit on count.
//!
//! Usage:
//!   cargo run --example stream_blocks -- \
//!     --operator http://127.0.0.1:13050 \
//!     --operator-address 0x<20-byte-hex> \
//!     --settlement-chain-id 421614 \
//!     --fds-address 0x<20-byte-hex> \
//!     --tap-collector 0x<20-byte-hex> \
//!     --count 10
//!
//! Defaults match the local-devnet setup (`make devnet` in `contracts/`).
//! Run a `mainline-service` instance on :13050 first — easiest path:
//!
//!     cd contracts && make devnet                # deploys FirehoseDataService
//!     cd mainline-service && cargo run --release \
//!       MAINLINE_FDS_ADDRESS=0x...               # from the devnet output
//!       MAINLINE_GRAPH_TALLY_COLLECTOR=0x...     # same
//!
//! Then in another terminal:
//!
//!     cargo run --example stream_blocks
//!
//! This is the same shape any real consumer would use: build + sign a TAP
//! receipt → call `Stream.Blocks` with `x-tap-receipt` metadata → for each
//! response, split the cursor on `||mainline-att||`, parse the packed
//! attestation, recompute the payload_hash locally, EIP-712 verify against
//! the operator's known signing address.

use std::env;
use std::time::{SystemTime, UNIX_EPOCH};

use k256::ecdsa::{signature::hazmat::PrehashSigner, RecoveryId, Signature, SigningKey};
use sha2::{Digest, Sha256};
use tonic::Request;

use mainline_sdk::attestation::{split_cursor, verify_attestation, AttestationDomain};

use mainline_service::billing::tap::{
    digest as tap_digest, encode_receipt, TapDomain, TapReceiptV2,
};
use mainline_service::grpc::firehose::{stream_client::StreamClient, Request as FhRequest};
use mainline_service::grpc::server::TAP_RECEIPT_METADATA_KEY;

struct Args {
    operator_url: String,
    operator_address: [u8; 20],
    settlement_chain_id: u64,
    fds_address: [u8; 20],
    tap_collector: [u8; 20],
    count: usize,
    sender_key: [u8; 32],
}

fn parse_hex_20(s: &str) -> [u8; 20] {
    let stripped = s.strip_prefix("0x").unwrap_or(s);
    let bytes = hex::decode(stripped).expect("hex address");
    assert_eq!(
        bytes.len(),
        20,
        "expected 20-byte address, got {}",
        bytes.len()
    );
    let mut out = [0u8; 20];
    out.copy_from_slice(&bytes);
    out
}

fn parse_args() -> Args {
    let mut a = Args {
        operator_url: env::var("MAINLINE_OPERATOR_URL")
            .unwrap_or_else(|_| "http://127.0.0.1:13050".to_string()),
        // Default operator address derived from the [0x55; 32] dev key (used in
        // the integration test). In a real deploy this is the indexer's
        // signing address pulled from the network subgraph.
        operator_address: parse_hex_20("0x69bcaee69e0ad4e7c0ce15a8c2d34c5d0d3a4d3b"),
        settlement_chain_id: env::var("MAINLINE_SETTLEMENT_CHAIN_ID")
            .ok()
            .and_then(|v| v.parse().ok())
            .unwrap_or(421614),
        fds_address: parse_hex_20("0xabababababababababababababababababababab"),
        tap_collector: parse_hex_20("0xcccccccccccccccccccccccccccccccccccccccc"),
        count: env::var("MAINLINE_BLOCK_COUNT")
            .ok()
            .and_then(|v| v.parse().ok())
            .unwrap_or(10),
        sender_key: [0x66u8; 32],
    };
    // Parse CLI overrides as `--flag value` pairs (simple, dependency-free).
    let argv: Vec<String> = env::args().collect();
    let mut i = 1;
    while i + 1 < argv.len() {
        match argv[i].as_str() {
            "--operator" => a.operator_url = argv[i + 1].clone(),
            "--operator-address" => a.operator_address = parse_hex_20(&argv[i + 1]),
            "--settlement-chain-id" => a.settlement_chain_id = argv[i + 1].parse().unwrap(),
            "--fds-address" => a.fds_address = parse_hex_20(&argv[i + 1]),
            "--tap-collector" => a.tap_collector = parse_hex_20(&argv[i + 1]),
            "--count" => a.count = argv[i + 1].parse().unwrap(),
            _ => {}
        }
        i += 2;
    }
    a
}

fn build_signed_receipt(args: &Args) -> TapReceiptV2 {
    let now_ns = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap()
        .as_nanos() as u64;
    let mut receipt = TapReceiptV2 {
        allocation_id: [0xaa; 20],
        timestamp_ns: now_ns,
        nonce: 1,
        value: 1_000_000, // 1 USDC in 6-decimal units, sized to cover a burst
        signature: vec![],
    };

    let domain = TapDomain {
        settlement_chain_id: args.settlement_chain_id,
        verifying_contract: args.tap_collector,
    };
    let prehash = tap_digest(&domain, &receipt);
    let key = SigningKey::from_bytes(&args.sender_key.into()).unwrap();
    let (sig, rec): (Signature, RecoveryId) = key.sign_prehash(&prehash).unwrap();
    let mut out = Vec::with_capacity(65);
    out.extend_from_slice(&sig.to_bytes());
    out.push(rec.to_byte() + 27);
    receipt.signature = out;
    receipt
}

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    let args = parse_args();
    println!(
        "→ Connecting to {} (expecting operator 0x{})",
        args.operator_url,
        hex::encode(args.operator_address)
    );

    let mut client = StreamClient::connect(args.operator_url.clone()).await?;
    println!("→ gRPC channel up");

    let receipt = build_signed_receipt(&args);
    println!(
        "→ Signed TAP receipt: allocation=0x{}, value={}, settlement-chain={}",
        hex::encode(receipt.allocation_id),
        receipt.value,
        args.settlement_chain_id
    );

    let mut req = Request::new(FhRequest {
        start_block_num: 0,
        cursor: String::new(),
        stop_block_num: 0,
        final_blocks_only: vec![],
        transforms: vec![],
    });
    req.metadata_mut().insert(
        TAP_RECEIPT_METADATA_KEY,
        hex::encode(encode_receipt(&receipt)).parse().unwrap(),
    );

    let response = client.blocks(req).await?;
    let mut stream = response.into_inner();
    println!("→ Stream open, requesting first {} blocks\n", args.count);

    let attestation_domain = AttestationDomain {
        settlement_chain_id: args.settlement_chain_id,
        verifying_contract: args.fds_address,
    };

    let mut received = 0usize;
    let mut last_inner_cursor: Option<String> = None;
    while let Some(item) = stream.message().await? {
        let payload_bytes = item
            .block
            .as_ref()
            .map(|a| a.value.clone())
            .unwrap_or_default();

        // Consumer-side payload_hash recomputation — same sha256 the
        // operator pinned in the attestation.
        let mut h = Sha256::new();
        h.update(&payload_bytes);
        let recomputed_hash: [u8; 32] = h.finalize().into();

        // Split off the attestation suffix and verify EIP-712.
        let (inner_cursor, attestation) = split_cursor(&item.cursor)?;
        verify_attestation(
            &attestation_domain,
            &attestation,
            &args.operator_address,
            Some(&recomputed_hash),
        )?;

        last_inner_cursor = Some(inner_cursor.clone());
        received += 1;
        println!(
            "✔ block #{received:>3}: payload={} bytes, sha256={}…{}, cursor={}",
            payload_bytes.len(),
            &hex::encode(&recomputed_hash[..4]),
            &hex::encode(&recomputed_hash[28..]),
            inner_cursor
        );

        if received >= args.count {
            break;
        }
    }

    println!(
        "\n✔ verified {received} blocks; resume from inner cursor: {}",
        last_inner_cursor.as_deref().unwrap_or("(none)")
    );
    Ok(())
}
