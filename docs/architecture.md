# Architecture

This document mirrors the topology in GRC-006 §7.2 for the reader who wants a single-page mental model without reading the full GRC.

## The layering

```
┌─────────────────────────────────────────────────────────────┐
│  Instrumented node (geth-firehose / firesol / ...)          │
│  ├─ dmlog → firecore reader                                 │
│  └─ merged-blocks → object store (S3 / Ceph / GCS)          │
└─────────────────────────────────────────────────────────────┘
                         │
                         ▼
┌─────────────────────────────────────────────────────────────┐
│  firehose-core (Relayer + gRPC server), unmodified          │
│  Port 13042 — speaks sf.firehose.v2 natively                │
└─────────────────────────────────────────────────────────────┘
                         │  gRPC (internal)
                         ▼
┌─────────────────────────────────────────────────────────────┐
│  mainline-service (Rust, this repo)                         │
│  - TAP receipt validation                                   │
│  - MainlineAttestation signing per block                    │
│  - Per-chain advertised-LIB publishing                      │
│  - Quality metrics (latency, throughput, completeness)      │
│  - TLS termination                                          │
└─────────────────────────────────────────────────────────────┘
                         │  sf.firehose.v2 (TLS)
                         ▼
                    Consumers
```

## What lives where

| Concern | Component | Why here |
|---|---|---|
| Stake, provision, registration, slashing | `contracts/FirehoseDataService.sol` | Horizon data service primitives are on-chain. |
| Header-proof dispute verification | `contracts/FirehoseDisputeVerifier.sol` (Phase 3) | Verification must be settled by consensus, not by an operator. |
| `sf.firehose.v2` gRPC surface | `mainline-service/src/grpc/` | The upstream protobuf is reused unchanged; we re-export. |
| EIP-712 attestation signing | `mainline-service/src/attestation/` | Held by the operator key; sits on the hot path. |
| TAP receipt validation | `mainline-service/src/billing/` | Has to be checked before serving a stream. |
| Per-chain block fetching/streaming | `mainline-service/src/chain_adapter/` | Pluggable. Each adapter wraps a `firehose-core` client. |
| Operator discovery, quality routing | `mainline-gateway/` | Optional, but the reference gateway is how Phase 1 quorum verification happens. |
| Operator/chain/LIB state on-chain | `subgraph/` | Consumers and the gateway use this to build their operator pool. |
| Consumer TAP signing, cursor handling | `mainline-sdk/` | Belongs in client libraries, not in the service. |

## What we do not own

- **The instrumented node forks.** `geth-firehose`, `firesol`, etc. live in StreamingFast's repos. Mainline depends on them but does not maintain them. GRC §6 flags this as a real single-point-of-failure.
- **The `sf.firehose.v2` protobuf.** Owned upstream by StreamingFast. We re-export.
- **`firehose-core`.** Same.

This is deliberate. Mainline's job is to put Horizon primitives around an existing, working Firehose stack — not to rebuild Firehose.
