# Contributing

This repository is a community-started reference implementation of [GRC-006: Mainline](https://forum.thegraph.com/). It is intended to be transferred to `graphprotocol/` or handed to the GRC author ([@cargopete](https://github.com/cargopete)) once it reaches a usable state. Contribute accordingly.

## Ground rules

1. **The GRC is the source of truth.** If code disagrees with the GRC, the GRC wins unless we have an explicit discussion to amend it. Substantive design changes should go to the forum thread, not this repo.
2. **Stay close to the templates.** Contracts model on [SubgraphService](https://github.com/graphprotocol/contracts) and [substreams-data-service](https://github.com/graphprotocol/substreams-data-service). The Rust service models on [Dispatch](https://github.com/cargopete/dispatch) and on `indexer-service-rs`.
3. **No new payment primitives.** GRC-006 §2.4 is explicit that all payments flow through GraphTally / TAP v2. Do not add new payment paths.
4. **No protobuf forks.** The service exposes `sf.firehose.v2` unchanged. Mainline-specific data goes in gRPC trailers as a `MainlineAttestation` message; it does not mutate the upstream schema.

## What needs doing

See `docs/STATUS.md` for the live list. Roughly in priority order:

- [ ] Wire `contracts/` against real `@graphprotocol/horizon` imports and make tests pass
- [ ] Implement TAP receipt verification in `mainline-service/src/billing/`
- [ ] Implement `MainlineAttestation` EIP-712 signing in `mainline-service/src/attestation/`
- [ ] Wire `mainline-service/src/chain_adapter/ethereum.rs` against `firehose-core` Rust client
- [ ] Define the network subgraph schema entities (Operator, Chain, AdvertisedLib, ServiceURL)
- [ ] Write the gateway routing logic in `mainline-gateway/`
- [ ] Write a reference TypeScript consumer in `mainline-sdk/typescript/`

## PR process

- One concern per PR. A PR that touches contracts, the daemon, and the subgraph at once is too big.
- Reference the GRC section your change implements in the PR description (e.g. "implements §2.2 EndpointInfo response").
- Tests are encouraged but not required for stub-filling PRs. They are required once a stub is claimed complete.

## Code of conduct

The Graph community Code of Conduct applies. Be useful, be specific, don't be a jerk.
