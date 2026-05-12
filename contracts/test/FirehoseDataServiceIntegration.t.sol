// SPDX-License-Identifier: GPL-2.0-or-later
pragma solidity 0.8.27;

import { Test } from "forge-std/Test.sol";

import { IHorizonStaking } from "@graphprotocol/interfaces/contracts/horizon/IHorizonStaking.sol";
import { IHorizonStakingTypes } from "@graphprotocol/interfaces/contracts/horizon/internal/IHorizonStakingTypes.sol";
import { IGraphPayments } from "@graphprotocol/interfaces/contracts/horizon/IGraphPayments.sol";
import { IGraphTallyCollector } from "@graphprotocol/interfaces/contracts/horizon/IGraphTallyCollector.sol";
import { IDataService } from "@graphprotocol/interfaces/contracts/data-service/IDataService.sol";
import { IController } from "@graphprotocol/interfaces/contracts/contracts/governance/IController.sol";

import { FirehoseDataService } from "../FirehoseDataService.sol";

/**
 * @dev Integration test that walks the GRC-006 Phase 0 happy path end-to-end
 *      on a forge/anvil instance, with mocks for the Horizon dependencies
 *      we don't vendor (HorizonStaking, GraphTallyCollector, Controller).
 *
 *      Steps mirrored from `docs/phase-0-runbook.md`:
 *        1. Deploy mocks + FirehoseDataService.
 *        2. Governance registers the Ethereum mainnet chain manifest.
 *        3. Indexer provisions stake (mocked) → register().
 *        4. Indexer startService().
 *        5. Indexer advertiseChain(LIB=18_000_000).
 *        6. Indexer collect() a signed RAV → GraphTallyCollector mock pays 1_000_000 wei.
 *        7. Verify every observable event fires and state is updated.
 *
 *      Runs locally as part of `forge test`. The same flow is replayed against
 *      a live Anvil node by `script/LocalDevnet.s.sol`.
 */
contract MockController is IController {
    mapping(bytes32 => address) private _contracts;
    bool private _paused;
    bool private _partialPaused;
    address private _governor;

    function setContractProxy(bytes32 id, address proxy) external override {
        _contracts[id] = proxy;
    }

    function getContractProxy(bytes32 id) external view override returns (address) {
        return _contracts[id];
    }

    function getGovernor() external view override returns (address) {
        return _governor;
    }

    function setPaused(bool p) external override {
        _paused = p;
    }

    function setPartialPaused(bool p) external override {
        _partialPaused = p;
    }

    function paused() external view override returns (bool) {
        return _paused;
    }

    function partialPaused() external view override returns (bool) {
        return _partialPaused;
    }

    function setPauseGuardian(address) external override { }
    function unsetContractProxy(bytes32) external override { }
    function updateController(bytes32, address) external override { }
}

contract MockHorizonStaking {
    mapping(address => mapping(address => IHorizonStakingTypes.Provision)) private _provisions;
    mapping(address => mapping(address => bool)) public authorized;

    function setProvision(address sp, address verifier, IHorizonStakingTypes.Provision memory p) external {
        _provisions[sp][verifier] = p;
    }

    function authorize(address sp, address operator) external {
        authorized[sp][operator] = true;
    }

    function getProvision(address sp, address verifier) external view returns (IHorizonStakingTypes.Provision memory) {
        return _provisions[sp][verifier];
    }

    function isAuthorized(address sp, address verifier, address operator) external view returns (bool) {
        if (operator == sp) return true;
        return authorized[sp][operator];
    }

    fallback() external { }
}

