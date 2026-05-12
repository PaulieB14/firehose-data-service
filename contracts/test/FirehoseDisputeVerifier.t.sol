// SPDX-License-Identifier: GPL-2.0-or-later
pragma solidity 0.8.27;

import { Test } from "forge-std/Test.sol";

import { IERC20 } from "@openzeppelin/contracts/token/ERC20/IERC20.sol";

import { FirehoseDisputeVerifier, IFirehoseDataServiceSlasher } from "../FirehoseDisputeVerifier.sol";
import { IFirehoseDisputeVerifier } from "../IFirehoseDisputeVerifier.sol";
import { IBeaconHeaderOracle } from "../IBeaconHeaderOracle.sol";

/// @dev Minimal ERC-20 mock for bond escrow flows.
contract MockGRT is IERC20 {
    mapping(address => uint256) public override balanceOf;
    mapping(address => mapping(address => uint256)) public override allowance;
    uint256 public override totalSupply;

    function mint(address to, uint256 amount) external {
        balanceOf[to] += amount;
        totalSupply += amount;
    }

    function approve(address spender, uint256 amount) external override returns (bool) {
        allowance[msg.sender][spender] = amount;
        return true;
    }

    function transfer(address to, uint256 amount) external override returns (bool) {
        balanceOf[msg.sender] -= amount;
        balanceOf[to] += amount;
        return true;
    }

    function transferFrom(address from, address to, uint256 amount) external override returns (bool) {
        if (allowance[from][msg.sender] != type(uint256).max) {
            allowance[from][msg.sender] -= amount;
        }
        balanceOf[from] -= amount;
        balanceOf[to] += amount;
        return true;
    }
}

/// @dev Stub oracle whose `headerOf` returns whatever the test pre-loaded
///      for a given (chainId, blockNumber) — or reverts when no record.
contract MockBeaconOracle is IBeaconHeaderOracle {
    struct Header { bytes32 blockHash; bytes32 stateRoot; bool set; }
    mapping(bytes32 => mapping(uint64 => Header)) public headers;

    function setHeader(bytes32 chainId, uint64 blockNumber, bytes32 blockHash, bytes32 stateRoot) external {
        headers[chainId][blockNumber] = Header(blockHash, stateRoot, true);
    }

    function headerOf(bytes32 chainId, uint64 blockNumber)
        external
        view
        override
        returns (bytes32, bytes32)
    {
        Header memory h = headers[chainId][blockNumber];
        require(h.set, "oracle: no canonical record yet");
        return (h.blockHash, h.stateRoot);
    }
}

/// @dev Captures slash() calls so tests can assert the data service is invoked
///      correctly without standing up the full FirehoseDataService + Horizon
///      stack. (Those integration paths are exercised in
///      FirehoseDataServiceIntegration.t.sol.)
contract SlashRecorder is IFirehoseDataServiceSlasher {
    event SlashRecorded(address indexed serviceProvider, uint256 tokens, uint256 reward);
    bool public revertOnNext;
    function setRevertOnNext(bool v) external { revertOnNext = v; }
    function slash(address serviceProvider, bytes calldata data) external override {
        if (revertOnNext) revert("forced revert");
        (uint256 tokens, uint256 reward) = abi.decode(data, (uint256, uint256));
        emit SlashRecorded(serviceProvider, tokens, reward);
    }
}

