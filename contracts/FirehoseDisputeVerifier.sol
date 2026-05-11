// SPDX-License-Identifier: GPL-2.0-or-later
pragma solidity 0.8.27;

/**
 * @title FirehoseDisputeVerifier
 * @notice Phase 3 contract. Resolves disputes over MainlineAttestations by
 *         comparing claimed payload hashes against canonical chain headers.
 * @dev Stub. Per-chain verification logic is real cryptographic work and is
 *      out of scope for the initial scaffold. See GRC-006 §2.6.
 *
 *      Tier 1 verification: Merkle-root comparison.
 *      - For Ethereum L1: light-client proofs against trusted beacon headers.
 *      - For L2s (Base, Arbitrum, Optimism): L1 inbox / state commitments.
 *      - For Solana: bank hash against a quorum of validators or a trusted RPC.
 *
 *      When a chain's verifier is not yet implemented, the parent
 *      FirehoseDataService records that chain as Tier-2-only and slash() is
 *      a no-op for it.
 */
contract FirehoseDisputeVerifier {
    struct MainlineAttestation {
        bytes32 chainId;
        uint64  blockNumber;
        bytes32 blockHash;
        bytes32 stateRoot;
        bytes32 payloadHash;
        bytes   cursor;
        bytes   indexerSig;       // EIP-712 over (chainId, blockNum, blockHash, payloadHash)
    }

    event DisputeOpened(
        bytes32 indexed disputeId,
        address indexed challenger,
        address indexed indexer,
        bytes32 chainId,
        uint64  blockNumber
    );
    event DisputeResolved(bytes32 indexed disputeId, bool indexerSlashed);

    function openDispute(
        MainlineAttestation calldata attestation,
        bytes calldata
    ) external returns (bytes32 disputeId) {
        attestation;
        revert("FirehoseDisputeVerifier: openDispute() not implemented (Phase 3)");
    }

    function resolveDispute(
        bytes32 disputeId,
        bytes calldata canonicalHeaderProof
    ) external {
        disputeId; canonicalHeaderProof;
        revert("FirehoseDisputeVerifier: resolveDispute() not implemented (Phase 3)");
    }
}