/// Mock GraphTallyCollector that just records its calls and returns a deterministic
/// `tokensCollected` so the integration test can verify routing without spinning up
/// the full payments stack.
contract MockGraphTallyCollector {
    event CollectCalled(
        IGraphPayments.PaymentTypes paymentType,
        address indexed serviceProvider,
        address indexed destination,
        uint128 valueAggregate,
        uint256 dataServiceCut,
        uint256 tokensCollected
    );

    uint256 public stubbedReturn;
    bytes public lastData;

    function setStubbedReturn(uint256 v) external {
        stubbedReturn = v;
    }

    function collect(
        IGraphPayments.PaymentTypes paymentType,
        bytes calldata data,
        uint256 // tokensToCollect — unused: full amount path
    )
        external
        returns (uint256)
    {
        lastData = data;
        (IGraphTallyCollector.SignedRAV memory signedRav, uint256 dataServiceCut, address destination) =
            abi.decode(data, (IGraphTallyCollector.SignedRAV, uint256, address));
        emit CollectCalled(
            paymentType,
            signedRav.rav.serviceProvider,
            destination,
            signedRav.rav.valueAggregate,
            dataServiceCut,
            stubbedReturn
        );
        return stubbedReturn;
    }

    // Tonic of no-op fallbacks so any other interaction with the real IGraphTallyCollector
    // surface compiles cleanly. We only care about `collect` for this flow.
    fallback() external { }
}

