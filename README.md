# firehose-data-service

[![ci](https://github.com/PaulieB14/firehose-data-service/actions/workflows/ci.yml/badge.svg?branch=main)](https://github.com/PaulieB14/firehose-data-service/actions/workflows/ci.yml)
[![tests](https://img.shields.io/badge/tests-63%20passing-brightgreen)](docs/STATUS.md)
[![phase](https://img.shields.io/badge/code-Phase%200%20%2B%201%20complete-blue)](docs/PROGRESS.md)
[![license](https://img.shields.io/badge/license-GPL--2.0--or--later-orange)](LICENSE)

**Reference implementation for [GRC-006: Mainline](https://forum.thegraph.com/t/grc-006-mainline-a-firehose-data-service-on-horizon/6920) — a decentralized Firehose data service on Graph Horizon.**

GRC authored by [@cargopete](https://github.com/cargopete) (Petko Pavlovski). This repository is a community-started implementation effort, intended to be transferred to `graphprotocol/` or handed to the GRC author when ready. All architectural credit belongs to the GRC.

> **Status:** Phase 0 / Phase 1 implementation complete in code. The contract compiles + tests green; the indexer service signs and serves attested Firehose responses; the gateway runs Tier-2 quorum; the SDKs verify attestations end-to-end. Phase-0 testnet *deployment* is still pending (operational steps in [`docs/phase-0-runbook.md`](docs/phase-0-runbook.md)). Phase-3 dispute verifier is design-only ([`docs/dispute-design.md`](docs/dispute-design.md)). See [`docs/STATUS.md`](docs/STATUS.md) for the live per-component breakdown.

---

## What is Mainline?

Mainline is a Horizon data service that serves raw, fork-aware, cursor-resumable Firehose block streams over gRPC. It sits one level below the in-flight Substreams Data Service in the Graph stack: it is the decentralized substrate that Substreams, Subgraphs, Tycho, Token API, and Dispatch all consume.

Indexers stake GRT, provision it to `FirehoseDataService`, register the chains they serve, and get paid per streamed gigabyte and per `Fetch` request via GraphTally (TAP v2). The contract inherits from the same `DataService` base as `SubgraphService`, reusing `HorizonStaking`, `GraphTallyCollector`, and `PaymentsEscrow` unchanged.

The strategic thesis from the GRC: *The Graph cannot credibly market Substreams, Tycho, or any streaming-first data service as "decentralized" while the blocks underneath those products come exclusively from StreamingFast's proprietary endpoint. Mainline fixes that.*

## Repository layout

This mirrors §7.1 of the GRC:

```
firehose-data-service/
├── contracts/                  # Solidity (Foundry)
│   ├── FirehoseDataService.sol      # done — inherits Horizon DataService
│   ├── FirehoseDisputeVerifier.sol  # stub; design in docs/dispute-design.md (Phase 3)
│   ├── script/Deploy.s.sol
│   ├── test/                        # 5 unit tests pass
│   └── lib/                         # vendored forge-std, OZ v5, trimmed @graphprotocol/contracts
├── mainline-service/           # Rust — the indexer-side daemon
│   ├── src/grpc/server.rs           # Stream/Fetch/EndpointInfo — done
│   ├── src/attestation/eip712.rs    # MainlineAttestation signing
│   ├── src/billing/tap.rs           # TAP v2 verify + recover_signer + EscrowVerifier
│   └── src/chain_adapter/           # per-chain adapters (ethereum, base, solana)
├── mainline-gateway/           # Rust — Tier-2 quorum gateway
│   ├── src/pool.rs                  # operator discovery via the subgraph
│   ├── src/quality.rs               # §2.5 sliding-window quality metrics
│   └── src/quorum.rs                # k-of-N payload_hash voting
├── mainline-sdk/               # Consumer SDKs (Rust + TypeScript)
│   ├── rust/                        # cursor, tap_signer, attestation, client
│   └── typescript/                  # mirror, byte-compat with Rust + service
├── subgraph/                   # Mainline network subgraph (operator/chain/LIB/payments)
└── docs/
    ├── architecture.md
    ├── dispute-design.md       # Phase 3 design
    ├── phase-0-runbook.md      # operational steps to close out the meta-issue
    └── STATUS.md               # live per-component status
```

## What's implemented

All seven implementation issues from the original tracking sweep are closed. **63 tests pass across the workspace**, including end-to-end gRPC integration tests that boot real tonic servers + clients over TCP for both the indexer service (attestation hot path) and the gateway (Tier-2 quorum vote against a byzantine operator). The contract layer has a `make devnet` target that brings up a live Anvil node with the full payment loop in one command; `mainline-service/examples/stream_blocks.rs` is a runnable consumer example that verifies attestations using the SDK.

| Component | Build | Tests |
|---|---|---|
| `contracts/` | `forge build` ✔ | 9 / 9 (5 unit + 4 integration) |
| `mainline-service/` | `cargo check` ✔ | 28 / 28 (25 unit + 3 gRPC integration) |
| `mainline-gateway/` | `cargo check` ✔ | 12 / 12 (9 unit + 3 byzantine-quorum integration) |
| `mainline-sdk/rust/` | `cargo check` ✔ | 14 / 14 |
| `mainline-sdk/typescript/` | `tsc --noEmit` ✔ | — |
| `subgraph/` | `graph codegen && graph build` ✔ | — |

CI runs all six pipelines mandatorily.

### Wire format (cross-language)

Three byte-exact formats span the service, the gateway, and both SDKs:

- **TAP receipt** — 118 bytes, hex-encoded into the `x-tap-receipt` gRPC metadata header. See `mainline-service::billing::tap::encode_receipt`, mirrored in `mainline-sdk/{rust,typescript}/tap_signer`.
- **MainlineAttestation** — 201 bytes packed (chain_id 32 ‖ block_number 8 ‖ block_hash 32 ‖ state_root 32 ‖ payload_hash 32 ‖ sig 65). For Stream.Blocks it rides as a hex suffix on `Response.cursor` separated by the sentinel `||mainline-att||`. For Fetch.Block it rides as the `x-mainline-attestation` response metadata header.
- **mainline-cursor-v1** — base64url of `chainIdShort 4 ‖ libNum 8 ‖ libHash 32 ‖ headNum 8 ‖ headHash 32 ‖ forkStepsSeenVarint`, portable across operators per §2.7.

## Phased rollout (from the GRC)

| Phase | Scope | Status |
|---|---|---|
| 0 — Reference impl | Contract on Arbitrum Sepolia, one operator on Ethereum + Base, full payment loop on testnet | code: ✔ — deploy still pending ([runbook](docs/phase-0-runbook.md)) |
| 1 — Limited mainnet | Arbitrum One, 3–5 invited operators across 4 chains, Tier-2 quorum verification | gateway code: ✔ — operational rollout pending |
| 2 — General availability | Permissionless operators, bond-based chain registration, subscription pricing | not started |
| 3 — Verification tier | `FirehoseDisputeVerifier` for Ethereum L1 + at least one L2, `slash()` activated | design: ✔ ([`docs/dispute-design.md`](docs/dispute-design.md)) — implementation not started |

## Reading order for new contributors

1. The GRC: [GRC-006: Mainline](https://forum.thegraph.com/t/grc-006-mainline-a-firehose-data-service-on-horizon/6920)
2. [GIP-0066: Graph Horizon](https://forum.thegraph.com/) — the underlying data service framework
3. [GRC-005: Dispatch](https://github.com/cargopete/dispatch) — the sibling data service this work models on
4. [graphprotocol/substreams-data-service](https://github.com/graphprotocol/substreams-data-service) — the closest existing contract template
5. [streamingfast/firehose-core](https://github.com/streamingfast/firehose-core) — the upstream gRPC contract Mainline wraps unchanged

## Contributing

See [`CONTRIBUTING.md`](CONTRIBUTING.md) and [`docs/STATUS.md`](docs/STATUS.md). The highest-leverage next pieces:

- Execute the Phase-0 testnet deployment in [`docs/phase-0-runbook.md`](docs/phase-0-runbook.md).
- Per-chain `ChainAdapter::fingerprint` overrides (Ethereum first) so T1 disputes can land in Phase 3.
- Reference watcher binary described at the end of [`docs/dispute-design.md`](docs/dispute-design.md).
- gRPC surface on `mainline-gateway` that proxies `sf.firehose.v2` to operators, reusing the existing pool/quality/quorum core.

## License

GPL-2.0-or-later, matching the GRC's contract pragma.
