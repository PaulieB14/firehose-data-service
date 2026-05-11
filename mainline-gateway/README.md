# mainline-gateway

Optional managed gateway. Mirrors `dispatch-gateway`'s role.

## What it does (eventually)

- Discovers operators by querying the network subgraph in `../subgraph/`
- Builds a routing table per chain weighted by quality score (latency, completeness, availability — §2.5)
- For Tier-2 verification: fans `Fetch` requests out to `k` operators and flags minority `payload_hash` values
- Aggregates per-operator quality metrics and feeds them back into routing weights

## Stub

`cargo build` succeeds; routing logic is `todo!()`.
