# Tier-1 dispute verifier — design

**Status**: design only. Tracked by [issue #6](https://github.com/PaulieB14/firehose-data-service/issues/6). Phase 3 deliverable per GRC-006 §2.6.

This document records the decisions made for the Tier-1 (T1, proof-backed) verification path on Ethereum L1. It exists so the contract surface in `FirehoseDisputeVerifier.sol` and the off-chain watcher pieces can be built in parallel rather than serially. None of the implementation below is shipped yet — slashing is gated to `return` no-op in `FirehoseDataService.slash()` until this design lands.

## Scope

GRC-006 §2.6 names three tiers:

- **T3 (Reputation)** — no on-chain dispute path. Stake-weighted reputation only. Phase 0.
- **T2 (Quorum)** — gateway-mediated `Fetch.Block` fan-out. Minority signers are logged; the §2.5 quality score absorbs the penalty. No slashing. Phase 1, lives in `mainline-gateway`.
- **T1 (Proof-backed)** — adversarial dispute against a canonical header proof. **This document.** Phase 3.

Why T1 is the heaviest tier: a fraudulent attestation here costs the indexer its provision (up to `STAKE_TO_FEES_RATIO * fees`, capped by `MAX_VERIFIER_CUT_PPM`). The verifier must therefore be conservative — it must never slash on a *protobuf encoding bug* that two honest indexers might disagree about. It should only slash when the on-chain claim contradicts consensus.

## What the verifier sees

`FirehoseDisputeVerifier` takes:

```
MainlineAttestation {
    bytes32 chainId;
    uint64  blockNumber;
    bytes32 blockHash;
    bytes32 stateRoot;
    bytes32 payloadHash;
    bytes   indexerSig;   // EIP-712 over the above + Mainline domain
}
```

…plus, for a *given* `blockNumber`, a canonical reference for that same block.

## Decision: (b) — slash on consensus-bound fields only

Of the two paths spelled out in [issue #6](https://github.com/PaulieB14/firehose-data-service/issues/6):

- **(a)** Re-derive the entire `sf.ethereum.type.v2.Block` protobuf from a header proof and compare the full `payloadHash`.
- **(b)** Compare only the *consensus-bound* fields (`blockHash`, `stateRoot`) and slash when those disagree, ignoring the protobuf payload byte-equality.

**We adopt (b).** Reasons:

1. **`payloadHash` is not consensus-bound.** It is `sha256` of the chain-specific `Block` protobuf. Honest indexers can disagree on its byte representation when running different `geth-firehose` patch versions, when StreamingFast bumps a field, or when proto-roundtripping reorders unknown fields. None of those are slashing-worthy events — they are operational events that the §2.5 quality score and ordinary protocol upgrade signaling already cover.

2. **`blockHash` and `stateRoot` are consensus-bound.** Disagreement on either of these for an attested `blockNumber` is unambiguous fraud: the indexer signed an attestation claiming a block that does not exist in the canonical chain. This is the only thing slashing should cover.

3. **Implementation cost is bounded.** (a) requires a full re-execution path or a vendored copy of the same protobuf encoder, and we have to maintain it for every supported chain. (b) requires only a header proof source per chain.

The cost of (b) is that an indexer who signs a bogus *payload* for an otherwise-real block escapes slashing. Quorum (T2) catches that case downstream — minority payload hashes get the operator demoted by the gateway and eventually starved of traffic.

## Header proof source

For Ethereum L1 settled on Arbitrum One / Sepolia (Phase 3 target), the verifier needs a way to authenticate a block header `(blockNumber → (blockHash, stateRoot))` on the settlement chain.

Three candidates:

| Source | Trust assumption | Cost | Notes |
|---|---|---|---|
| **L1 inbox on Arbitrum (`ArbSys.arbBlockHash`)** | Trusted L2 sequencer + canonical inbox | Low (single read) | Only resolves the most recent 256 L1 blocks; depth issues for older disputes. |
| **Beacon chain SSZ proof relayed onto Arbitrum** | Trusted relay + crypto-economic backstop | Medium | Works for any L1 block. Requires a relay contract; one already exists in the LayerZero / Succinct ecosystem. |
| **Light-client proof verified on-chain (Helios-style)** | Cryptographic only | High (verification gas) | Most adversarial-safe. Most expensive. |

**Choice**: Phase 3 uses the **beacon chain SSZ proof** path, with a `IBeaconHeaderOracle` interface the verifier reads from. The oracle is parameterised so it can be swapped to a light-client implementation later without touching `FirehoseDisputeVerifier`.

The 21-day dispute window (next section) gives the relay plenty of headroom even when the oracle is paused for emergencies.

## Bond size + window

GRC-006 §2.6 suggests **10,000 GRT bond, 21-day window**. We adopt both unchanged:

- **Bond `DISPUTE_BOND = 10_000 ether`**. Paid by the challenger. Forfeited if the challenge is dismissed; returned + ½ slashed amount if upheld.
- **Window `DISPUTE_WINDOW = 21 days`**. Identical to `MIN_THAWING_PERIOD` so an indexer cannot rotate stake out faster than disputes can be filed.

The 1:1 alignment between dispute window and thawing period is deliberate. It is the same invariant DisputeManager v1 maintains on the legacy SubgraphService.

## Slash mechanics

When upheld:

- The disputed indexer's provision is slashed for the verifier-cut PPM applied to `tokensCovered`, where `tokensCovered` is set by governance (Phase 3 default: same as DisputeManager v1).
- ½ of slashed tokens go to the challenger as `reward`.
- ½ are burned (GraphPayments protocol tax).
- An `IndexerSlashed(indexer, tokens, reward)` event is emitted via `FirehoseDataService.slash()`.

When dismissed:

- The challenger's bond is forfeited and burned.
- An `DisputeDismissed(disputeId, reason)` event is emitted.

## `FirehoseDisputeVerifier` ABI

```solidity
interface IFirehoseDisputeVerifier {
    struct Dispute {
        bytes32 chainId;
        uint64  blockNumber;
        bytes32 claimedBlockHash;
        bytes32 claimedStateRoot;
        address indexer;
        address challenger;
        uint64  createdAt;
        bool    resolved;
    }

    function createDispute(
        bytes32 chainId,
        uint64  blockNumber,
        bytes32 claimedBlockHash,
        bytes32 claimedStateRoot,
        bytes calldata attestationSig,
        bytes calldata beaconProof          // SSZ proof of canonical (blockHash, stateRoot)
    ) external returns (uint256 disputeId);

    function settleDispute(uint256 disputeId) external;

    function getDispute(uint256 disputeId) external view returns (Dispute memory);

    event DisputeCreated(uint256 indexed disputeId, bytes32 chainId, uint64 blockNumber);
    event DisputeUpheld(uint256 indexed disputeId, address indexed indexer, uint256 slashed, uint256 reward);
    event DisputeDismissed(uint256 indexed disputeId, bytes32 reason);
}
```

`FirehoseDataService.slash()` becomes:

```solidity
function slash(address indexer, bytes calldata data) external override {
    // Only the dispute verifier is authorised to call slash().
    require(msg.sender == address(disputeVerifier),
        FirehoseDataServiceSlashUnauthorized(msg.sender));
    (uint256 tokens, uint256 reward) = abi.decode(data, (uint256, uint256));
    _slashProvision(indexer, tokens, reward);  // delegates to HorizonStaking
    emit ServiceProviderSlashed(indexer, tokens);
}
```

Where `disputeVerifier` is an immutable address set in the constructor (Phase 3 deploy).

## What the off-chain watcher does

A reference watcher implementation will be added under `mainline-gateway/src/watcher/` in Phase 3. Per §2.6 it watches:

1. Every operator's per-chain `ChainAdvertised` events.
2. For each advertised LIB, periodically `Fetch.Block` for a sampled subset and compute `(blockHash, stateRoot)` from the chain-specific `Block` protobuf.
3. Cross-reference against the beacon oracle's `(blockHash, stateRoot)` for that height.
4. On mismatch, call `createDispute` with the relevant proof.

The watcher is permissionless — anyone running enough archive infra can run it. The 10k GRT bond is its skin in the game.

## References

- GRC-006 §2.6 (tier definitions)
- GRC-005 (Dispatch) — for the contrast (Dispatch cannot do T1 because its data is reads, not blocks).
- `graphprotocol/contracts` — `DisputeManager.sol` (legacy SubgraphService dispute path; pattern source).
- `succinctlabs/sp1-helios` — reference SSZ proof verifier (one option for the oracle backend).

## Out of scope for Phase 3 (deliberate)

- T1 for L2s (Arbitrum, Base, etc.) — these need rollup-specific finality oracles and are deferred to Phase 4+.
- T1 for Solana — protocol-level adversarial proofs aren't a thing on Solana the way they are on Ethereum. The current expectation is that Solana stays at T2 indefinitely.
- Slashing for protobuf-encoding bugs — explicitly rejected in (b) above.
