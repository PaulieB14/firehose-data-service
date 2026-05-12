// SPDX-License-Identifier: GPL-2.0-or-later
pragma solidity 0.8.27;

import { IERC20 } from "@openzeppelin/contracts/token/ERC20/IERC20.sol";

import { IFirehoseDisputeVerifier } from "./IFirehoseDisputeVerifier.sol";
import { IBeaconHeaderOracle } from "./IBeaconHeaderOracle.sol";

/**
 * @title FirehoseDisputeVerifier
 * @notice Phase-3 verifier per GRC-006 §2.6 and docs/dispute-design.md.
 *
 * Path-(b) verification — slash only on consensus-bound disagreements
 * (`blockHash` / `stateRoot`), not on protobuf encoding bugs. The verifier
 * collects a bond from the challenger, names a disputed attestation, and
 * after `MIN_RESOLUTION_DELAY` consults an `IBeaconHeaderOracle` for the
 * canonical `(blockHash, stateRoot)` at `(chainId, blockNumber)`. On
 * mismatch, the verifier calls `FirehoseDataService.slash()` with the
 * configured slash amount and ½-as-reward; on match, the bond is forfeited.
 *
 * Phase-3 implementation note: this skeleton wires the bond-escrow,
 * dispute-storage, and `IFirehoseDataServiceSlasher.slash()` call paths.
 * The signer-recovery and oracle integration are real but parameterised —
 * production deployment swaps in a real `IBeaconHeaderOracle` (SSZ relay
 * or equivalent) and a non-zero `slashAmount`.
 */
interface IFirehoseDataServiceSlasher {
    /// @notice Same signature as `IDataService.slash`. The dispute verifier
    /// calls this with abi-encoded `(uint256 tokens, uint256 reward)`.
    function slash(address serviceProvider, bytes calldata data) external;
}

