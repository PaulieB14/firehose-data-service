# contracts/

Solidity contracts for the Mainline data service.

## What's here

- `FirehoseDataService.sol` — the main data service contract. Inherits `DataService` + four extensions from `@graphprotocol/horizon`, exactly mirroring SubgraphService's inheritance graph. Stub.
- `FirehoseDisputeVerifier.sol` — Phase 3 contract. Takes a `MainlineAttestation` plus a chain header proof and decides whether to slash. Stub.
- `interfaces/` — Solidity interfaces split out for SDK consumption.
- `test/` — empty.

## Toolchain

Matches SubgraphService and Dispatch: Hardhat + Foundry side-by-side.

```bash
npm install
npx hardhat compile
forge build
forge test
```

Neither tool will succeed against this scaffold yet — `@graphprotocol/horizon` is not yet a dependency. First real PR should add it and make `compile` pass.

## Parameter defaults (GRC §2.1)

| Constant | Value | Source |
|---|---|---|
| `MIN_PROVISION_TOKENS` | 25,000 GRT | §2.1 |
| `STAKE_TO_FEES_RATIO` | 4 | §2.1 |
| `MIN_THAWING_PERIOD` | 21 days | §2.1 |
| `MAX_VERIFIER_CUT_PPM` | 500,000 (50%) | §2.1 |

These are intentionally `constant` in the scaffold for readability. The production contract should make them governance-mutable through the standard Horizon governance path.
