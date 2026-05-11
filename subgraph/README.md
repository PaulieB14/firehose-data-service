# subgraph

The Graph network subgraph for Mainline state.

## What it indexes

From `FirehoseDataService`:

- `ChainRegistered` → `Chain` entity (manifest, supported by which indexers)
- `IndexerRegistered` → `Operator` entity (URL, tier, geo)
- `ChainAdvertised` → `AdvertisedLib` entity (per-operator-per-chain LIB timeseries)
- `ServiceStarted` / `ServiceStopped` → operator availability windows
- `PaymentCollected` → revenue per operator per period
- `IndexerSlashed` → slashing events (Phase 3)

## Consumers

- `mainline-gateway` queries this to build its operator pool
- `mainline-sdk` queries this for operator discovery
- Indexers query this to monitor their own state
- Anyone building a Mainline indexer dashboard

## Status

Stub. `subgraph.yaml` points at a placeholder address. Schema entities are sketched; mappings are empty.
