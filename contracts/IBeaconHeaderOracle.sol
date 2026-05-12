// SPDX-License-Identifier: GPL-2.0-or-later
pragma solidity 0.8.27;

/**
 * @title IBeaconHeaderOracle
 * @notice The canonical chain-header source consulted by FirehoseDisputeVerifier.
 *
 * Per docs/dispute-design.md, Phase 3 T1 verification compares a disputed
 * indexer attestation's `(blockHash, stateRoot)` against an authoritative
 * reference for the same `(chainId, blockNumber)`. This interface abstracts
 * that reference so the verifier can be wired to:
 *
 *   1. A beacon-chain SSZ-proof relay (default Phase-3 plan).
 *   2. The L1 inbox / ArbSys.arbBlockHash for L2 disputes.
 *   3. A trusted multisig oracle during early Phase-3 (with a planned
 *      migration to (1) once a production relay is in place).
 *
 * Implementations are interchangeable — `FirehoseDisputeVerifier` only ever
 * calls `headerOf`.
 */
interface IBeaconHeaderOracle {
    /**
     * @notice Return the canonical `(blockHash, stateRoot)` for a block on
     *         `chainId` at `blockNumber`.
     * @dev If the oracle has no record (block not yet relayed / proof not
     *      yet finalised), it MUST revert. The verifier interprets a revert
     *      as "cannot rule on this dispute yet" and the challenger can retry.
     */
    function headerOf(bytes32 chainId, uint64 blockNumber) external view returns (bytes32 blockHash, bytes32 stateRoot);
}
