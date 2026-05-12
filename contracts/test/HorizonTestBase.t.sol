// SPDX-License-Identifier: GPL-2.0-or-later
pragma solidity 0.8.27;

import { Test } from "forge-std/Test.sol";

import { IERC20 } from "@openzeppelin/contracts/token/ERC20/IERC20.sol";
import { ERC20 } from "@openzeppelin/contracts/token/ERC20/ERC20.sol";

import { IGraphToken } from "@graphprotocol/interfaces/contracts/contracts/token/IGraphToken.sol";
import { IController } from "@graphprotocol/interfaces/contracts/contracts/governance/IController.sol";
import { IGraphPayments } from "@graphprotocol/interfaces/contracts/horizon/IGraphPayments.sol";
import { IPaymentsEscrow } from "@graphprotocol/interfaces/contracts/horizon/IPaymentsEscrow.sol";
import { IHorizonStakingTypes } from "@graphprotocol/interfaces/contracts/horizon/internal/IHorizonStakingTypes.sol";
import { IGraphTallyCollector } from "@graphprotocol/interfaces/contracts/horizon/IGraphTallyCollector.sol";

import { GraphTallyCollector } from "@graphprotocol/horizon/contracts/payments/collectors/GraphTallyCollector.sol";

import { FirehoseDataService } from "../FirehoseDataService.sol";

// ─────────────────────────────────────────────────────────────────────────────
// MockGRTToken
//
// A real OZ ERC20 that also satisfies IGraphToken.  Tests mint directly;
// GraphDirectory stores the address and the escrow moves tokens.
// ─────────────────────────────────────────────────────────────────────────────
contract MockGRTToken is ERC20, IGraphToken {
    constructor() ERC20("Graph Token", "GRT") { }

    function mint(address to, uint256 amount) external override {
        _mint(to, amount);
    }

    function burn(uint256 amount) external override {
        _burn(msg.sender, amount);
    }

    function burnFrom(address from, uint256 amount) external override {
        _burn(from, amount);
    }

    // Minter-admin stubs — not exercised in payment tests.
    function addMinter(address) external override { }
    function removeMinter(address) external override { }
    function renounceMinter() external override { }

    function isMinter(address) external pure override returns (bool) {
        return true;
    }

    function permit(address, address, uint256, uint256, uint8, bytes32, bytes32) external override { }

    function increaseAllowance(address spender, uint256 added) external override returns (bool) {
        _approve(msg.sender, spender, allowance(msg.sender, spender) + added);
        return true;
    }

    function decreaseAllowance(address spender, uint256 subtracted) external override returns (bool) {
        _approve(msg.sender, spender, allowance(msg.sender, spender) - subtracted);
        return true;
    }
}

