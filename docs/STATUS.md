# Status

Live snapshot of what is real vs. what is a stub. Update this file in the same PR as any change that flips an item.

Last updated: 2026-05-11.

## Contracts

| Item | Status | Notes |
|---|---|---|
| `FirehoseDataService.sol` | **done** | Inherits Horizon `DataService`. `register/startService/stopService/collect/slash` implemented; routes RAVs through `GraphTallyCollector`. |
| `FirehoseDisputeVerifier.sol` skeleton | stub | Phase 3. Design in `docs/dispute-design.md`. |
| Foundry config (`foundry.toml`, `remappings.txt`) | **done** | via-IR enabled to avoid stack-too-deep. |
| `script/Deploy.s.sol` | **done** | Reads `GRAPH_CONTROLLER` / `GRAPH_TALLY_COLLECTOR` / `FIREHOSE_GOVERNANCE` env vars. |
| Unit tests | **done** | 5 tests pass (`forge test`). Covers governance, chain registry, idempotency, unregistered-indexer guards. |
| Integration tests against Horizon staking | not started | Needs Anvil + Horizon devenv. |
| Vendored Horizon source under `contracts/lib/` | **done** | trimmed to horizon + interfaces + oz v5; `scripts/update-horizon.sh` refreshes. |

## mainline-service (Rust)

| Item | Status | Notes |
|---|---|---|
| Cargo crate | scaffolded | |
| `build.rs` + vendored `sf.firehose.v2` proto | **done** | Generates tonic stubs at build time. |
| `grpc/server.rs` — Stream/Fetch/EndpointInfo handlers | stub (real signatures) | Trait impls compile; handler bodies return `Unimplemented`. Implementation tracked in issue #3. |
| `attestation/eip712.rs` — MainlineAttestation signing | **done** | Real EIP-712 domain + struct hash + secp256k1 signing. Unit-tested. |
| `billing/tap.rs` — TAP v2 receipt EIP-712 | **done** | Real digest computation. Signature recovery + escrow check are TODO (issue #4). |
| `chain_adapter/mod.rs` | **done** | `ChainAdapter` trait + default sha256 `payload_hash`. Tested. |
| `chain_adapter/{ethereum,base,solana}.rs` | partial | Chain IDs encoded correctly; `current_lib` is TODO. |
| Quality metrics (latency, throughput, completeness) | not started | Required for §2.5 SLA reporting. |
| TLS termination | wired in tonic features | Not configured in main.rs yet. |

## mainline-gateway (Rust)

| Item | Status | Notes |
|---|---|---|
| Cargo crate | scaffolded | |
| Operator discovery via network subgraph | not started | Depends on `subgraph/` being deployed (issue #7). |
| Tier-2 quorum routing | not started | The primary Phase 1 deliverable (issue #7). |
| Quality-score weighting | not started | |

## mainline-sdk

| Item | Status | Notes |
|---|---|---|
| Rust crate | scaffolded | |
| TypeScript package | scaffolded | |
| `mainline-cursor-v1` encode/decode (Rust) | **done** | Real implementation, unit-tested, byte-compatible with TS. |
| `mainline-cursor-v1` encode/decode (TS) | **done** | Real implementation, byte-compatible with Rust. |
| TAP receipt signing helper | not started | Issue #8. |
| High-level `stream()` API | not started | Issue #8. |

## subgraph

| Item | Status | Notes |
|---|---|---|
| Manifest (`subgraph.yaml`) | **done** | Event signatures match the deployed contract. Placeholder address + startBlock — update on Phase 0 deploy. |
| Schema | **done** | Operator, Chain, AdvertisedLib, PaymentEvent, SlashEvent, DestinationChange. |
| Mappings (AssemblyScript) | **done** | All 8 handlers implemented. `graph codegen && graph build` exits 0. |
| ABI export | **done** | `subgraph/abis/FirehoseDataService.json` populated from `forge` artifact. |

## CI

| Item | Status | Notes |
|---|---|---|
| `.github/workflows/ci.yml` | **done** | Rust check+test, Foundry build+test, TS typecheck, graph build. |

## Open architectural questions (carried from GRC §6)

- StreamingFast patch maintenance dependency (geth-firehose, firesol)
- Chain-specific Tier-1 dispute verifiers
- Bandwidth economics for operators in expensive-egress regions
- Cursor portability assumes operators maintain a ForkDB deep enough to resume

## What's *actually compilable* right now (honest summary)

- `contracts/`: `forge build` and `forge test` both exit 0. 5/5 tests pass.
- `mainline-service/`: `cargo check` should succeed if `protoc` is installed. Tests pass for `attestation::eip712`, `billing::tap`, `chain_adapter` `payload_hash`, and `chain_adapter::ethereum`.
- `mainline-sdk/rust/`: `cargo test` should pass cleanly (no external deps beyond base64 + thiserror).
- `mainline-sdk/typescript/`: `tsc --noEmit` should pass.
- `subgraph/`: `npm install && npx graph codegen && npx graph build` exits 0.
