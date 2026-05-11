# mainline-service

Indexer-side daemon. One process per operator.

## What it does (eventually)

For every block emitted by the upstream `firehose-core` gRPC endpoint on the operator's box:

1. Validates the TAP receipt presented by the consumer for the active stream
2. Signs a `MainlineAttestation` over `(chainId, blockNumber, blockHash, payloadHash)`
3. Attaches the attestation as a gRPC trailer
4. Forwards the block to the consumer
5. Periodically publishes `advertiseChain(chainId, lib)` on-chain
6. Periodically aggregates TAP receipts into RAVs and submits via `indexer-tap-agent`

## What it does today

Nothing. `cargo build` succeeds; `cargo run` panics. Every handler is `todo!()`.

## Layout

```
src/
├── main.rs              # tokio entry point
├── grpc/
│   ├── mod.rs           # re-exports sf.firehose.v2 generated stubs
│   └── server.rs        # Stream/Fetch/EndpointInfo handlers (stub)
├── attestation/
│   ├── mod.rs
│   └── eip712.rs        # MainlineAttestation domain + signing (stub)
├── billing/
│   ├── mod.rs
│   └── tap.rs           # TAP v2 receipt verification (stub)
└── chain_adapter/
    ├── mod.rs
    ├── ethereum.rs
    ├── solana.rs
    └── base.rs
```

## Why Rust

Matches Dispatch, matches GRC-004's push on Rust as a first-class language, matches `indexer-service-rs` and `indexer-tap-agent` v2.0.0. The upstream Firehose stack is Go; we are not reimplementing it, we are wrapping it.
