# Status

Live snapshot of what is real vs. what is a stub. Update this file in the same PR as any change that flips an item.

Last updated: 2026-05-11.

## Contracts

| Item | Status | Notes |
|---|---|---|
| `FirehoseDataService.sol` skeleton | stub | Interfaces declared, parameters set per §2.1, events declared. Bodies revert. |
| `FirehoseDisputeVerifier.sol` skeleton | stub | Phase 3. Header proof verification logic not started. |
| Hardhat config | stub | |
| Foundry config | stub | |
| `script/Deploy.s.sol` | stub | Compiles structure; will need `@graphprotocol/horizon` to actually build. |
| Unit tests | not started | |
| Integration tests against Horizon staking | not started | |

## mainline-service (Rust)

| Item | Status | Notes |
|---|---|---|
| Cargo crate | scaffolded | |
| `build.rs` + vendored `sf.firehose.v2` proto | **done** | Generates tonic stubs at build time. |
| `grpc/server.rs` — Stream/Fetch/EndpointInfo handlers | stub (real signatures) | Trait impls compile; handler bodies return `Unimplemented`. |
| `attestation/eip712.rs` — MainlineAttestation signing | **done** | Real EIP-712 domain + struct hash + secp256k1 signing. Unit-tested. |
| `billing/tap.rs` — TAP v2 receipt EIP-712 | **done** | Real digest computation. Signature recovery + escrow check are TODO. |
| `chain_adapter/mod.rs` | **done** | `ChainAdapter` trait + default sha256 `payload_hash`. Tested. |
| `chain_adapter/{ethereum,base,solana}.rs` | partial | Chain IDs encoded correctly; `current_lib` is TODO. |
| Quality metrics (latency, throughput, completeness) | not started | Required for §2.5 SLA reporting. |
| TLS termination | wired in tonic features | Not configured in main.rs yet. |

## mainline-gateway (Rust)

| Item | Status | Notes |
|---|---|---|
| Cargo crate | scaffolded | |
| Operator discovery via network subgraph | not started | Depends on `subgraph/` being deployed. |
| Tier-2 quorum routing | not started | The primary Phase 1 deliverable. |
| Quality-score weighting | not started | |

## mainline-sdk

| Item | Status | Notes |
|---|---|---|
| Rust crate | scaffolded | |
| TypeScript package | scaffolded | |
| `mainline-cursor-v1` encode/decode (Rust) | **done** | Real implementation, unit-tested, byte-compatible with TS. |
| `mainline-cursor-v1` encode/decode (TS) | **done** | Real implementation, byte-compatible with Rust. |
| TAP receipt signing helper | not started | |
| High-level `stream()` API | not started | |

## subgraph

| Item | Status | Notes |
|---|---|---|
| Manifest (`subgraph.yaml`) | wired to events | Placeholder contract address. |
| Schema | done | Operator, Chain, AdvertisedLib, PaymentEvent, SlashEvent. |
| Mappings (AssemblyScript) | stub | Empty handlers; need wiring against generated types. |

## CI

| Item | Status | Notes |
|---|---|---|
| `.github/workflows/ci.yml` | **done** | Rust check+test, Foundry build (allowed to fail until Horizon imports), TS typecheck. |

## Open architectural questions (carried from GRC §6)

- StreamingFast patch maintenance dependency (geth-firehose, firesol)
- Chain-specific Tier-1 dispute verifiers
- Bandwidth economics for operators in expensive-egress regions
- Cursor portability assumes operators maintain a ForkDB deep enough to resume

## What's *actually compilable* right now (honest summary)

- `mainline-service/`: `cargo check` should succeed if `protoc` is installed. Tests pass for `attestation::eip712`, `billing::tap`, `chain_adapter` `payload_hash`, and `chain_adapter::ethereum`.
- `mainline-sdk/rust/`: `cargo test` should pass cleanly (no external deps beyond base64 + thiserror).
- `mainline-sdk/typescript/`: `tsc --noEmit` should pass.
- `contracts/`: `forge build` will fail until `@graphprotocol/horizon` is wired. Expected.
