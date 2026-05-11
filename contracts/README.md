# contracts/

Solidity contracts for the Mainline data service (GRC-006).

## What's here

- `FirehoseDataService.sol` — the main data service contract. Inherits `DataService`
  from `@graphprotocol/horizon`, mirroring [`SubstreamsDataService`](https://github.com/graphprotocol/substreams-data-service/blob/main/horizon/devenv/build/contracts/SubstreamsDataService.sol)'s
  inheritance graph. Routes `collect()` through `GraphTallyCollector`.
- `FirehoseDisputeVerifier.sol` — Phase 3 contract. Stub. Design lives in
  `../docs/dispute-design.md`.
- `script/Deploy.s.sol` — Foundry deploy script.
- `test/` — unit tests for the data service.
- `lib/` — vendored dependencies (forge-std, OpenZeppelin v5, a trimmed
  `graphprotocol/contracts` subset containing only horizon + interfaces). See
  the "Why vendored" note below.

## Toolchain

```bash
forge build        # exits 0
forge test         # 5/5 pass
forge fmt --check
```

The Hardhat surface is kept for parity with `graphprotocol/contracts` tasks (verification, deploy-to-explorer plumbing) but the source of truth is Foundry.

## Parameter defaults (GRC-006 §2.1)

| Constant | Value | Source |
|---|---|---|
| `MIN_PROVISION_TOKENS` | 25,000 GRT | §2.1 |
| `MIN_THAWING_PERIOD` | 21 days | §2.1 |
| `MAX_VERIFIER_CUT_PPM` | 500,000 (50%) | §2.1 |

The constructor applies these as the Horizon `ProvisionManager`'s allowed ranges
(`_setProvisionTokensRange`, `_setThawingPeriodRange`, `_setVerifierCutRange`),
which is the supported governance-mutable path under the Horizon framework.

## Deploying to Arbitrum Sepolia (Phase 0)

```bash
export ARBITRUM_SEPOLIA_RPC=https://...
export DEPLOYER_KEY=0x...
export GRAPH_CONTROLLER=0x...          # Horizon controller on Arb Sepolia
export GRAPH_TALLY_COLLECTOR=0x...     # Deployed GraphTallyCollector
export FIREHOSE_GOVERNANCE=0x...       # Phase-1 chain registrar

forge script script/Deploy.s.sol:Deploy \
  --rpc-url $ARBITRUM_SEPOLIA_RPC \
  --broadcast \
  --private-key $DEPLOYER_KEY
```

After deploying, copy the resulting address into `subgraph/subgraph.yaml`
(`dataSources[0].source.address`) and bump `startBlock` to the deploy block.
The ABI in `subgraph/abis/FirehoseDataService.json` is generated from
`out/FirehoseDataService.sol/FirehoseDataService.json`; refresh after any
contract change with:

```bash
jq '.abi' out/FirehoseDataService.sol/FirehoseDataService.json \
  > ../subgraph/abis/FirehoseDataService.json
```

## Why vendored

`@graphprotocol/horizon` is published to npm but ships compiled artifacts only,
not Solidity source. Forge needs source to compile against. The substreams-data-service
reference works around this with a Docker build that mounts a local
`graphprotocol/contracts` checkout. To keep the repo CI-runnable without that
step, this scaffold vendors:

- `forge-std@v1.9.7`
- `OpenZeppelin/openzeppelin-contracts@v5.0.2` (test/certora directories pruned)
- `OpenZeppelin/openzeppelin-contracts-upgradeable@v5.0.2` (same pruning)
- A curated subset of `graphprotocol/contracts/packages/{horizon,interfaces}/`
  with legacy-dependent files removed (`GraphPayments`, `PaymentsEscrow`, the
  HorizonStaking implementation, `RecurringCollector`, curation interfaces).
  None of these are required at compile time — we interact with them through
  their interfaces.

Remappings live in `remappings.txt`.

To upgrade the Horizon subset, re-run:

```bash
./scripts/update-horizon.sh  # see scripts/ for the trim script
```
