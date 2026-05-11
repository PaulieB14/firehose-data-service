# Status

Live snapshot of what is real vs. what is a stub. Update this file in the same PR as any change that flips an item.

Last updated: 2026-05-11.

## Contracts

| Item | Status | Notes |
|---|---|---|
| `FirehoseDataService.sol` | **done** | Inherits Horizon `DataService`. `register/start/stop/collect/slash` implemented; routes RAVs through `GraphTallyCollector`. |
| `FirehoseDisputeVerifier.sol` | stub | Phase 3. Design finalized in `docs/dispute-design.md` (#6). Slashing currently no-ops in `FirehoseDataService.slash()`. |
| Foundry config + remappings | **done** | via-IR enabled. |
| `script/Deploy.s.sol` | **done** | Reads `GRAPH_CONTROLLER` / `GRAPH_TALLY_COLLECTOR` / `FIREHOSE_GOVERNANCE` env vars. |
| Unit + integration tests | **done** | 9 tests pass (`forge test`). Integration suite walks the full §5 payment loop (register chain → register indexer → start service → advertise → collect RAV) in-process; same flow against live Anvil via `make devnet`. |
| Vendored Horizon source under `contracts/lib/` | **done** | Trimmed; `scripts/update-horizon.sh` refreshes. |

## mainline-service (Rust)

| Item | Status | Notes |
|---|---|---|
| `build.rs` + vendored `sf.firehose.v2` proto | **done** | |
| `grpc/server.rs` — Stream/Fetch/EndpointInfo | **done** (#3) | Stream.Blocks splices the attestation onto the cursor via `||mainline-att||`; Fetch.Block returns it as `x-mainline-attestation` metadata. EndpointInfo.Info is fed by the active `ChainAdapter`. |
| `attestation/eip712.rs` | **done** | EIP-712 sign over the §2.2 attestation typehash. |
| `billing/tap.rs` | **done** (#4) | Includes `recover_signer`, `EscrowVerifier`, allocation lookup + caching layer, staleness window. 10 tap-specific tests pass. |
| `chain_adapter/mod.rs` + impls | **done** | `BlockFingerprint` trait method (default zeros — chain-specific overrides land per chain). `chain_name`, `block_id_encoding`, `first_streamable_block` populated. |
| Quality metrics | tracked in `mainline-gateway::quality` | Lives in gateway since it informs routing, not serving. |

Total: 24 / 24 tests pass (21 unit + 3 end-to-end gRPC integration: `tests/grpc_end_to_end.rs` boots a mock firehose-core upstream, a real `MainlineService`, and a real tonic client; drives Stream.Blocks + Fetch.Block through real network sockets; verifies attestation cursor splice + metadata header + EIP-712 signer recovery).

## mainline-gateway (Rust)

| Item | Status | Notes |
|---|---|---|
| Operator discovery via network subgraph | **done** (#7) | `OperatorPool::replace_from_json` parses the subgraph response; refresh loop in `main.rs`. |
| Tier-2 quorum routing | **done** (#7) | `quorum::run_fetch_quorum` returns `Decided` / `NoMajority`; minorities partitioned out. Error responses do not count toward majority. |
| Quality scoring | **done** | `quality::QualityMetrics` with sliding window across latency / throughput / completeness / availability. |
| gRPC proxy surface | not started | Reuses `pool`/`quality`/`quorum`; follow-on issue. |

Total: 6 / 6 tests pass.

## mainline-sdk

| Item | Status | Notes |
|---|---|---|
| Rust crate `mainline_sdk` | **done** (#8) | `cursor`, `tap_signer`, `attestation`, `client::{Client, OperatorPool}`. Transport-agnostic — consumers wire tonic / grpc-web themselves. |
| TypeScript package | **done** (#8) | Same surface as Rust. `@noble/curves` + `@noble/hashes` for secp256k1 + keccak. |
| Byte-compat with `mainline-service` wire format | **done** | Receipt + attestation packed forms match `mainline-service::billing::tap::encode_receipt` and `mainline-service::grpc::server::encode_attestation`. |

Total: 14 / 14 Rust tests pass; TS typecheck clean.

## subgraph

| Item | Status | Notes |
|---|---|---|
| `subgraph.yaml` | **done** | Event signatures match deployed contract; placeholder address + startBlock pending Phase 0 deploy. |
| `schema.graphql` | **done** | Operator, Chain, AdvertisedLib, PaymentEvent, SlashEvent, DestinationChange. |
| Mappings | **done** (#5) | All 8 handlers implemented. `graph codegen && graph build` exits 0. |

## CI

| Item | Status | Notes |
|---|---|---|
| `.github/workflows/ci.yml` | **done** | rust check+test, foundry build+test (mandatory), typescript typecheck, subgraph codegen+build. |

## Open architectural questions (carried from GRC §6)

- StreamingFast patch maintenance dependency (geth-firehose, firesol).
- Chain-specific Tier-1 dispute verifiers — design exists for Ethereum L1 in `docs/dispute-design.md`; L2/Solana deferred.
- Bandwidth economics for operators in expensive-egress regions.
- Cursor portability assumes operators maintain a ForkDB deep enough to resume — partially mitigated by the `MAINLINE_CURSOR_UNRESUMABLE` failover protocol on the SDK side.

## What's *actually compilable* right now

- `contracts/`: `forge build` and `forge test` exit 0. 9/9 tests pass (4 integration + 5 unit).
- `mainline-service/`: `cargo check` + `cargo test` exit 0. 24/24 tests pass (21 unit + 3 end-to-end gRPC integration).
- `mainline-gateway/`: `cargo check` + `cargo test` exit 0. 6/6 tests pass.
- `mainline-sdk/rust/`: `cargo test` exits 0. 14/14 tests pass.
- `mainline-sdk/typescript/`: `npx tsc --noEmit` exits 0.
- `subgraph/`: `npm install && npx graph codegen && npx graph build` exits 0.

## What's still open (not implementation-blocked)

- Phase 0 deploy of `FirehoseDataService` to Arbitrum Sepolia and re-pointing the subgraph (#9). Step-by-step in [`docs/phase-0-runbook.md`](phase-0-runbook.md).
- Live integration test: one Mainline operator + 1000-block consumer pull end-to-end (#9). Same runbook.
- The reference dispute-watcher binary outlined in `docs/dispute-design.md` (#6 follow-on).
- gRPC surface on the gateway that exposes sf.firehose.v2 to consumers and proxies to selected operators — reuses the existing pool/quorum/quality core.