// ─────────────────────────────────────────────────────────────────────────────
// MockPaymentsEscrow
//
// Implements IPaymentsEscrow in full.  The key methods are real:
//   deposit()  — pulls GRT from the payer and credits the [payer][collector][receiver] bucket.
//   collect()  — debits the bucket and distributes GRT:
//                  dataServiceCut PPM → dataService address
//                  remainder          → receiverDestination (or receiver if zero)
//
// The GraphTallyCollector calls collect() with msg.sender == tallyCollector, so the
// collector key in the balance map is the GraphTallyCollector address.
// ─────────────────────────────────────────────────────────────────────────────
contract MockPaymentsEscrow is IPaymentsEscrow {
    IERC20 public grt;

    /// @dev balances[payer][collector][receiver]
    mapping(address => mapping(address => mapping(address => uint256))) public rawBalances;

    constructor(IERC20 _grt) {
        grt = _grt;
    }

    // ── IPaymentsEscrow: real implementations ────────────────────────────────

    function deposit(address collector, address receiver, uint256 tokens) external override {
        grt.transferFrom(msg.sender, address(this), tokens);
        rawBalances[msg.sender][collector][receiver] += tokens;
        emit Deposit(msg.sender, collector, receiver, tokens);
    }

    function depositTo(address payer, address collector, address receiver, uint256 tokens) external override {
        grt.transferFrom(msg.sender, address(this), tokens);
        rawBalances[payer][collector][receiver] += tokens;
        emit Deposit(payer, collector, receiver, tokens);
    }

    /// @notice Called by GraphTallyCollector (msg.sender == collector).
    function collect(
        IGraphPayments.PaymentTypes paymentType,
        address payer,
        address receiver,
        uint256 tokens,
        address dataService,
        uint256 dataServiceCut,
        address receiverDestination
    ) external override {
        address collector = msg.sender;
        uint256 balance = rawBalances[payer][collector][receiver];
        if (balance < tokens) revert PaymentsEscrowInsufficientBalance(balance, tokens);
        rawBalances[payer][collector][receiver] -= tokens;

        uint256 dsTokens = (tokens * dataServiceCut) / 1_000_000;
        uint256 toReceiver = tokens - dsTokens;
        address dest = receiverDestination == address(0) ? receiver : receiverDestination;

        if (dsTokens > 0) grt.transfer(dataService, dsTokens);
        if (toReceiver > 0) grt.transfer(dest, toReceiver);

        emit EscrowCollected(paymentType, payer, collector, receiver, tokens, receiverDestination);
    }

    function getBalance(address payer, address collector, address receiver) external view override returns (uint256) {
        return rawBalances[payer][collector][receiver];
    }

    function escrowAccounts(address payer, address collector, address receiver)
        external
        view
        override
        returns (uint256 balance, uint256 tokensThawing, uint256 thawEndTimestamp)
    {
        return (rawBalances[payer][collector][receiver], 0, 0);
    }

    // ── IPaymentsEscrow: stubs (not exercised in these tests) ────────────────

    function thaw(address, address, uint256) external override { }

    function adjustThaw(address, address, uint256, bool) external override returns (uint256) {
        return 0;
    }

    function cancelThaw(address, address) external override { }
    function withdraw(address, address) external override { }
    function initialize() external override { }

    function MAX_WAIT_PERIOD() external pure override returns (uint256) {
        return 90 days;
    }

    function WITHDRAW_ESCROW_THAWING_PERIOD() external pure override returns (uint256) {
        return 7 days;
    }
}

// ─────────────────────────────────────────────────────────────────────────────
// MockGraphPayments
//
// GraphDirectory stores a GraphPayments address; it must be non-zero and respond
// to PROTOCOL_PAYMENT_CUT() without reverting.  GraphTallyCollector calls the
// escrow (not GraphPayments) directly, so this is a minimal stub.
// ─────────────────────────────────────────────────────────────────────────────
contract MockGraphPayments is IGraphPayments {
    function collect(PaymentTypes, address, uint256, address, uint256, address) external override { }

    function PROTOCOL_PAYMENT_CUT() external pure override returns (uint256) {
        return 0;
    }

    function initialize() external override { }
}

// ─────────────────────────────────────────────────────────────────────────────
// GraphDirectory peripheral stubs
// Non-zero addresses that satisfy the GraphDirectory constructor's zero-check.
// ─────────────────────────────────────────────────────────────────────────────
contract MockEpochManager {
    function currentEpoch() external pure returns (uint256) {
        return 1;
    }
}

contract MockRewardsManager {
    function onSubgraphAllocationUpdate(bytes32) external pure returns (uint256) {
        return 0;
    }
}

contract MockTokenGateway { }

contract MockProxyAdmin { }

