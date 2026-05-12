// SPDX-License-Identifier: GPL-2.0-or-later
pragma solidity 0.8.27;

import { IGraphPayments } from "@graphprotocol/interfaces/contracts/horizon/IGraphPayments.sol";
import { IGraphTallyCollector } from "@graphprotocol/interfaces/contracts/horizon/IGraphTallyCollector.sol";
import { IDataService } from "@graphprotocol/interfaces/contracts/data-service/IDataService.sol";

import { DataService } from "@graphprotocol/horizon/contracts/data-service/DataService.sol";

/**
 * @title FirehoseDataService
 * @notice Horizon data service for raw, fork-aware Firehose block streams (GRC-006: Mainline).
 * @dev Inherits the minimal Horizon `DataService` base, exactly mirroring the layering used
 *      by `SubstreamsDataService` from `graphprotocol/substreams-data-service`. All payments
 *      flow through `GraphTallyCollector` — there is no new payment primitive.
 *
 *      Per GRC-006 §2.1, registration requires a provision in HorizonStaking with:
 *        - tokens >= MIN_PROVISION_TOKENS
 *        - thawingPeriod >= MIN_THAWING_PERIOD
 *        - verifierCut <= MAX_VERIFIER_CUT_PPM
 *
 *      Per §2.3, chains are governance-allowlisted in Phase 1 (this contract). Phases 2+
 *      relax to bond-based and curation-weighted registration.
 */
