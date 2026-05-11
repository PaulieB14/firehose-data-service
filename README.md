# firehose-data-service

**Reference implementation scaffold for [GRC-006: Mainline](https://forum.thegraph.com/) — a decentralized Firehose data service on Graph Horizon.**

GRC authored by [@cargopete](https://github.com/cargopete) (Petko Pavlovski). This repository is a community-started implementation effort, intended to be transferred to `graphprotocol/` or handed to the GRC author when ready. All architectural credit belongs to the GRC.

> **Status:** Phase 0 scaffold. No code is production-ready. Contracts are stubs, Rust crates compile but do nothing. The goal of this initial pass is to give the GRC a concrete code surface that contributors can fill in.

---

## What is Mainline?

Mainline is a Horizon data service that serves raw, fork-aware, cursor-resumable Firehose block streams over gRPC. It sits one level below the in-flight Substreams Data Service in the Graph stack: it is the decentralized substrate that Substreams, Subgraphs, Tycho, Token API, and Dispatch all consume.

Indexers stake GRT, provision it to `FirehoseDataService`, register the chains they serve, and get paid per streamed gigabyte and per `Fetch` request via GraphTally (TAP v2). The contract inherits from the same `DataService` base as `SubgraphService`, reusing `HorizonStaking`, `GraphTallyCollector`, and `PaymentsEscrow` unchanged.

The strategic thesis from the GRC: *The Graph cannot credibly market Substreams, Tycho, or any streaming-first data service as "decentralized" while the blocks underneath those products come exclusively from StreamingFast's proprietary endpoint. Mainline fixes that.*

## Repository layout

This mirrors §7.1 of the GRC:

```
firehose-data-service/
├── contracts/                  # Solidity (Hardhat + Foundry)
│   ├── FirehoseDataService.sol
│   ├── FirehoseDisputeVerifier.sol  (Phase 3)
│   └── test/
├── mainline-service/           # Rust — the indexer-side daemon
│   ├── src/
│   │   ├── main.rs
│   │   ├── grpc/               # re-exports sf.firehose.v2 stubs via tonic
│   │   ├── attestation/        # MainlineAttestation signing
│   │   ├── billing/            # TAP receipt verification
│   │   └── chain_adapter/      # pluggable per-chain adapters (eth, sol, ...)
│   └── Cargo.toml
├── mainline-gateway/           # Rust — optional managed gateway
├── mainline-sdk/               # Rust + TypeScript consumer SDKs (TAP signing)
├── subgraph/                   # The Graph network subgraph for Mainline state
└── docs/
```

## Phased rollout (from the GRC)

| Phase | Scope | Target |
|---|---|---|
| 0 — Reference impl | Contract on Arbitrum Sepolia, one operator on Ethereum + Base, full payment loop on testnet | Q1–Q2 2026 |
| 1 — Limited mainnet | Arbitrum One, 3–5 invited operators across 4 chains, Tier-2 quorum verification | Q2–Q3 2026 |
| 2 — General availability | Permissionless operators, bond-based chain registration, subscription pricing | Q4 2026 |
| 3 — Verification tier | `FirehoseDisputeVerifier` for Ethereum L1 + at least one L2, `slash()` activated | Q1–Q2 2027 |

## Reading order for new contributors

1. The GRC itself (link TBD once posted to forum.thegraph.com)
2. [GIP-0066: Graph Horizon](https://forum.thegraph.com/) — the underlying data service framework
3. [GRC-005: Dispatch](https://github.com/cargopete/dispatch) — the sibling data service this work models on
4. [graphprotocol/substreams-data-service](https://github.com/graphprotocol/substreams-data-service) — the closest existing contract template
5. [streamingfast/firehose-core](https://github.com/streamingfast/firehose-core) — the upstream gRPC contract Mainline wraps unchanged

## Contributing

Open. See [`CONTRIBUTING.md`](./CONTRIBUTING.md) and [`docs/STATUS.md`](./docs/STATUS.md) for live status. The most useful contributions right now are:

- Filling in the contract stubs in `contracts/` against the SubgraphService and Substreams DS templates
- Wiring the Rust crates against real `firehose-core` clients
- Writing the network subgraph schema in `subgraph/`
- Reviewing the parameter defaults in `FirehoseDataService.sol` (provision min, stake-to-fees ratio, thawing period)

## License

GPL-2.0-or-later, matching the GRC's contract pragma.