// ─────────────────────────────────────────────────────────────────────────────
// HorizonStakingMock
//
// Implements the subset of IHorizonStaking called by ProvisionManager:
//   isAuthorized, getProvision, acceptProvisionParameters, slash.
// A fallback absorbs any other calls from DataService internals.
// ─────────────────────────────────────────────────────────────────────────────
contract HorizonStakingMock {
    struct SlashEvent {
        address serviceProvider;
        uint256 tokens;
        uint256 reward;
        address verifierDestination;
    }

    mapping(address => mapping(address => IHorizonStakingTypes.Provision)) private _provisions;
    mapping(address => mapping(address => mapping(address => bool))) private _operators;

    SlashEvent[] public slashEvents;

    // ── Test helpers ─────────────────────────────────────────────────────────

    function setProvision(address sp, address verifier, IHorizonStakingTypes.Provision memory p) external {
        _provisions[sp][verifier] = p;
    }

    function setOperator(address sp, address verifier, address operator, bool auth) external {
        _operators[sp][verifier][operator] = auth;
    }

    // ── IHorizonStaking surface used by DataService / ProvisionManager ───────

    function isAuthorized(address sp, address verifier, address operator) external view returns (bool) {
        if (operator == sp) return true;
        return _operators[sp][verifier][operator];
    }

    function getProvision(address sp, address verifier) external view returns (IHorizonStakingTypes.Provision memory) {
        return _provisions[sp][verifier];
    }

    function acceptProvisionParameters(address) external { }

    function getProviderTokensAvailable(address sp, address verifier) external view returns (uint256) {
        IHorizonStakingTypes.Provision memory p = _provisions[sp][verifier];
        return p.tokens > p.tokensThawing ? p.tokens - p.tokensThawing : 0;
    }

    function slash(address serviceProvider, uint256 tokens, uint256 reward, address verifierDestination) external {
        slashEvents.push(SlashEvent(serviceProvider, tokens, reward, verifierDestination));
    }

    function slashEventCount() external view returns (uint256) {
        return slashEvents.length;
    }

    fallback() external { }
}

// ─────────────────────────────────────────────────────────────────────────────
// ControllerMock
// ─────────────────────────────────────────────────────────────────────────────
contract ControllerMock is IController {
    mapping(bytes32 => address) private _registry;

    function setContractProxy(bytes32 id, address addr) external override {
        _registry[id] = addr;
    }

    function getContractProxy(bytes32 id) external view override returns (address) {
        return _registry[id];
    }

    function getGovernor() external pure override returns (address) {
        return address(0);
    }

    function setPaused(bool) external override { }
    function setPartialPaused(bool) external override { }

    function paused() external pure override returns (bool) {
        return false;
    }

    function partialPaused() external pure override returns (bool) {
        return false;
    }

    function setPauseGuardian(address) external override { }
    function unsetContractProxy(bytes32) external override { }
    function updateController(bytes32, address) external override { }
}