contract FirehoseDataService is DataService {
    // -----------------------------------------------------------------------
    // Protocol parameters (GRC-006 §2.1)
    // -----------------------------------------------------------------------
    uint256 public constant MIN_PROVISION_TOKENS = 25_000 ether;
    uint64  public constant MIN_THAWING_PERIOD   = 21 days;
    uint32  public constant MAX_VERIFIER_CUT_PPM = 500_000; // 50%

    // -----------------------------------------------------------------------
    // Wired Horizon collector
    // -----------------------------------------------------------------------
    /// @notice The GraphTallyCollector used to settle TAP RAVs.
    IGraphTallyCollector public immutable GRAPH_TALLY_COLLECTOR;

    // -----------------------------------------------------------------------
    // Chain registry (§2.3 — governance-allowlisted in Phase 1)
    // -----------------------------------------------------------------------
    struct ChainManifest {
        uint64  genesisBlock;
        bytes32 genesisHash;
        string  firehoseProtoType;     // e.g. "sf.ethereum.type.v2.Block"
        uint32  firstStreamableBlock;
        uint32  reorgDepth;            // irreversibility horizon
        bool    supportsFetch;         // true for archive-backed chains
        bool    registered;
    }

    /// @notice Governance address authorized to add chains (Phase 1 model).
    address public governance;

    // -----------------------------------------------------------------------
    // Phase-3 dispute verifier (set by governance after deployment per the
    // design doc — the verifier is deployed downstream from this contract
    // because it needs this contract's address in its constructor)
    // -----------------------------------------------------------------------
    /// @notice The FirehoseDisputeVerifier authorised to call `slash()`.
    ///         Zero address = slashing disabled (Phase-0/1/2 default).
    address public disputeVerifier;

    /// @notice Per-chainId manifest.
    mapping(bytes32 chainId => ChainManifest manifest) public chains;

    // -----------------------------------------------------------------------
    // Indexer registration (§2.1)
    // -----------------------------------------------------------------------
    enum Tier { Reputation, Quorum, ProofBacked }

    struct IndexerService {
        bool      registered;
        bool      active;
        string    url;            // gRPC endpoint (TLS)
        Tier      tier;
        uint32    geoHint;
    }

    /// @notice Per-indexer service metadata.
    mapping(address indexer => IndexerService service) public services;

    /// @notice Per-indexer payments destination (cf. SubstreamsDataService).
    mapping(address indexer => address destination) public paymentsDestination;

    /// @notice Per-(indexer, chainId) most recently advertised last-irreversible-block.
    mapping(address indexer => mapping(bytes32 chainId => uint64 lib)) public advertisedLIB;

    // -----------------------------------------------------------------------
    // Events
    // -----------------------------------------------------------------------
    event GovernanceTransferred(address indexed previousGovernance, address indexed newGovernance);
    event DisputeVerifierSet(address indexed previousVerifier, address indexed newVerifier);
    event ChainRegistered(bytes32 indexed chainId, ChainManifest manifest);
    event MainlineIndexerRegistered(address indexed indexer, string url, Tier tier, uint32 geoHint);
    event MainlineServiceStarted(address indexed indexer);
    event MainlineServiceStopped(address indexed indexer);
    event ChainAdvertised(address indexed indexer, bytes32 indexed chainId, uint64 lib);
    event PaymentsDestinationSet(address indexed indexer, address indexed destination);

    // -----------------------------------------------------------------------
    // Errors
    // -----------------------------------------------------------------------
    error FirehoseDataServiceNotGovernance(address caller);
    error FirehoseDataServiceChainAlreadyRegistered(bytes32 chainId);
    error FirehoseDataServiceChainNotRegistered(bytes32 chainId);
    error FirehoseDataServiceIndexerNotRegistered(address indexer);
    error FirehoseDataServiceIndexerAlreadyRegistered(address indexer);
    error FirehoseDataServiceLIBRegression(address indexer, bytes32 chainId, uint64 advertised, uint64 last);
    error FirehoseDataServiceIndexerMismatch(address ravServiceProvider, address indexer);
    error FirehoseDataServiceUnsupportedPaymentType(IGraphPayments.PaymentTypes paymentType);
    error FirehoseDataServiceSlashUnauthorized(address caller);
    error FirehoseDataServiceSlashDisabled();

    // -----------------------------------------------------------------------
    // Modifiers
    // -----------------------------------------------------------------------
    modifier onlyGovernance() {
        if (msg.sender != governance) revert FirehoseDataServiceNotGovernance(msg.sender);
        _;
    }

    modifier onlyRegisteredIndexer(address indexer) {
        if (!services[indexer].registered) revert FirehoseDataServiceIndexerNotRegistered(indexer);
        _;
    }

    modifier onlyAuthorizedForProvision(address serviceProvider) {
        _requireAuthorizedForProvision(serviceProvider);
        _;
    }

    modifier onlyValidProvision(address serviceProvider) {
        _requireValidProvision(serviceProvider);
        _;
    }

    // -----------------------------------------------------------------------
    // Constructor
    // -----------------------------------------------------------------------
    /**
     * @param controller The Graph Horizon controller address (gives access to HorizonStaking,
     *                   GraphPayments, etc. via GraphDirectory).
     * @param graphTallyCollector The deployed `GraphTallyCollector` to route RAVs through.
     * @param governance_ Address allowed to register chains in Phase 1.
     */
    constructor(
        address controller,
        address graphTallyCollector,
        address governance_
    ) DataService(controller) {
        GRAPH_TALLY_COLLECTOR = IGraphTallyCollector(graphTallyCollector);
        governance = governance_;

        // Apply Phase 1 provision guard rails (§2.1).
        _setProvisionTokensRange(MIN_PROVISION_TOKENS, type(uint256).max);
        _setThawingPeriodRange(MIN_THAWING_PERIOD, type(uint64).max);
        _setVerifierCutRange(0, MAX_VERIFIER_CUT_PPM);

        emit GovernanceTransferred(address(0), governance_);
    }

    // -----------------------------------------------------------------------
    // Governance
    // -----------------------------------------------------------------------
    function transferGovernance(address newGovernance) external onlyGovernance {
        emit GovernanceTransferred(governance, newGovernance);
        governance = newGovernance;
    }

    /**
     * @notice Set the FirehoseDisputeVerifier authorised to call `slash()`.
     *         Setting to `address(0)` disables slashing entirely (the Phase-0/1/2
     *         default). The verifier is deployed downstream from this contract
     *         (it needs this contract's address in its constructor), so the
     *         wiring is governance-driven rather than constructor-time.
     */
    function setDisputeVerifier(address newVerifier) external onlyGovernance {
        emit DisputeVerifierSet(disputeVerifier, newVerifier);
        disputeVerifier = newVerifier;
    }

    /**
     * @notice Register a new chain manifest (§2.3, Phase 1: governance allowlist).
     */
    function registerChain(bytes32 chainId, ChainManifest calldata manifest) external onlyGovernance {
        if (chains[chainId].registered) revert FirehoseDataServiceChainAlreadyRegistered(chainId);
        ChainManifest memory m = manifest;
        m.registered = true;
        chains[chainId] = m;
        emit ChainRegistered(chainId, m);
    }

    // -----------------------------------------------------------------------
    // DataService overrides (IDataService surface)
    // -----------------------------------------------------------------------

    /**
     * @inheritdoc IDataService
     * @dev `data` ABI-decodes as `(string url, Tier tier, uint32 geoHint, address paymentsDestination_)`.
     */
    function register(
        address indexer,
        bytes calldata data
    ) external override onlyAuthorizedForProvision(indexer) onlyValidProvision(indexer) {
        if (services[indexer].registered) revert FirehoseDataServiceIndexerAlreadyRegistered(indexer);

        (string memory url, Tier tier, uint32 geoHint, address destination) =
            abi.decode(data, (string, Tier, uint32, address));

        services[indexer] = IndexerService({
            registered: true,
            active: false,
            url: url,
            tier: tier,
            geoHint: geoHint
        });
        _setPaymentsDestination(indexer, destination);

        emit ServiceProviderRegistered(indexer, data);
        emit MainlineIndexerRegistered(indexer, url, tier, geoHint);
    }

    /// @inheritdoc IDataService
    function acceptProvisionPendingParameters(
        address indexer,
        bytes calldata data
    ) external override onlyAuthorizedForProvision(indexer) {
        _acceptProvisionParameters(indexer);
        emit ProvisionPendingParametersAccepted(indexer);
        // silence unused-parameter warning
        data;
    }

    /// @inheritdoc IDataService
    function startService(
        address indexer,
        bytes calldata data
    ) external override onlyAuthorizedForProvision(indexer) onlyRegisteredIndexer(indexer) {
        services[indexer].active = true;
        emit ServiceStarted(indexer, data);
        emit MainlineServiceStarted(indexer);
    }

    /// @inheritdoc IDataService
    function stopService(
        address indexer,
        bytes calldata data
    ) external override onlyAuthorizedForProvision(indexer) onlyRegisteredIndexer(indexer) {
        services[indexer].active = false;
        emit ServiceStopped(indexer, data);
        emit MainlineServiceStopped(indexer);
    }

    /**
     * @inheritdoc IDataService
     * @dev Only `QueryFee` is supported. Per GRC-006 §2.4, both `Stream.Blocks` (bandwidth-priced)
     *      and `Fetch.Block` (per-block-priced) settle as QueryFee RAVs — the data service is
     *      payment-mode-agnostic; pricing lives in the off-chain TAP receipt domain.
     */
    function collect(
        address indexer,
        IGraphPayments.PaymentTypes paymentType,
        bytes calldata data
    )
        external
        override
        onlyAuthorizedForProvision(indexer)
        onlyValidProvision(indexer)
        onlyRegisteredIndexer(indexer)
        returns (uint256)
    {
        if (paymentType != IGraphPayments.PaymentTypes.QueryFee) {
            revert FirehoseDataServiceUnsupportedPaymentType(paymentType);
        }

        (IGraphTallyCollector.SignedRAV memory signedRav, uint256 dataServiceCut) =
            abi.decode(data, (IGraphTallyCollector.SignedRAV, uint256));

        if (signedRav.rav.serviceProvider != indexer) {
            revert FirehoseDataServiceIndexerMismatch(signedRav.rav.serviceProvider, indexer);
        }

        uint256 tokensCollected = GRAPH_TALLY_COLLECTOR.collect(
            IGraphPayments.PaymentTypes.QueryFee,
            abi.encode(signedRav, dataServiceCut, paymentsDestination[indexer]),
            0
        );

        emit ServicePaymentCollected(indexer, paymentType, tokensCollected);
        return tokensCollected;
    }

    /**
     * @inheritdoc IDataService
     * @dev Only the configured `disputeVerifier` is authorised to call this.
     *      When `disputeVerifier == address(0)` (Phase-0/1/2 default), slashing
     *      is disabled and any call reverts with `FirehoseDataServiceSlashDisabled`.
     *      Delegates to HorizonStaking via `_graphStaking().slash(...)`, mirroring
     *      the path SubgraphService takes; ½ of slashed tokens go to the dispute
     *      challenger as `verifierDestination`, the rest is burned by the protocol.
     *
     *      Per docs/dispute-design.md the `data` payload is
     *      `abi.encode(uint256 tokens, uint256 reward)`.
     */
    function slash(address indexer, bytes calldata data) external override {
        if (disputeVerifier == address(0)) revert FirehoseDataServiceSlashDisabled();
        if (msg.sender != disputeVerifier) revert FirehoseDataServiceSlashUnauthorized(msg.sender);

        (uint256 tokens, uint256 reward) = abi.decode(data, (uint256, uint256));
        // verifierDestination = the dispute verifier (which forwards the reward
        // to the upheld-dispute challenger). Keeps the data service free of
        // per-dispute bookkeeping.
        _graphStaking().slash(indexer, tokens, reward, disputeVerifier);
        emit ServiceProviderSlashed(indexer, tokens);
    }

    // -----------------------------------------------------------------------
    // Firehose-specific
    // -----------------------------------------------------------------------

    /**
     * @notice Advertise the last-irreversible-block for `chainId`. Indexers MUST NOT regress
     *         their advertised LIB (§2.5).
     */
    function advertiseChain(bytes32 chainId, uint64 lib) external onlyRegisteredIndexer(msg.sender) {
        if (!chains[chainId].registered) revert FirehoseDataServiceChainNotRegistered(chainId);

        uint64 last = advertisedLIB[msg.sender][chainId];
        if (lib < last) revert FirehoseDataServiceLIBRegression(msg.sender, chainId, lib, last);

        advertisedLIB[msg.sender][chainId] = lib;
        emit ChainAdvertised(msg.sender, chainId, lib);
    }

    /**
     * @notice Update the payments-destination address used when settling RAVs for `msg.sender`.
     */
    function setPaymentsDestination(address destination) external onlyRegisteredIndexer(msg.sender) {
        _setPaymentsDestination(msg.sender, destination);
    }

    // -----------------------------------------------------------------------
    // Internal helpers
    // -----------------------------------------------------------------------
    function _setPaymentsDestination(address indexer, address destination) internal {
        paymentsDestination[indexer] = destination;
        emit PaymentsDestinationSet(indexer, destination);
    }
}