contract FirehoseDisputeVerifierTest is Test {
    FirehoseDisputeVerifier internal verifier;
    MockGRT internal grt;
    MockBeaconOracle internal oracle;
    SlashRecorder internal dataService;

    address internal challenger = address(0xC4A11ED);
    address internal indexer    = address(0xBEEF);

    bytes32 internal constant ETH = bytes32(uint256(1));
    uint64  internal constant BLOCK = 19_000_000;

    uint256 internal constant SLASH = 25_000 ether;

    function setUp() public {
        grt = new MockGRT();
        oracle = new MockBeaconOracle();
        dataService = new SlashRecorder();
        verifier = new FirehoseDisputeVerifier(grt, dataService, oracle, SLASH);

        grt.mint(challenger, verifier.DISPUTE_BOND() * 4);
        vm.prank(challenger);
        grt.approve(address(verifier), type(uint256).max);
    }

    // ── createDispute ──────────────────────────────────────────────────────

    function test_createDispute_escrowsBondAndStoresFields() public {
        uint256 balBefore = grt.balanceOf(challenger);

        vm.expectEmit(true, false, false, true, address(verifier));
        emit IFirehoseDisputeVerifier.DisputeCreated(1, ETH, BLOCK, indexer, challenger);

        vm.prank(challenger);
        uint256 id = verifier.createDispute(
            ETH, BLOCK, bytes32(uint256(0xa1)), bytes32(uint256(0xa2)),
            indexer, hex"deadbeef", hex""
        );

        assertEq(id, 1);
        assertEq(grt.balanceOf(challenger), balBefore - verifier.DISPUTE_BOND(), "bond escrowed");
        assertEq(grt.balanceOf(address(verifier)), verifier.DISPUTE_BOND(), "bond held in verifier");

        IFirehoseDisputeVerifier.Dispute memory d = verifier.getDispute(id);
        assertEq(d.chainId, ETH);
        assertEq(d.blockNumber, BLOCK);
        assertEq(d.indexer, indexer);
        assertEq(d.challenger, challenger);
        assertFalse(d.resolved);
    }

    function test_createDispute_rejectsEmptyAttestationSig() public {
        vm.prank(challenger);
        vm.expectRevert(FirehoseDisputeVerifier.EmptyAttestationSig.selector);
        verifier.createDispute(
            ETH, BLOCK, bytes32(uint256(0xa1)), bytes32(uint256(0xa2)),
            indexer, hex"", hex""
        );
    }

    // ── settleDispute: pre-conditions ──────────────────────────────────────

    function test_settleDispute_revertsBeforeMinDelay() public {
        vm.prank(challenger);
        uint256 id = _openSimpleDispute();

        vm.expectRevert(
            abi.encodeWithSelector(
                FirehoseDisputeVerifier.DisputeResolutionTooEarly.selector,
                id, uint64(block.timestamp) + verifier.MIN_RESOLUTION_DELAY()
            )
        );
        verifier.settleDispute(id);
    }

    function test_settleDispute_revertsOnUnknownId() public {
        vm.expectRevert(abi.encodeWithSelector(FirehoseDisputeVerifier.DisputeNotFound.selector, 99));
        verifier.settleDispute(99);
    }

    function test_settleDispute_revertsOnOracleSilence() public {
        vm.prank(challenger);
        uint256 id = _openSimpleDispute();
        vm.warp(block.timestamp + verifier.MIN_RESOLUTION_DELAY() + 1);
        // Oracle has no record → reverts with its own message.
        vm.expectRevert("oracle: no canonical record yet");
        verifier.settleDispute(id);
    }

    // ── settleDispute: upheld (indexer was fraudulent) ─────────────────────

    function test_settleDispute_upheld_slashesAndReturnsBond() public {
        bytes32 claimedHash = bytes32(uint256(0xa1));
        bytes32 claimedRoot = bytes32(uint256(0xa2));
        bytes32 canonicalHash = bytes32(uint256(0xbeef)); // different → fraud
        bytes32 canonicalRoot = bytes32(uint256(0xcafe));

        vm.prank(challenger);
        uint256 id = verifier.createDispute(
            ETH, BLOCK, claimedHash, claimedRoot, indexer, hex"01", hex""
        );

        oracle.setHeader(ETH, BLOCK, canonicalHash, canonicalRoot);
        vm.warp(block.timestamp + verifier.MIN_RESOLUTION_DELAY() + 1);

        uint256 bondHeld = grt.balanceOf(address(verifier));
        uint256 challengerBefore = grt.balanceOf(challenger);

        vm.expectEmit(true, false, false, true, address(dataService));
        emit SlashRecorder.SlashRecorded(indexer, SLASH, SLASH / 2);

        vm.expectEmit(true, true, false, true, address(verifier));
        emit IFirehoseDisputeVerifier.DisputeUpheld(id, indexer, SLASH, SLASH / 2);

        verifier.settleDispute(id);

        assertTrue(verifier.getDispute(id).resolved, "marked resolved");
        assertEq(grt.balanceOf(address(verifier)), bondHeld - verifier.DISPUTE_BOND(), "bond returned");
        assertEq(grt.balanceOf(challenger), challengerBefore + verifier.DISPUTE_BOND(), "challenger got bond back");
    }

    // ── settleDispute: dismissed (indexer was honest) ──────────────────────

    function test_settleDispute_dismissed_forfeitsBond() public {
        bytes32 sharedHash = bytes32(uint256(0xa1));
        bytes32 sharedRoot = bytes32(uint256(0xa2));

        vm.prank(challenger);
        uint256 id = verifier.createDispute(
            ETH, BLOCK, sharedHash, sharedRoot, indexer, hex"01", hex""
        );
        oracle.setHeader(ETH, BLOCK, sharedHash, sharedRoot);
        vm.warp(block.timestamp + verifier.MIN_RESOLUTION_DELAY() + 1);

        uint256 bondHeld = grt.balanceOf(address(verifier));
        uint256 challengerBefore = grt.balanceOf(challenger);

        vm.expectEmit(true, false, false, true, address(verifier));
        emit IFirehoseDisputeVerifier.DisputeDismissed(id, "consensus-match");

        verifier.settleDispute(id);

        assertTrue(verifier.getDispute(id).resolved);
        // Bond stays in the verifier (governance sweeps + burns it Phase-3-real).
        assertEq(grt.balanceOf(address(verifier)), bondHeld, "bond forfeited");
        assertEq(grt.balanceOf(challenger), challengerBefore, "challenger keeps nothing");
    }

    // ── re-settle ──────────────────────────────────────────────────────────

    function test_settleDispute_cannotResolveTwice() public {
        bytes32 hash_ = bytes32(uint256(0xa1));
        bytes32 root_ = bytes32(uint256(0xa2));

        vm.prank(challenger);
        uint256 id = verifier.createDispute(
            ETH, BLOCK, hash_, root_, indexer, hex"01", hex""
        );
        oracle.setHeader(ETH, BLOCK, hash_, root_);
        vm.warp(block.timestamp + verifier.MIN_RESOLUTION_DELAY() + 1);

        verifier.settleDispute(id);

        vm.expectRevert(abi.encodeWithSelector(FirehoseDisputeVerifier.DisputeAlreadyResolved.selector, id));
        verifier.settleDispute(id);
    }

    // ── constants per GRC-006 §2.6 + dispute-design.md ─────────────────────

    function test_constants_matchDesignDoc() public view {
        assertEq(verifier.DISPUTE_BOND(), 10_000 ether, "bond = 10k GRT");
        assertEq(verifier.DISPUTE_WINDOW(), 21 days, "window = 21 days");
        assertEq(verifier.MIN_RESOLUTION_DELAY(), 1 hours, "min delay = 1 hour");
        assertEq(verifier.SLASH_AMOUNT(), SLASH);
    }

    // ── helpers ────────────────────────────────────────────────────────────
    function _openSimpleDispute() internal returns (uint256) {
        return verifier.createDispute(
            ETH, BLOCK, bytes32(uint256(0xa1)), bytes32(uint256(0xa2)),
            indexer, hex"01", hex""
        );
    }
}
