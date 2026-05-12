// SPDX-License-Identifier: GPL-2.0-or-later
pragma solidity 0.8.27;

import { Test } from "forge-std/Test.sol";
import { Vm } from "forge-std/Vm.sol";

import { IHorizonStaking } from "@graphprotocol/interfaces/contracts/horizon/IHorizonStaking.sol";
import { IController } from "@graphprotocol/interfaces/contracts/contracts/governance/IController.sol";

import { FirehoseDataService } from "../FirehoseDataService.sol";

/**
 * @dev Unit-test scaffold for FirehoseDataService.
 *
 * The real `DataService` base interacts with HorizonStaking via `GraphDirectory` —
 * Phase 0 integration tests against a staked Anvil run live in `test/integration/`
 * once the Horizon devenv is wired in CI. For the unit layer we stub the controller
 * + staking so we can exercise the chain registry, advertisement, and indexer
 * registration paths without spinning up the full Horizon stack.
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
    mapping(address => mapping(address => IHorizonStaking.Provision)) private _provisions;
    mapping(address => mapping(address => bool)) public authorized;

    function setProvision(address sp, address verifier, IHorizonStaking.Provision memory p) external {
        _provisions[sp][verifier] = p;
    }

    function authorize(address sp, address operator) external {
        authorized[sp][operator] = true;
    }

    function getProvision(address sp, address verifier) external view returns (IHorizonStaking.Provision memory) {
        return _provisions[sp][verifier];
    }

    function isAuthorized(address sp, address verifier, address operator) external view returns (bool) {
        if (operator == sp) return true;
        return authorized[sp][operator];
    }

    // Fallback no-ops for setters touched by ProvisionManager paths we don't exercise.
    fallback() external { }
}

contract FirehoseDataServiceTest is Test {
    FirehoseDataService internal svc;
    MockController internal controller;
    MockHorizonStaking internal staking;

    address internal governance = address(0xACAB);
    address internal collector = address(0xC0117EC);
    address internal indexer = address(0xBEEF);
    address internal payee = address(0xCAFE);

    bytes32 internal constant ETHEREUM_MAINNET = bytes32(uint256(1));

    function setUp() public {
        controller = new MockController();
        staking = new MockHorizonStaking();

        // GraphDirectory pulls these via Controller.getContractProxy(keccak256("Foo")).
        controller.setContractProxy(keccak256("Staking"), address(staking));
        controller.setContractProxy(keccak256("GraphPayments"), address(0xDEAD));
        controller.setContractProxy(keccak256("PaymentsEscrow"), address(0xDEAD));
        controller.setContractProxy(keccak256("GraphToken"), address(0xDEAD));
        controller.setContractProxy(keccak256("EpochManager"), address(0xDEAD));
        controller.setContractProxy(keccak256("RewardsManager"), address(0xDEAD));
        controller.setContractProxy(keccak256("GraphTokenGateway"), address(0xDEAD));
        controller.setContractProxy(keccak256("GraphProxyAdmin"), address(0xDEAD));
        controller.setContractProxy(keccak256("Curation"), address(0xDEAD));

        svc = new FirehoseDataService(address(controller), collector, governance);
    }

    function test_constants_matchGRC006() public view {
        assertEq(svc.MIN_PROVISION_TOKENS(), 25_000 ether, "min provision = 25k GRT");
        assertEq(svc.MIN_THAWING_PERIOD(), 21 days, "thawing >= 21d");
        assertEq(svc.MAX_VERIFIER_CUT_PPM(), 500_000, "verifier cut <= 50%");
    }

    function test_governance_canRegisterChain() public {
        FirehoseDataService.ChainManifest memory manifest = FirehoseDataService.ChainManifest({
            genesisBlock: 0,
            genesisHash: bytes32(uint256(0xdeadbeef)),
            firehoseProtoType: "sf.ethereum.type.v2.Block",
            firstStreamableBlock: 0,
            reorgDepth: 64,
            supportsFetch: true,
            registered: false
        });

        vm.expectEmit(true, false, false, false, address(svc));
        emit FirehoseDataService.ChainRegistered(ETHEREUM_MAINNET, manifest);

        vm.prank(governance);
        svc.registerChain(ETHEREUM_MAINNET, manifest);

        (
            uint64 genesisBlock,
            bytes32 genesisHash,
            string memory firehoseProtoType,
            uint32 firstStreamableBlock,
            uint32 reorgDepth,
            bool supportsFetch,
            bool registered
        ) = svc.chains(ETHEREUM_MAINNET);

        assertEq(genesisBlock, manifest.genesisBlock);
        assertEq(genesisHash, manifest.genesisHash);
        assertEq(firehoseProtoType, manifest.firehoseProtoType);
        assertEq(firstStreamableBlock, manifest.firstStreamableBlock);
        assertEq(reorgDepth, manifest.reorgDepth);
        assertEq(supportsFetch, manifest.supportsFetch);
        assertTrue(registered);
    }

    function test_nonGovernance_cannotRegisterChain() public {
        FirehoseDataService.ChainManifest memory m = _ethereumManifest();
        vm.expectRevert(
            abi.encodeWithSelector(FirehoseDataService.FirehoseDataServiceNotGovernance.selector, address(this))
        );
        svc.registerChain(ETHEREUM_MAINNET, m);
    }

    function test_chainRegistrationIsIdempotentlyRejected() public {
        FirehoseDataService.ChainManifest memory m = _ethereumManifest();

        vm.prank(governance);
        svc.registerChain(ETHEREUM_MAINNET, m);

        vm.prank(governance);
        vm.expectRevert(
            abi.encodeWithSelector(
                FirehoseDataService.FirehoseDataServiceChainAlreadyRegistered.selector, ETHEREUM_MAINNET
            )
        );
        svc.registerChain(ETHEREUM_MAINNET, m);
    }

    function test_unregisteredIndexer_cannotAdvertise() public {
        vm.prank(governance);
        svc.registerChain(ETHEREUM_MAINNET, _ethereumManifest());

        vm.prank(indexer);
        vm.expectRevert(
            abi.encodeWithSelector(FirehoseDataService.FirehoseDataServiceIndexerNotRegistered.selector, indexer)
        );
        svc.advertiseChain(ETHEREUM_MAINNET, 100);
    }

    function _ethereumManifest() internal pure returns (FirehoseDataService.ChainManifest memory) {
        return FirehoseDataService.ChainManifest({
            genesisBlock: 0,
            genesisHash: bytes32(uint256(0xdeadbeef)),
            firehoseProtoType: "sf.ethereum.type.v2.Block",
            firstStreamableBlock: 0,
            reorgDepth: 64,
            supportsFetch: true,
            registered: false
        });
    }
}
