# GRC-006 Mainline — implementation progress

A phase-by-phase checklist of where this reference implementation stands against the [GRC-006 spec](https://forum.thegraph.com/t/grc-006-mainline-a-firehose-data-service-on-horizon/6920). Designed to be read top-to-bottom; each item links to the spec section and the implementing artifact.

For per-component detail see [`STATUS.md`](STATUS.md). For the testnet deployment runbook see [`phase-0-runbook.md`](phase-0-runbook.md). For the Phase 3 dispute verifier design see [`dispute-design.md`](dispute-design.md).

---

## Phase 0 — Reference implementation (Q1–Q2 2026 per GRC)

> **Exit criterion (§5):** *Full payment loop demonstrated on testnet; at least one external consumer (graph-node dev stack) pulling blocks.*

### Code (complete)

- [x] `FirehoseDataService.sol` inherits Horizon `DataService` and routes RAVs through `GraphTallyCollector` ([commit `ee886aa`](https://github.com/PaulieB14/firehose-data-service/commit/ee886aa))
- [x] GRC-006 §2.1 parameter defaults wired (25,000 GRT min provision, 21-day thawing, 50% max verifier cut)
- [x] Chain registry with Phase-1 governance allowlist (§2.3), LIB regression guard (§2.5)
- [x] Foundry build + unit tests pass; 5 unit tests + 4 integration tests cover the §5 happy path
- [x] `make devnet` brings up Anvil + the full payment loop in one command
- [x] `mainline-service` daemon: Stream.Blocks, Fetch.Block, EndpointInfo.Info implemented ([commit `5d45fe3`](https://github.com/PaulieB14/firehose-data-service/commit/5d45fe3))
- [x] EIP-712 `MainlineAttestation` signing per §2.2; cursor splice format `||mainline-att||` for streaming, `x-mainline-attestation` metadata header for unary
- [x] TAP v2 receipt verification per §2.4: signature recovery + allocation→payer match + escrow check + 5-min staleness window
- [x] `mainline-cursor-v1` (§2.7) implemented in Rust and TypeScript, byte-compatible
- [x] Ethereum `ChainAdapter::fingerprint()` decodes `sf.ethereum.types.v2.Block` and extracts `block_number`/`block_hash`/`state_root` ([commit `d84c303`](https://github.com/PaulieB14/firehose-data-service/commit/d84c303))
- [x] Network subgraph: schema, manifest, all 8 event handlers ([commit `ee886aa`](https://github.com/PaulieB14/firehose-data-service/commit/ee886aa))
- [x] Consumer SDK in Rust + TypeScript, transport-agnostic ([commit `5d45fe3`](https://github.com/PaulieB14/firehose-data-service/commit/5d45fe3))
- [x] Runnable consumer example (Rust): `cargo run --example stream_blocks` ([commit `d84c303`](https://github.com/PaulieB14/firehose-data-service/commit/d84c303))
- [x] Runnable consumer example (TypeScript): `npx tsx examples/stream_blocks.ts` ([commit `bba5e77`](https://github.com/PaulieB14/firehose-data-service/commit/bba5e77))
- [x] CI mandatory across all six pipelines (Rust × 3, Foundry, TS, subgraph) — see badges in [README](../README.md)
- [x] `cargo clippy --all-targets -- -D warnings` mandatory in CI ([commit `42f6260`](https://github.com/PaulieB14/firehose-data-service/commit/42f6260), follow-up [commit `b1c5d98`](https://github.com/PaulieB14/firehose-data-service/commit/b1c5d98))
- [x] End-to-end gRPC integration tests for the indexer-service hot path ([commit `7324db4`](https://github.com/PaulieB14/firehose-data-service/commit/7324db4))

### Operational (pending — needs Paul + indexer coordination)

- [x] Deploy `FirehoseDataService` to Arbitrum Sepolia — live at [`0xD9242fa6…b98c77`](https://sepolia.arbiscan.io/address/0xD9242fa6Eed1aBFD649C7ee868B1eD37DAb98c77), deploy block 268,383,630. Ethereum-mainnet chain manifest registered; 8/8 on-chain verification checks pass.
- [x] `subgraph/subgraph.yaml` `address` + `startBlock` pointed at the live deployment.
- [ ] Deploy the network subgraph to Subgraph Studio (needs a Studio API token).
- [ ] Stand up one `mainline-service` operator on Ethereum L1 (or Base) with a real `firehose-ethereum` upstream.
- [ ] Pull 1,000 blocks end-to-end with attestations from one external consumer.

When the four boxes above are checked, GRC-006 §5 is satisfied.

---

## Phase 1 — Limited mainnet (Q2–Q3 2026 per GRC)

> **Goal:** Arbitrum One, 3–5 invited operators across 4 chains, Tier-2 quorum verification.

### Code (complete)

- [x] Gateway operator discovery via the network subgraph ([commit `5d45fe3`](https://github.com/PaulieB14/firehose-data-service/commit/5d45fe3))
- [x] Tier-2 quorum routing: `run_fetch_quorum` returns Decided / NoMajority, partitions minorities, never crowns the error bucket
- [x] Quality scoring: sliding-window across latency / throughput / completeness / availability (§2.5)
- [x] gRPC proxy surface on `mainline-gateway`: Stream.Blocks forwards to best operator, Fetch.Block fans out top-k, runs quorum, returns majority winner, demotes byzantine ([commit `d84c303`](https://github.com/PaulieB14/firehose-data-service/commit/d84c303))
- [x] Byzantine-quorum end-to-end integration test: 3 mock operators (2 honest + 1 byzantine) → gateway proves majority wins + minority is demoted

### Operational (pending Phase 0 close-out)

- [ ] Identify 3–5 invited operators willing to run on Arbitrum One mainnet
- [ ] Deploy `FirehoseDataService` to Arbitrum One
- [ ] Onboard each operator (register, advertise initial LIB, start service)
- [ ] Run the gateway on infra accessible to invited consumers

---

## Phase 2 — General availability (Q4 2026 per GRC)

- [ ] Permissionless operator registration (replace governance allowlist)
- [ ] Bond-based chain registration (§2.3 Phase 2 model)
- [ ] Subscription pricing alongside per-call TAP receipts
- [ ] Reference watcher binary (described at the bottom of [`dispute-design.md`](dispute-design.md))

---

## Phase 3 — Verification tier (Q1–Q2 2027 per GRC)

### Design (complete)

- [x] T1 dispute verifier design — chose path (b): slash only on consensus-bound (`blockHash`, `stateRoot`) disagreement, not protobuf encoding bugs ([`dispute-design.md`](dispute-design.md), [commit `5d45fe3`](https://github.com/PaulieB14/firehose-data-service/commit/5d45fe3))
- [x] Bond size (10,000 GRT) and dispute window (21 days) decided
- [x] `IFirehoseDisputeVerifier` ABI sketched
- [x] Beacon-header oracle interface specified as the canonical header source

### Implementation

- [x] **Implement `FirehoseDisputeVerifier.sol` against an `IBeaconHeaderOracle`** — skeleton complete, full bond/escrow/oracle/slash-delegation paths wired; production swap is a real beacon-header oracle + non-zero slash amount.
- [x] **Wire `FirehoseDataService.slash()` to delegate to the verifier** — `slash()` reverts unless `msg.sender == disputeVerifier`; delegates to `_graphStaking().slash()` with the verifier as `verifierDestination`. Governance-gated `setDisputeVerifier()`.
- [x] **Chain-specific fingerprint overrides for L2s** — Arbitrum One (chain_id 42161) and Base (chain_id 8453) adapters now decode `sf.ethereum.type.v2.Block` via a shared `decode_evm_block_fingerprint` helper, so T1 disputes can bind `block_hash` + `state_root` on either L2 ([commit `83da9ce`](https://github.com/PaulieB14/firehose-data-service/commit/83da9ce))
- [ ] Off-chain watcher binary that listens for `ChainAdvertised`, samples `Fetch.Block` against an honest oracle, files disputes on mismatch
- [ ] Live `IBeaconHeaderOracle` implementation (SSZ relay posting canonical Ethereum L1 headers); paired L2 header sources (Arbitrum sequencer-anchored, Base / OP-stack)
- [ ] Solana fingerprint override — deferred per design (different proto, no shared decode path)

---

## How to read the test count

The header badge tracks total workspace tests:

| Component | Count | Source |
|---|---|---|
| `contracts/` | 37 | `cd contracts && forge test` (5 unit + 4 integration + 9 dispute-verifier + 3 slash-wiring + 16 production-EIP-712 payment from PR #11) |
| `mainline-service/` | 36 | `cd mainline-service && cargo test` (25 unit + 8 L2 fingerprint + 3 gRPC e2e) |
| `mainline-gateway/` | 12 | `cd mainline-gateway && cargo test` (9 unit + 3 byzantine-quorum e2e) |
| `mainline-sdk/rust/` | 14 | `cd mainline-sdk/rust && cargo test` |
| `mainline-sdk/typescript/` | typecheck-only | `cd mainline-sdk/typescript && npx tsc --noEmit` (example runnable via `npx tsx`) |
| `subgraph/` | build-only | `cd subgraph && npx graph codegen && npx graph build` |
| **Total** | **99** | Plus 3 build-only pipelines, all in CI |

## Where things live

```
firehose-data-service/
├── contracts/                FirehoseDataService.sol + Anvil devnet + tests
├── mainline-service/         indexer daemon (gRPC, EIP-712, TAP, chain adapters)
│   ├── proto/                vendored sf.firehose.v2 + sf.ethereum.types.v2
│   ├── examples/             stream_blocks.rs — runnable consumer
│   └── tests/                grpc_end_to_end.rs
├── mainline-gateway/         §2.6 Tier-2 quorum gateway
│   └── tests/                gateway_proxy.rs — byzantine-quorum integration
├── mainline-sdk/
│   ├── rust/                 cursor, tap_signer, attestation, client
│   └── typescript/           same surface, byte-compat
│       └── examples/         stream_blocks.ts — transport-agnostic consumer
├── subgraph/                 network subgraph (operator/chain/LIB/payments)
├── docs/
│   ├── PROGRESS.md           ← you are here
│   ├── STATUS.md             per-component detail
│   ├── phase-0-runbook.md    operational steps for §5 exit criterion
│   ├── dispute-design.md     Phase 3 T1 dispute design (path b)
│   └── architecture.md       higher-level architecture overview
└── Makefile                  `make devnet`, `make test`, `make build`
```
