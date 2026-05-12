// SPDX-License-Identifier: GPL-2.0-or-later
pragma solidity 0.8.27;

/**
 * @title IFirehoseDisputeVerifier
 * @notice ABI for the Phase-3 dispute verifier described in docs/dispute-design.md.
 *
 * Path-(b) verification: a challenger posts a bond, names a disputed
 * attestation, and provides a beacon-chain header proof for the same
 * `(chainId, blockNumber)`. The verifier compares `claimedBlockHash` /
 * `claimedStateRoot` against the canonical header from an
 * `IBeaconHeaderOracle`. On mismatch, the verifier calls
 * `FirehoseDataService.slash()` and returns ½ of the slashed tokens to the
 * challenger; on match, the bond is forfeited and burned.
 */
interface IFirehoseDisputeVerifier {
    /**
     * @notice One pending or settled dispute.
     * @param chainId           The disputed attestation's chain (bytes32).
     * @param blockNumber       The disputed attestation's block number.
     * @param claimedBlockHash  The block hash the indexer signed in their attestation.
     * @param claimedStateRoot  The state root the indexer signed.
     * @param indexer           The indexer address that issued the disputed attestation.
     * @param challenger        The address that opened the dispute (and posted the bond).
     * @param createdAt         Unix timestamp the dispute was created.
     * @param resolved          Whether `settleDispute` has run.
     */
    struct Dispute {
        bytes32 chainId;
        uint64 blockNumber;
        bytes32 claimedBlockHash;
        bytes32 claimedStateRoot;
        address indexer;
        address challenger;
        uint64 createdAt;
        bool resolved;
    }

    /**
     * @notice Open a dispute against an indexer's attestation. Pulls
     *         `DISPUTE_BOND` GRT from the challenger.
     * @param chainId            The bytes32 chain id from the attestation.
     * @param blockNumber        The block number from the attestation.
     * @param claimedBlockHash   `attestation.blockHash`.
     * @param claimedStateRoot   `attestation.stateRoot`.
     * @param attestationSig     The indexer's EIP-712 signature over the
     *                           attestation (used to identify the indexer
     *                           via signer recovery on settle).
     * @param beaconProof        Opaque proof bytes passed through to the
     *                           configured `IBeaconHeaderOracle`. Format
     *                           depends on the oracle implementation
     *                           (SSZ proof / merkle proof / etc.).
     */
    function createDispute(
        bytes32 chainId,
        uint64 blockNumber,
        bytes32 claimedBlockHash,
        bytes32 claimedStateRoot,
        address indexer,
        bytes calldata attestationSig,
        bytes calldata beaconProof
    ) external returns (uint256 disputeId);

    /**
     * @notice Resolve a pending dispute by consulting the oracle. Anyone
     *         may call after `MIN_RESOLUTION_DELAY` has elapsed.
     */
    function settleDispute(uint256 disputeId) external;

    /// @notice Read a dispute by id.
    function getDispute(uint256 disputeId) external view returns (Dispute memory);

    event DisputeCreated(
        uint256 indexed disputeId,
        bytes32 chainId,
        uint64 blockNumber,
        address indexed indexer,
        address indexed challenger
    );
    event DisputeUpheld(uint256 indexed disputeId, address indexed indexer, uint256 slashed, uint256 reward);
    event DisputeDismissed(uint256 indexed disputeId, bytes32 reason);
}
