# Glossary

Terms used throughout the GRC and this repo, in alphabetical order.

- **Advertised LIB** — the last irreversible block an operator claims to serve for a given chain. Published on-chain via `advertiseChain`. Must not regress; regression is evidence for a Tier-1 dispute.
- **Attestation** — `MainlineAttestation`, an EIP-712-signed gRPC trailer attached to served blocks. Anchors all three verification tiers.
- **ChainManifest** — on-chain record pinning a chain's genesis, protobuf schema URI, irreversibility horizon, and Fetch support.
- **Cursor (mainline-cursor-v1)** — portable resumption token containing `(chainId, libNum, libHash, headNum, headHash, forkSteps)`. Replaces operator-specific Firehose cursors.
- **DataService** — base Horizon contract that FirehoseDataService inherits from. Provides registration, provisioning, fee collection, slashing hooks.
- **Fetch** — single-block lookup by number or hash. One of the three gRPC services Mainline exposes (alongside Stream and EndpointInfo).
- **Firehose** — StreamingFast's block streaming protocol. Mainline serves the `sf.firehose.v2` protobuf surface unchanged.
- **GraphTally / TAP v2** — the off-chain payment primitive Horizon data services use. Receipts → RAVs → on-chain `collect()`. No new payment paths in Mainline.
- **Provision** — GRT staked through `HorizonStaking` and provisioned to a specific data service. Minimum 25,000 GRT for FirehoseDataService per the GRC defaults.
- **STEP_NEW / STEP_UNDO / STEP_IRREVERSIBLE** — Firehose fork-step types. Undos are billed at the same per-byte rate as news; see §2.4.
- **Stake-to-fees ratio** — protocol parameter capping fees collectable per thawing cycle against provision. 4:1 in the GRC defaults.
- **Tier 1 / 2 / 3** — verification tiers. T1 = proof-backed (Merkle root comparison), T2 = quorum across operators, T3 = reputation. See §2.6.