contract FirehoseDataServiceIntegrationTest is Test {
    FirehoseDataService internal svc;
    MockController internal controller;
    MockHorizonStaking internal staking;
    MockGraphTallyCollector internal collector;

    address internal governance = address(0xACAB);
    address internal indexer = address(0xBEEF);
    address internal payee = address(0xCAFE);
    address internal payer = address(0xDEEDEED);

    bytes32 internal constant ETHEREUM_MAINNET = bytes32(uint256(1));
    uint64 internal constant ADVERTISED_LIB = 18_000_000;
    uint256 internal constant TOKENS_COLLECTED = 1_000_000;

    function setUp() public {
        controller = new MockController();
        staking = new MockHorizonStaking();
        collector = new MockGraphTallyCollector();

        controller.setContractProxy(keccak256("Staking"), address(staking));
        controller.setContractProxy(keccak256("GraphPayments"), address(0xDEAD));
        controller.setContractProxy(keccak256("PaymentsEscrow"), address(0xDEAD));
        controller.setContractProxy(keccak256("GraphToken"), address(0xDEAD));
        controller.setContractProxy(keccak256("EpochManager"), address(0xDEAD));
        controller.setContractProxy(keccak256("RewardsManager"), address(0xDEAD));
        controller.setContractProxy(keccak256("GraphTokenGateway"), address(0xDEAD));
        controller.setContractProxy(keccak256("GraphProxyAdmin"), address(0xDEAD));
        controller.setContractProxy(keccak256("Curation"), address(0xDEAD));

        svc = new FirehoseDataService(address(controller), address(collector), governance);

        // Mock the indexer as having an in-range Horizon provision.
        IHorizonStakingTypes.Provision memory p = IHorizonStakingTypes.Provision({
            tokens: 25_000 ether,
            tokensThawing: 0,
            sharesThawing: 0,
            maxVerifierCut: 500_000,
            thawingPeriod: 21 days,
            createdAt: uint64(block.timestamp),
            maxVerifierCutPending: 500_000,
            thawingPeriodPending: 21 days,
            lastParametersStagedAt: uint64(block.timestamp),
            thawingNonce: 0
        });
        staking.setProvision(indexer, address(svc), p);
    }

    /// The full Phase-0 happy path in one test, end to end.
    function test_phase0_fullPaymentLoop() public {
        // ── 1. Governance registers Ethereum mainnet ───────────────────────
        FirehoseDataService.ChainManifest memory manifest = FirehoseDataService.ChainManifest({
            genesisBlock: 0,
            genesisHash: bytes32(uint256(0xdeadbeef)),
            firehoseProtoType: "sf.ethereum.type.v2.Block",
            firstStreamableBlock: 0,
            reorgDepth: 64,
            supportsFetch: true,
            registered: false
        });
        vm.prank(governance);
        svc.registerChain(ETHEREUM_MAINNET, manifest);

        // ── 2. Indexer registers ───────────────────────────────────────────
        bytes memory registerData =
            abi.encode("https://indexer.example", FirehoseDataService.Tier.Reputation, uint32(0), payee);

        vm.expectEmit(true, false, false, false, address(svc));
        emit FirehoseDataService.MainlineIndexerRegistered(
            indexer, "https://indexer.example", FirehoseDataService.Tier.Reputation, 0
        );

        vm.prank(indexer);
        svc.register(indexer, registerData);

        (bool registered, bool active,,,) = svc.services(indexer);
        assertTrue(registered, "indexer registered");
        assertFalse(active, "service not yet started");
        assertEq(svc.paymentsDestination(indexer), payee, "payments destination set");

        // ── 3. Indexer starts service ──────────────────────────────────────
        vm.expectEmit(true, false, false, false, address(svc));
        emit FirehoseDataService.MainlineServiceStarted(indexer);

        vm.prank(indexer);
        svc.startService(indexer, "");

        (, active,,,) = svc.services(indexer);
        assertTrue(active, "service is active");

        // ── 4. Indexer advertises Ethereum mainnet LIB ─────────────────────
        vm.expectEmit(true, true, false, true, address(svc));
        emit FirehoseDataService.ChainAdvertised(indexer, ETHEREUM_MAINNET, ADVERTISED_LIB);

        vm.prank(indexer);
        svc.advertiseChain(ETHEREUM_MAINNET, ADVERTISED_LIB);

        assertEq(svc.advertisedLIB(indexer, ETHEREUM_MAINNET), ADVERTISED_LIB, "lib persisted");

        // LIB cannot regress (§2.5).
        vm.prank(indexer);
        vm.expectRevert(
            abi.encodeWithSelector(
                FirehoseDataService.FirehoseDataServiceLIBRegression.selector,
                indexer,
                ETHEREUM_MAINNET,
                uint64(ADVERTISED_LIB - 1),
                ADVERTISED_LIB
            )
        );
        svc.advertiseChain(ETHEREUM_MAINNET, ADVERTISED_LIB - 1);

        // LIB can advance.
        vm.prank(indexer);
        svc.advertiseChain(ETHEREUM_MAINNET, ADVERTISED_LIB + 100);
        assertEq(svc.advertisedLIB(indexer, ETHEREUM_MAINNET), ADVERTISED_LIB + 100, "lib advanced");

        // ── 5. Collect a signed RAV (the full Phase-0 payment loop) ────────
        IGraphTallyCollector.ReceiptAggregateVoucher memory rav = IGraphTallyCollector.ReceiptAggregateVoucher({
            collectionId: bytes32(uint256(1)),
            payer: payer,
            serviceProvider: indexer,
            dataService: address(svc),
            timestampNs: uint64(block.timestamp * 1e9),
            valueAggregate: uint128(TOKENS_COLLECTED),
            metadata: ""
        });
        IGraphTallyCollector.SignedRAV memory signedRav =
            IGraphTallyCollector.SignedRAV({ rav: rav, signature: bytes(hex"deadbeef") });

        collector.setStubbedReturn(TOKENS_COLLECTED);
        bytes memory collectData = abi.encode(signedRav, uint256(0));

        vm.expectEmit(true, true, false, true, address(svc));
        emit IDataService.ServicePaymentCollected(indexer, IGraphPayments.PaymentTypes.QueryFee, TOKENS_COLLECTED);

        vm.prank(indexer);
        uint256 collected = svc.collect(indexer, IGraphPayments.PaymentTypes.QueryFee, collectData);
        assertEq(collected, TOKENS_COLLECTED, "tokens collected returned");
    }

    /// collect() rejects payment types it doesn't understand. We only do QueryFee.
    function test_collect_rejectsUnsupportedPaymentType() public {
        _bringUpRegisteredIndexer();

        bytes memory junk = abi.encode(uint256(0), uint256(0));
        vm.prank(indexer);
        vm.expectRevert(
            abi.encodeWithSelector(
                FirehoseDataService.FirehoseDataServiceUnsupportedPaymentType.selector,
                IGraphPayments.PaymentTypes.IndexingFee
            )
        );
        svc.collect(indexer, IGraphPayments.PaymentTypes.IndexingFee, junk);
    }

    /// RAVs must name the same indexer as the caller (§2.4 prevents grief).
    function test_collect_rejectsMismatchedServiceProvider() public {
        _bringUpRegisteredIndexer();

        IGraphTallyCollector.ReceiptAggregateVoucher memory rav = IGraphTallyCollector.ReceiptAggregateVoucher({
            collectionId: bytes32(uint256(1)),
            payer: payer,
            serviceProvider: payee, // wrong!
            dataService: address(svc),
            timestampNs: uint64(block.timestamp * 1e9),
            valueAggregate: 1,
            metadata: ""
        });
        IGraphTallyCollector.SignedRAV memory signedRav =
            IGraphTallyCollector.SignedRAV({ rav: rav, signature: bytes(hex"") });
        bytes memory data = abi.encode(signedRav, uint256(0));

        vm.prank(indexer);
        vm.expectRevert(
            abi.encodeWithSelector(FirehoseDataService.FirehoseDataServiceIndexerMismatch.selector, payee, indexer)
        );
        svc.collect(indexer, IGraphPayments.PaymentTypes.QueryFee, data);
    }

    /// setPaymentsDestination can be re-invoked by the indexer after register.
    function test_setPaymentsDestination_updatable() public {
        _bringUpRegisteredIndexer();
        address newDest = address(0xBABE);
        vm.expectEmit(true, true, false, false, address(svc));
        emit FirehoseDataService.PaymentsDestinationSet(indexer, newDest);
        vm.prank(indexer);
        svc.setPaymentsDestination(newDest);
        assertEq(svc.paymentsDestination(indexer), newDest);
    }

    // ── slash() wiring (Phase 3 dispute-verifier path) ─────────────────────

    function test_slash_disabledWhenNoVerifierSet() public {
        _bringUpRegisteredIndexer();
        vm.expectRevert(FirehoseDataService.FirehoseDataServiceSlashDisabled.selector);
        svc.slash(indexer, abi.encode(uint256(1), uint256(0)));
    }

    function test_slash_unauthorizedCallerRejected() public {
        _bringUpRegisteredIndexer();
        vm.prank(governance);
        svc.setDisputeVerifier(address(0xDEC1DE));

        vm.expectRevert(
            abi.encodeWithSelector(FirehoseDataService.FirehoseDataServiceSlashUnauthorized.selector, address(this))
        );
        svc.slash(indexer, abi.encode(uint256(1), uint256(0)));
    }

    function test_setDisputeVerifier_isGovernanceGated() public {
        vm.expectRevert(
            abi.encodeWithSelector(FirehoseDataService.FirehoseDataServiceNotGovernance.selector, address(this))
        );
        svc.setDisputeVerifier(address(0xDEC1DE));

        vm.expectEmit(true, true, false, false, address(svc));
        emit FirehoseDataService.DisputeVerifierSet(address(0), address(0xDEC1DE));
        vm.prank(governance);
        svc.setDisputeVerifier(address(0xDEC1DE));
        assertEq(svc.disputeVerifier(), address(0xDEC1DE));
    }

    // ─── Helpers ─────────────────────────────────────────────────────────
    function _bringUpRegisteredIndexer() internal {
        vm.prank(governance);
        svc.registerChain(
            ETHEREUM_MAINNET,
            FirehoseDataService.ChainManifest({
                genesisBlock: 0,
                genesisHash: bytes32(uint256(0xdeadbeef)),
                firehoseProtoType: "sf.ethereum.type.v2.Block",
                firstStreamableBlock: 0,
                reorgDepth: 64,
                supportsFetch: true,
                registered: false
            })
        );

        bytes memory registerData =
            abi.encode("https://indexer.example", FirehoseDataService.Tier.Reputation, uint32(0), payee);
        vm.prank(indexer);
        svc.register(indexer, registerData);
        vm.prank(indexer);
        svc.startService(indexer, "");
    }
}
