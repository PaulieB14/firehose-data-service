# Phase 0 runbook — Arbitrum Sepolia end-to-end

This runbook walks through the Phase 0 exit criterion from GRC-006 §5:

> Full payment loop demonstrated on testnet; at least one external consumer (graph-node dev stack) pulling blocks.

Tracked by [issue #9](https://github.com/PaulieB14/firehose-data-service/issues/9). The contract, subgraph, service, gateway and SDK are all green in CI as of [commit 5d45fe3](https://github.com/PaulieB14/firehose-data-service/commit/5d45fe3); what remains is the operational work: deploy, point the subgraph at the deployment, boot an operator, hook up a consumer.

Total wall-clock: ~2 hours assuming RPC + ETH-on-Sepolia are already in hand.

## 0. Prerequisites

| Thing | Why | Where to get it |
|---|---|---|
| Arbitrum Sepolia RPC URL | `forge script` target | Alchemy / Infura / public RPC |
| Deployer EOA with ≥0.05 testnet ETH on Arbitrum Sepolia | Pays for the `forge script` deployment | [Arbitrum Sepolia faucet](https://faucet.quicknode.com/arbitrum/sepolia) |
| Indexer EOA registered as a Horizon service provider on Arbitrum Sepolia | Operator account; needs 25k testnet GRT provisioned | [Horizon testnet UI](https://thegraph.com/horizon) (Arbitrum Sepolia) |
| Indexer's `geth-firehose` instance pointing at Sepolia | Upstream block source | Run `streamingfast/firehose-ethereum` locally; default port 13042 |
| Subgraph Studio account | Hosts the network subgraph | https://thegraph.com/studio/ |
| Graph CLI auth token | `graph deploy` | Studio UI → API key |

The two Horizon addresses you need are public on Arbitrum Sepolia:

| Contract | Arbitrum Sepolia |
|---|---|
| `Controller` | look up the latest in `@graphprotocol/horizon/addresses.json` |
| `GraphTallyCollector` | same source |

Capture them once into your shell env:

```bash
export ARBITRUM_SEPOLIA_RPC=https://...
export DEPLOYER_KEY=0x...
export INDEXER_KEY=0x...
export GRAPH_CONTROLLER=0x...
export GRAPH_TALLY_COLLECTOR=0x...
export FIREHOSE_GOVERNANCE=$(cast wallet address $DEPLOYER_KEY)   # phase-1 chain registrar; can be a multisig later
```

## 1. Deploy `FirehoseDataService`

```bash
cd contracts
forge script script/Deploy.s.sol:Deploy \
  --rpc-url $ARBITRUM_SEPOLIA_RPC \
  --broadcast \
  --private-key $DEPLOYER_KEY \
  -vvvv
```

Expected output ends with:

```
FirehoseDataService deployed at: 0x…
  controller:          0x…
  graphTallyCollector: 0x…
  governance:          0x…
```

Capture the deploy address and the deploy block number:

```bash
export FIREHOSE_DATA_SERVICE=0x…
export FIREHOSE_DEPLOY_BLOCK=$(cast block-number --rpc-url $ARBITRUM_SEPOLIA_RPC)
# (deploy block ≈ current block — close enough for subgraph startBlock)
```

## 2. Register Ethereum mainnet as a chain manifest

Phase 1 governance model: the address you set as `FIREHOSE_GOVERNANCE` is the only one allowed to call `registerChain`.

```bash
cast send $FIREHOSE_DATA_SERVICE \
  "registerChain(bytes32,(uint64,bytes32,string,uint32,uint32,bool,bool))" \
  0x0000000000000000000000000000000000000000000000000000000000000001 \
  "(0,0x0000000000000000000000000000000000000000000000000000000000000000,\"sf.ethereum.type.v2.Block\",0,64,true,false)" \
  --rpc-url $ARBITRUM_SEPOLIA_RPC \
  --private-key $DEPLOYER_KEY
```

The chain id is `bytes32(uint256(1))` — Ethereum mainnet eip-155. `reorgDepth=64` matches mainnet finality.

## 3. Update + deploy the subgraph

```bash
cd ../subgraph

# Refresh the ABI from the just-deployed contract artifact.
cd ../contracts && npm run export-abi
cd ../subgraph

# Point the manifest at the live deployment.
sed -i.bak \
  -e "s|address: \".*\"|address: \"$FIREHOSE_DATA_SERVICE\"|" \
  -e "s|startBlock: .*|startBlock: $FIREHOSE_DEPLOY_BLOCK|" \
  subgraph.yaml

npx graph codegen
npx graph build

# Deploy to Subgraph Studio (use your slug).
npx graph deploy mainline-network --version-label v0.1.0
```

After ~5 minutes the subgraph is synced from the deploy block forward and will pick up the `ChainRegistered` event from step 2.

Note its query URL — you'll point the gateway at it in step 6.

## 4. Register the indexer

```bash
# Encode register() data: (string url, Tier tier, uint32 geoHint, address paymentsDestination_)
# Tier 0 = Reputation (no on-chain dispute path); upgrade later.
INDEXER_URL=https://your-indexer.example
PAYMENTS_DEST=$(cast wallet address $INDEXER_KEY)
DATA=$(cast abi-encode "f(string,uint8,uint32,address)" \
  $INDEXER_URL 0 0 $PAYMENTS_DEST)

# Important: this requires the indexer to have an active Horizon provision
# of >=25000 testnet GRT, with thawing >=21d and verifier cut <=50%.
cast send $FIREHOSE_DATA_SERVICE \
  "register(address,bytes)" \
  $(cast wallet address $INDEXER_KEY) \
  $DATA \
  --rpc-url $ARBITRUM_SEPOLIA_RPC \
  --private-key $INDEXER_KEY
```

You should see a `MainlineIndexerRegistered` event from the subgraph indexer within a block or two.

## 5. Start one Mainline operator

In the indexer's environment:

```bash
cd mainline-service
export MAINLINE_LISTEN=0.0.0.0:13050
export MAINLINE_UPSTREAM=http://127.0.0.1:13042       # your firehose-ethereum
export MAINLINE_CHAIN=ethereum
export MAINLINE_SETTLEMENT_CHAIN_ID=421614            # Arbitrum Sepolia
export MAINLINE_FDS_ADDRESS=$FIREHOSE_DATA_SERVICE
export MAINLINE_GRAPH_TALLY_COLLECTOR=$GRAPH_TALLY_COLLECTOR
export MAINLINE_OPERATOR_KEY=$(echo $INDEXER_KEY | sed 's/^0x//')

cargo run --release
```

`mainline-service starting` confirms it's listening; the gateway/SDK pick it up from the network subgraph as soon as the indexer advertises a chain (next step).

## 6. Advertise the chain

The indexer pushes its current LIB to the contract whenever firehose-ethereum advances. For the first run a one-shot is fine:

```bash
LIB=$(cast call $MAINLINE_UPSTREAM_ADDRESS "currentLib()(uint64)" 2>/dev/null || echo 18000000)

cast send $FIREHOSE_DATA_SERVICE \
  "advertiseChain(bytes32,uint64)" \
  0x0000000000000000000000000000000000000000000000000000000000000001 \
  $LIB \
  --rpc-url $ARBITRUM_SEPOLIA_RPC \
  --private-key $INDEXER_KEY

# Tier 0 → activate the service so the gateway considers it.
cast send $FIREHOSE_DATA_SERVICE \
  "startService(address,bytes)" \
  $(cast wallet address $INDEXER_KEY) \
  0x \
  --rpc-url $ARBITRUM_SEPOLIA_RPC \
  --private-key $INDEXER_KEY
```

A `ChainAdvertised` event lands within ~one Arbitrum Sepolia block; the subgraph picks it up; gateway/SDK now consider this operator a candidate.

The indexer should re-call `advertiseChain` on every firehose-ethereum head advance to keep the on-chain LIB fresh — wire this into your operator's existing chainhead loop.

## 7. Pull blocks from the consumer side

From any host with `mainline-sdk/rust` available:

```rust
use mainline_sdk::{
    AttestationDomain, Client, OperatorPool, OperatorTier,
    sign_receipt, tap_header, TapDomain, TapReceiptV2,
};

let pool = OperatorPool::from_subgraph_response(
    &reqwest::Client::new()
        .post("https://api.studio.thegraph.com/query/<id>/mainline-network/v0.1.0")
        .json(&serde_json::json!({
            "query": "{ operators(where:{active:true}) { id url tier geoHint active chains { chain { id } lib } } }"
        }))
        .send().await?
        .text().await?,
    chain_id_eth_mainnet,
)?;

let op = pool.next_for_chain(chain_id_eth_mainnet, OperatorTier::Reputation)?;

// Build + sign a single TAP receipt covering ~1000 blocks worth of bandwidth.
let mut receipt = TapReceiptV2 {
    allocation_id,                  // your Horizon allocation
    timestamp_ns: now_ns(),
    nonce: 1,
    value: 1_000_000,               // wei; sized to the per-burst price
    signature: vec![],
};
sign_receipt(&tap_domain, &mut receipt, &sender_key)?;

// Open the gRPC stream against op.url with metadata:
//   x-tap-receipt: <hex>
// using your preferred tonic / grpc-web client. Each response.cursor will
// have "||mainline-att||<hex>" appended; pass it back through
// Client::recv_block to verify + extract the inner cursor.
```

Set the loop break condition to 1000 blocks. Successful completion satisfies the §5 exit criterion.

## 8. Verifying the loop closed cleanly

Once the consumer hits 1000 blocks:

```bash
# Check that the indexer's RAV settled at least once.
# (This step requires a Phase 0 RAV-aggregator harness; for now the
# off-chain demonstration is the consumer reading 1000 blocks plus a
# manual cast call to collect() with a hand-signed RAV.)
RAV_DATA=$(...)   # encoded as (IGraphTallyCollector.SignedRAV, uint256 dataServiceCut)
cast send $FIREHOSE_DATA_SERVICE \
  "collect(address,uint8,bytes)" \
  $(cast wallet address $INDEXER_KEY) \
  0 \
  $RAV_DATA \
  --rpc-url $ARBITRUM_SEPOLIA_RPC \
  --private-key $INDEXER_KEY
```

A `ServicePaymentCollected` event is emitted; the subgraph indexes it as a `PaymentEvent`. The full payment loop is now demonstrated.

## Troubleshooting

| Symptom | Likely cause | Fix |
|---|---|---|
| `forge script` reverts with `OnlyAuthorizedForProvision` | Deployer EOA does not match the registered Horizon governance | Use the `--account` that the Horizon `Controller` recognises as governor, or transfer governance there first |
| `register()` reverts with `InvalidProvision` | Provision tokens / thawing / verifier cut outside the ranges set in the constructor | Adjust the provision in HorizonStaking and re-call |
| Subgraph fails to index `MainlineIndexerRegistered` | `subgraph.yaml` `startBlock` is past the actual deploy block | Set `startBlock` to the block of step 1, redeploy with a new version label |
| Consumer reads `Unauthenticated: missing x-tap-receipt` | TAP receipt header not set / wrong key name | Use `tap_header(&receipt)` and attach as `x-tap-receipt` (hex-encoded) |
| Operator returns `Unavailable: upstream firehose-core unreachable` | `MAINLINE_UPSTREAM` not actually serving | Confirm firehose-ethereum on the configured port; default 13042 |

## What this runbook deliberately leaves out

- **Tier-2 quorum gateway boot.** Standing up `mainline-gateway` is a Phase 1 step. The Phase 0 exit criterion only requires *one* operator and *one* consumer.
- **Tier-1 dispute watcher.** Phase 3; see `docs/dispute-design.md`.
- **RAV aggregator end-to-end.** Phase 0 demonstrates the on-chain `collect()` path with a hand-signed RAV. Wiring `tap-agent` or equivalent is a follow-up.