// ─────────────────────────────────────────────────────────────────────────────
// HorizonTestBase
//
// Abstract base for payment tests.  Deploys the REAL GraphTallyCollector so
// that EIP-712 RAV verification, authorizeSigner proof checking, and the
// cumulative-RAV monotonicity tracking are all exercised against production
// contract code — not mocks.
//
// Only the escrow and peripheral infrastructure are mocked; the collector and
// FirehoseDataService are the real artefacts under test.
// ─────────────────────────────────────────────────────────────────────────────
abstract contract HorizonTestBase is Test {
    // ── Deployed contracts ───────────────────────────────────────────────────
    MockGRTToken internal grt;
    MockPaymentsEscrow internal escrow;
    MockGraphPayments internal graphPayments;
    HorizonStakingMock internal staking;
    ControllerMock internal controller;
    GraphTallyCollector internal tallyCollector;
    FirehoseDataService internal svc;

    // ── Shared participants ──────────────────────────────────────────────────
    address internal governance = makeAddr("governance");
    address internal indexer = makeAddr("indexer");
    address internal payee = makeAddr("payee");

    bytes32 internal constant ETHEREUM_MAINNET = bytes32(uint256(1));

    function setUp() public virtual {
        // 1. Token + escrow.
        grt = new MockGRTToken();
        escrow = new MockPaymentsEscrow(IERC20(address(grt)));
        graphPayments = new MockGraphPayments();

        // 2. Staking.
        staking = new HorizonStakingMock();

        // 3. Controller — must register all eight addresses that GraphDirectory
        //    reads in its constructor (none may be address(0)).
        controller = new ControllerMock();
        controller.setContractProxy(keccak256("GraphToken"), address(grt));
        controller.setContractProxy(keccak256("Staking"), address(staking));
        controller.setContractProxy(keccak256("GraphPayments"), address(graphPayments));
        controller.setContractProxy(keccak256("PaymentsEscrow"), address(escrow));
        controller.setContractProxy(keccak256("EpochManager"), address(new MockEpochManager()));
        controller.setContractProxy(keccak256("RewardsManager"), address(new MockRewardsManager()));
        controller.setContractProxy(keccak256("GraphTokenGateway"), address(new MockTokenGateway()));
        controller.setContractProxy(keccak256("GraphProxyAdmin"), address(new MockProxyAdmin()));

        // 4. Real GraphTallyCollector — EIP-712 name/version must match the
        //    canonical contract so that consumer-side receipt signing works.
        //    Constructor: (eip712Name, eip712Version, controller, revokeSignerThawingPeriod)
        tallyCollector = new GraphTallyCollector(
            "GraphTallyCollector",
            "1",
            address(controller),
            24 hours // revokeSignerThawingPeriod
        );

        // 5. FirehoseDataService wired to the real collector.
        svc = new FirehoseDataService(address(controller), address(tallyCollector), governance);

        // 6. Ethereum mainnet chain manifest (Phase 1 governance allow-list).
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

        // 7. Indexer provision that satisfies all three ProvisionManager guards
        //    (MIN_PROVISION_TOKENS, MIN_THAWING_PERIOD, MAX_VERIFIER_CUT_PPM).
        staking.setProvision(
            indexer,
            address(svc),
            IHorizonStakingTypes.Provision({
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
            })
        );

        // 8. Register + start the indexer.
        vm.prank(indexer);
        svc.register(
            indexer, abi.encode("https://indexer.example", FirehoseDataService.Tier.Reputation, uint32(0), payee)
        );
        vm.prank(indexer);
        svc.startService(indexer, "");
    }

    // ── Helper: build the authorizeSigner proof ──────────────────────────────
    //
    // The proof is: ecSign(keccak256("\x19Ethereum Signed Message:\n32" ||
    //               keccak256(chainId || collectorAddr || "authorizeSignerProof" || deadline || authorizer)))
    // signed by the SIGNER key, proving the signer consents to be authorized.

    function _signerProof(uint256 signerPk, address authorizer, uint256 deadline) internal view returns (bytes memory) {
        bytes32 msgHash = keccak256(
            abi.encodePacked(block.chainid, address(tallyCollector), "authorizeSignerProof", deadline, authorizer)
        );
        bytes32 ethHash = keccak256(abi.encodePacked("\x19Ethereum Signed Message:\n32", msgHash));
        (uint8 v, bytes32 r, bytes32 s) = vm.sign(signerPk, ethHash);
        return abi.encodePacked(r, s, v);
    }

    // ── Helper: build a RAV targeting our FirehoseDataService ────────────────

    function _buildRAV(address payer_, address serviceProvider_, bytes32 collectionId_, uint128 valueAggregate_)
        internal
        view
        returns (IGraphTallyCollector.ReceiptAggregateVoucher memory)
    {
        return IGraphTallyCollector.ReceiptAggregateVoucher({
            collectionId: collectionId_,
            payer: payer_,
            serviceProvider: serviceProvider_,
            dataService: address(svc),
            timestampNs: uint64(block.timestamp * 1_000_000_000),
            valueAggregate: valueAggregate_,
            metadata: ""
        });
    }

    // ── Helper: sign a RAV with a given key ──────────────────────────────────
    //
    // tallyCollector.encodeRAV() returns the full EIP-712 typed-data digest
    // (0x1901 || domainSeparator || structHash), which vm.sign() signs directly
    // without adding any additional prefix — exactly what ECDSA.recover expects.

    function _signRAV(IGraphTallyCollector.ReceiptAggregateVoucher memory rav, uint256 pk)
        internal
        view
        returns (IGraphTallyCollector.SignedRAV memory)
    {
        bytes32 digest = tallyCollector.encodeRAV(rav);
        (uint8 v, bytes32 r, bytes32 s) = vm.sign(pk, digest);
        return IGraphTallyCollector.SignedRAV({ rav: rav, signature: abi.encodePacked(r, s, v) });
    }

    // ── Helper: encode the calldata for FirehoseDataService.collect() ─────────

    function _collectData(IGraphTallyCollector.SignedRAV memory signedRav, uint256 dataServiceCut)
        internal
        pure
        returns (bytes memory)
    {
        return abi.encode(signedRav, dataServiceCut);
    }
}
