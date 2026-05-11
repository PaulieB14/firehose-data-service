# Status

Live snapshot of what is real vs. what is a stub. Update this file in the same PR as any change that flips an item.

## Contracts

| Item | Status | Notes |
|---|---|---|
| `FirehoseDataService.sol` skeleton | stub | Interfaces declared, all bodies revert or are empty. Does not yet import real `@graphprotocol/horizon` paths. |
| `FirehoseDisputeVerifier.sol` skeleton | stub | Phase 3. Header proof verification logic not started. |
| Hardhat config | stub | |
| Foundry config | stub | |
| Deployment scripts (Arbitrum Sepolia) | not started | Phase 0 exit criterion. |
| Unit tests | not started | |
| Integration tests against Horizon staking | not started | |

## mainline-service (Rust)

| Item | Status | Notes |
|---|---|---|
| Cargo workspace | scaffolded | |
| `grpc/` module — `sf.firehose.v2` server | stub | Re-exports proto definitions; handlers are `todo!()`. |
| `attestation/` — EIP-712 MainlineAttestation signing | stub | Type defined; signing not implemented. |
| `billing/` — TAP v2 receipt verification | stub | Type defined; verification not implemented. |
| `chain_adapter/ethereum.rs` | stub | Wraps a `firehose-core` client; not wired. |
| `chain_adapter/solana.rs` | stub | Same. |
| `chain_adapter/base.rs` | stub | Same. |
| Quality metrics (latency, throughput, completeness) | not started | Required for §2.5 SLA reporting. |
| TLS termination | not started | |

## mainline-gateway (Rust)

| Item | Status | Notes |
|---|---|---|
| Cargo workspace | scaffolded | |
| Operator discovery via network subgraph | not started | Depends on `subgraph/` being deployed. |
| Tier-2 quorum routing | not started | The primary Phase 1 deliverable. |
| Quality-score weighting | not started | |

## mainline-sdk

| Item | Status | Notes |
|---|---|---|
| Rust client | stub | |
| TypeScript client | stub | |
| TAP receipt signing helper | not started | |
| Portable cursor (mainline-cursor-v1) encode/decode | stub | Format defined in §2.7; encoder is a placeholder. |

## subgraph

| Item | Status | Notes |
|---|---|---|
| Manifest (`subgraph.yaml`) | stub | Points at placeholder contract address. |
| Schema | stub | Operator, Chain, AdvertisedLib, ServiceURL entities sketched. |
| Mappings (TypeScript) | stub | |

## Open architectural questions

Carried over from GRC-006 §6, repeated here so contributors see them on day one:

- StreamingFast patch maintenance dependency (geth-firehose, firesol). Single-point-of-failure for every operator simultaneously on hard forks.
- Chain-specific Tier-1 dispute verifiers. Real cryptography work per chain. Phase 3 scope.
- Bandwidth economics for operators in expensive-egress regions. Subscription lane is a partial mitigation.
- Cursor portability assumes operators maintain a ForkDB deep enough to resume arbitrary recent cursors.