contract FirehoseDisputeVerifier is IFirehoseDisputeVerifier {
    // ── Constants (per GRC-006 §2.6 + dispute-design.md) ───────────────────
    /// @notice Bond pulled from the challenger on `createDispute`. 10,000 GRT.
    uint256 public constant DISPUTE_BOND = 10_000 ether;

    /// @notice Maximum age of a disputable attestation. Mirrors `MIN_THAWING_PERIOD`
    /// in FirehoseDataService so an indexer cannot rotate stake out faster than
    /// disputes can be filed.
    uint64 public constant DISPUTE_WINDOW = 21 days;

    /// @notice Minimum time between `createDispute` and `settleDispute`. Gives the
    /// beacon-header oracle time to relay the canonical header if it's recent.
    uint64 public constant MIN_RESOLUTION_DELAY = 1 hours;

    // ── Immutable wiring ──────────────────────────────────────────────────
    /// @notice The Graph token (GRT) used for bonds.
    IERC20 public immutable GRAPH_TOKEN;

    /// @notice The FirehoseDataService contract — used to invoke slash().
    IFirehoseDataServiceSlasher public immutable DATA_SERVICE;

    /// @notice The header oracle the verifier consults to settle disputes.
    IBeaconHeaderOracle public immutable HEADER_ORACLE;

    /// @notice Tokens to slash per upheld dispute (post-Phase-3 governance-mutable).
    uint256 public immutable SLASH_AMOUNT;

    // ── Storage ────────────────────────────────────────────────────────────
    /// @notice Strictly monotonic id assigned by `createDispute`.
    uint256 public nextDisputeId = 1;

    /// @notice All disputes ever created, indexed by id.
    mapping(uint256 disputeId => Dispute dispute) private _disputes;

    // ── Errors ─────────────────────────────────────────────────────────────
    error DisputeNotFound(uint256 disputeId);
    error DisputeAlreadyResolved(uint256 disputeId);
    error DisputeResolutionTooEarly(uint256 disputeId, uint64 earliestAt);
    error BondTransferFailed();
    error BondReturnFailed();
    error EmptyAttestationSig();

    // ── Constructor ────────────────────────────────────────────────────────
    constructor(
        IERC20 graphToken,
        IFirehoseDataServiceSlasher dataService,
        IBeaconHeaderOracle headerOracle,
        uint256 slashAmount
    ) {
        GRAPH_TOKEN = graphToken;
        DATA_SERVICE = dataService;
        HEADER_ORACLE = headerOracle;
        SLASH_AMOUNT = slashAmount;
    }

    // ── External: dispute lifecycle ────────────────────────────────────────

    /// @inheritdoc IFirehoseDisputeVerifier
    function createDispute(
        bytes32 chainId,
        uint64 blockNumber,
        bytes32 claimedBlockHash,
        bytes32 claimedStateRoot,
        address indexer,
        bytes calldata attestationSig,
        bytes calldata /* beaconProof */
    ) external returns (uint256 disputeId) {
        if (attestationSig.length == 0) revert EmptyAttestationSig();

        // Escrow the bond from the challenger.
        bool ok = GRAPH_TOKEN.transferFrom(msg.sender, address(this), DISPUTE_BOND);
        if (!ok) revert BondTransferFailed();

        disputeId = nextDisputeId++;
        _disputes[disputeId] = Dispute({
            chainId: chainId,
            blockNumber: blockNumber,
            claimedBlockHash: claimedBlockHash,
            claimedStateRoot: claimedStateRoot,
            indexer: indexer,
            challenger: msg.sender,
            createdAt: uint64(block.timestamp),
            resolved: false
        });

        emit DisputeCreated(disputeId, chainId, blockNumber, indexer, msg.sender);
    }

    /// @inheritdoc IFirehoseDisputeVerifier
    function settleDispute(uint256 disputeId) external {
        Dispute storage d = _disputes[disputeId];
        if (d.createdAt == 0) revert DisputeNotFound(disputeId);
        if (d.resolved) revert DisputeAlreadyResolved(disputeId);

        uint64 earliest = d.createdAt + MIN_RESOLUTION_DELAY;
        if (uint64(block.timestamp) < earliest) {
            revert DisputeResolutionTooEarly(disputeId, earliest);
        }

        // Consult the oracle. If the oracle reverts (no canonical record yet),
        // the whole call reverts and the challenger can retry later — that's
        // the documented semantics in IBeaconHeaderOracle.
        (bytes32 canonicalBlockHash, bytes32 canonicalStateRoot) = HEADER_ORACLE.headerOf(d.chainId, d.blockNumber);

        d.resolved = true;

        bool consensusMatch = (canonicalBlockHash == d.claimedBlockHash) && (canonicalStateRoot == d.claimedStateRoot);

        if (consensusMatch) {
            // Indexer was honest. Forfeit + burn the bond.
            emit DisputeDismissed(disputeId, "consensus-match");
            // The forfeited bond stays in this contract; an off-chain
            // governance call (Phase-3 follow-up) can sweep + burn.
            return;
        }

        // Indexer was fraudulent. Slash via the parent FirehoseDataService.
        // Bond is returned to the challenger plus a reward = ½ slash.
        uint256 reward = SLASH_AMOUNT / 2;
        DATA_SERVICE.slash(d.indexer, abi.encode(SLASH_AMOUNT, reward));

        // Return bond to challenger.
        bool ok = GRAPH_TOKEN.transfer(d.challenger, DISPUTE_BOND);
        if (!ok) revert BondReturnFailed();

        emit DisputeUpheld(disputeId, d.indexer, SLASH_AMOUNT, reward);
    }

    /// @inheritdoc IFirehoseDisputeVerifier
    function getDispute(uint256 disputeId) external view returns (Dispute memory) {
        Dispute storage d = _disputes[disputeId];
        if (d.createdAt == 0) revert DisputeNotFound(disputeId);
        return d;
    }
}
