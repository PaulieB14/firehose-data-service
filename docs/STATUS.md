# Status

Live snapshot of what is real vs. what is a stub. Update this file in the same PR as any change that flips an item.

Last updated: 2026-05-11.

## Contracts

| Item | Status | Notes |
|---|---|---|
| `FirehoseDataService.sol` | **done** | Inherits Horizon `DataService`. `register/start/stop/collect/slash` implemented; routes RAVs through `GraphTallyCollector`. |
| `FirehoseDisputeVerifier.sol` | **skeleton done** | Per `docs/dispute-design.md`: bond escrow (10k GRT), 21-day window, 1-hour min resolution delay, oracle-driven settle, slash delegation. Production-real swap: a real `IBeaconHeaderOracle` (SSZ relay) + non-zero `slashAmount`. 9 forge tests cover happy + dismissed + revert paths. |
| `FirehoseDataService.slash()` wired | **done** | Reverts unless `disputeVerifier` is set + `msg.sender == disputeVerifier`. Delegates to `_graphStaking().slash(...)` with the dispute verifier as `verifierDestination`. Governance setter `setDisputeVerifier()`. 3 forge tests cover the auth surface. |
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
| `chain_adapter/mod.rs` + impls | **done** | `BlockFingerprint` trait method. Ethereum adapter now decodes `sf.ethereum.types.v2.Block` and extracts `(block_number, block_hash, state_root)` via a vendored header-view proto. `chain_name`, `block_id_encoding`, `first_streamable_block` populated for all three chains. |
| Quality metrics | tracked in `mainline-gateway::quality` | Lives in gateway since it informs routing, not serving. |
| Runnable consumer example | **done** | `examples/stream_blocks.rs` — connects via tonic to a live operator, signs a TAP receipt, pulls N blocks, verifies each attestation via `mainline-sdk`. Mirrors a real consumer's loop. |

Total: 28 / 28 tests pass (25 unit + 3 end-to-end gRPC integration: `tests/grpc_end_to_end.rs` boots a mock firehose-core upstream, a real `MainlineService`, and a real tonic client; drives Stream.Blocks + Fetch.Block through real network sockets; verifies attestation cursor splice + metadata header + EIP-712 signer recovery).

## mainline-gateway (Rust)

| Item | Status | Notes |
|---|---|---|
| Operator discovery via network subgraph | **done** (#7) | `OperatorPool::replace_from_json` parses the subgraph response; refresh loop in `main.rs`. |
| Tier-2 quorum routing | **done** (#7) | `quorum::run_fetch_quorum` returns `Decided` / `NoMajority`; minorities partitioned out. Error responses do not count toward majority. |
| Quality scoring | **done** | `quality::QualityMetrics` with sliding window across latency / throughput / completeness / availability. |
| gRPC proxy surface | **done** | `gateway::GatewayService` exposes the full sf.firehose.v2 surface. Stream.Blocks forwards to the best operator; Fetch.Block fans out to top-k Quorum-tier operators, runs `run_fetch_quorum`, returns majority winner's payload, demotes minorities. TAP receipts pass through unchanged. |

Total: 12 / 12 tests pass (9 unit + 3 end-to-end byzantine-quorum integration: `tests/gateway_proxy.rs` boots 3 mock operators where 1 is byzantine, the gateway, and a real tonic client; verifies the majority payload wins and the byzantine operator's quality score is demoted).

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
- `mainline-service/`: 28/28 tests pass (25 unit + 3 gRPC integration). Example: `cargo run --example stream_blocks`.
- `mainline-gateway/`: 12/12 tests pass (9 unit + 3 byzantine-quorum integration).
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
