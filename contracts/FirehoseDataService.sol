// SPDX-License-Identifier: GPL-2.0-or-later
pragma solidity 0.8.27;

// NOTE: Imports are commented out until @graphprotocol/horizon is added
// as a dependency. See contracts/README.md.
//
// import { DataService } from "@graphprotocol/horizon/contracts/data-service/DataService.sol";
// import { DataServicePausable } from "@graphprotocol/horizon/contracts/data-service/extensions/DataServicePausable.sol";
// import { DataServiceFees } from "@graphprotocol/horizon/contracts/data-service/extensions/DataServiceFees.sol";
// import { DataServiceRescuable } from "@graphprotocol/horizon/contracts/data-service/extensions/DataServiceRescuable.sol";
// import { IGraphPayments } from "@graphprotocol/horizon/contracts/interfaces/IGraphPayments.sol";

/**
 * @title FirehoseDataService
 * @notice Horizon data service for raw, fork-aware Firehose block streams.
 * @dev Reference implementation scaffold for GRC-006 (Mainline).
 *
 *      Inherits from the same DataService base as SubgraphService:
 *      DataService + DataServicePausable + DataServiceFees + DataServiceRescuable + DataServiceUpgradeable.
 *      No new payment primitive. All payments flow through GraphTally / TAP v2.
 *
 *      Stub: function bodies are placeholders. See docs/STATUS.md.
 */
contract FirehoseDataService
    // is DataService, DataServicePausable, DataServiceFees, DataServiceRescuable
{
    // -----------------------------------------------------------------------
    // Protocol parameters (GRC §2.1 defaults — should be governance-mutable)
    // -----------------------------------------------------------------------
    uint256 public constant MIN_PROVISION_TOKENS = 25_000 ether;
    uint256 public constant STAKE_TO_FEES_RATIO  = 4;
    uint64  public constant MIN_THAWING_PERIOD   = 21 days;
    uint32  public constant MAX_VERIFIER_CUT_PPM = 500_000;

    // -----------------------------------------------------------------------
    // Chain registry (GRC §2.3)
    // Phase 1: governance allowlist. Phase 2: bond-based. Phase 3: curation-weighted.
    // -----------------------------------------------------------------------
    struct ChainManifest {
        uint64  genesisBlock;
        bytes32 genesisHash;
        string  firehoseProtoType;     // e.g. "sf.ethereum.type.v2.Block"
        uint32  firstStreamableBlock;
        uint32  reorgDepth;            // irreversibility horizon
        bool    supportsFetch;         // true for archive-backed chains
    }
    mapping(bytes32 => ChainManifest) public chains;

    // -----------------------------------------------------------------------
    // Indexer registration (GRC §2.1)
    // -----------------------------------------------------------------------
    enum Tier { Reputation, Quorum, ProofBacked }

    struct IndexerService {
        string    url;            // gRPC endpoint (TLS)
        bytes32[] chainIds;       // chains served
        Tier      tier;           // verification tier indexer subscribes to
        uint32    geoHint;
        uint64    advertisedLIB;  // last advertised irreversible block; per-chain mapping below
    }
    mapping(address => IndexerService) public services;
    mapping(address => mapping(bytes32 => uint64)) public advertisedLIB;

    // -----------------------------------------------------------------------
    // Events
    // -----------------------------------------------------------------------
    event ChainRegistered(bytes32 indexed chainId, ChainManifest manifest);
    event IndexerRegistered(address indexed indexer, string url, Tier tier);
    event ChainAdvertised(address indexed indexer, bytes32 indexed chainId, uint64 lib);
    event ServiceStarted(address indexed indexer);
    event ServiceStopped(address indexed indexer);
    event PaymentCollected(address indexed indexer, uint8 paymentType, uint256 tokens);
    event IndexerSlashed(address indexed indexer, uint256 tokens, uint256 reward);

    // -----------------------------------------------------------------------
    // DataService overrides — stubs
    // -----------------------------------------------------------------------

    /**
     * @notice Register a new Mainline indexer service.
     * @dev Decodes `data` as (string url, bytes32[] chainIds, Tier tier, uint32 geoHint).
     */
    function register(address indexer, bytes calldata data) external /* override */ {
        // TODO: decode data, validate provision >= MIN_PROVISION_TOKENS via HorizonStaking,
        // persist IndexerService, emit IndexerRegistered.
        indexer; data;
        revert("FirehoseDataService: register() not implemented");
    }

    function startService(address indexer, bytes calldata data) external /* override */ {
        indexer; data;
        revert("FirehoseDataService: startService() not implemented");
    }

    function stopService(address indexer, bytes calldata data) external /* override */ {
        indexer; data;
        revert("FirehoseDataService: stopService() not implemented");
    }

    // -----------------------------------------------------------------------
    // Firehose-specific
    // -----------------------------------------------------------------------

    function advertiseChain(bytes32 chainId, uint64 lib) external {
        // TODO: require msg.sender is a registered indexer for chainId,
        // require lib >= previous advertisedLIB (no regressions; §2.5),
        // persist, emit ChainAdvertised.
        chainId; lib;
        revert("FirehoseDataService: advertiseChain() not implemented");
    }

    /**
     * @notice Collect TAP receipts/RAVs for streamed bytes or Fetch requests.
     * @dev Routes through GraphTallyCollector. No custom math here.
     */
    function collect(
        address indexer,
        uint8   paymentType,
        bytes calldata data
    ) external /* override */ returns (uint256) {
        // TODO: forward to GraphTallyCollector, enforce STAKE_TO_FEES_RATIO,
        // emit PaymentCollected.
        indexer; paymentType; data;
        revert("FirehoseDataService: collect() not implemented");
    }

    /**
     * @notice Slash an indexer for fraudulent attestations.
     * @dev Phase 0/1/2: reverts. Phase 3 wires to FirehoseDisputeVerifier.
     */
    function slash(
        address indexer,
        uint256 tokens,
        uint256 reward,
        bytes calldata evidence
    ) external /* override */ {
        indexer; tokens; reward; evidence;
        revert("FirehoseDataService: slash() not enabled until Phase 3");
    }
}
