// SPDX-License-Identifier: GPL-2.0-or-later
pragma solidity 0.8.27;

import { IGraphPayments } from "@graphprotocol/interfaces/contracts/horizon/IGraphPayments.sol";
import { IGraphTallyCollector } from "@graphprotocol/interfaces/contracts/horizon/IGraphTallyCollector.sol";
import { IHorizonStakingTypes } from "@graphprotocol/interfaces/contracts/horizon/internal/IHorizonStakingTypes.sol";

import { HorizonTestBase } from "./HorizonTestBase.t.sol";
import { FirehoseDataService } from "../FirehoseDataService.sol";

/**
 * @title FirehoseDataServicePayment
 * @notice End-to-end payment tests for FirehoseDataService using the REAL GraphTallyCollector.
 *
 * Every test that touches collect() goes through genuine EIP-712 RAV signing, real signer-
 * authorization proof verification, and a MockPaymentsEscrow that actually moves GRT tokens
 * — so the happy path exercises the full on-chain payment stack, not a mock shortcut.
 */
contract FirehoseDataServicePayment is HorizonTestBase {
    // ── Key-pair actors ──────────────────────────────────────────────────────
    uint256 internal signerPk;
    address internal signer;
    uint256 internal payerPk;
    address internal payer;

    bytes32 internal constant COLLECTION_ID = bytes32(uint256(0xc011));
    uint256 internal constant DATA_SERVICE_CUT_PPM = 50_000;  // 5%
    uint256 internal constant ESCROW_AMOUNT = 10_000 ether;

    function setUp() public override {
        super.setUp();

        (signer, signerPk) = makeAddrAndKey("signer");
        (payer,  payerPk)  = makeAddrAndKey("payer");

        // Mint GRT to payer and deposit into escrow for the indexer.
        grt.mint(payer, ESCROW_AMOUNT);
        vm.startPrank(payer);
        grt.approve(address(escrow), ESCROW_AMOUNT);
        // collector = tallyCollector, receiver = indexer
        escrow.deposit(address(tallyCollector), indexer, ESCROW_AMOUNT);
        vm.stopPrank();

        // Authorize signer on behalf of payer.
        uint256 deadline = block.timestamp + 1 hours;
        bytes memory proof = _signerProof(signerPk, payer, deadline);
        vm.prank(payer);
        tallyCollector.authorizeSigner(signer, deadline, proof);
    }

    // ─────────────────────────────────────────────────────────────────────────
    // Happy-path
    // ─────────────────────────────────────────────────────────────────────────

    /// @notice Full EIP-712 end-to-end: build RAV → sign with authorized signer → collect.
    function test_collect_happyPath_realEIP712EndToEnd() public {
        uint128 valueAggregate = 1_000 ether;
        IGraphTallyCollector.SignedRAV memory signedRav =
            _signRAV(_buildRAV(payer, indexer, COLLECTION_ID, valueAggregate), signerPk);

        uint256 payeeBefore   = grt.balanceOf(payee);
        uint256 govBefore     = grt.balanceOf(address(svc));
        uint256 escrowBefore  = escrow.getBalance(payer, address(tallyCollector), indexer);

        vm.prank(indexer);
        uint256 collected = svc.collect(
            indexer,
            IGraphPayments.PaymentTypes.QueryFee,
            _collectData(signedRav, DATA_SERVICE_CUT_PPM)
        );

        assertEq(collected, valueAggregate, "collected tokens mismatch");

        uint256 expectedDsCut    = (valueAggregate * DATA_SERVICE_CUT_PPM) / 1_000_000;
        uint256 expectedToPayee  = valueAggregate - expectedDsCut;

        assertEq(grt.balanceOf(payee),        payeeBefore  + expectedToPayee, "payee balance mismatch");
        assertEq(grt.balanceOf(address(svc)), govBefore    + expectedDsCut,   "data service cut mismatch");
        assertEq(
            escrow.getBalance(payer, address(tallyCollector), indexer),
            escrowBefore - valueAggregate,
            "escrow balance mismatch"
        );
    }

    /// @notice Escrow balance correctly decrements after collection.
    function test_collect_escrowBalanceDecrements() public {
        uint128 amount = 500 ether;
        IGraphTallyCollector.SignedRAV memory signedRav =
            _signRAV(_buildRAV(payer, indexer, COLLECTION_ID, amount), signerPk);

        uint256 before = escrow.getBalance(payer, address(tallyCollector), indexer);
        vm.prank(indexer);
        svc.collect(indexer, IGraphPayments.PaymentTypes.QueryFee, _collectData(signedRav, 0));

        assertEq(
            escrow.getBalance(payer, address(tallyCollector), indexer),
            before - amount
        );
    }

    /// @notice When dataServiceCut == 0, all tokens go to the payments destination.
    function test_collect_zeroDataServiceCut_allTokensToPayee() public {
        uint128 amount = 200 ether;
        IGraphTallyCollector.SignedRAV memory signedRav =
            _signRAV(_buildRAV(payer, indexer, COLLECTION_ID, amount), signerPk);

        vm.prank(indexer);
        svc.collect(indexer, IGraphPayments.PaymentTypes.QueryFee, _collectData(signedRav, 0));

        assertEq(grt.balanceOf(payee), amount, "all tokens should go to payee");
        assertEq(grt.balanceOf(address(svc)), 0, "data service should receive nothing");
    }

    /// @notice When dataServiceCut == 1_000_000 (100%), all tokens go to the data service.
    function test_collect_fullDataServiceCut_allTokensToDataService() public {
        uint128 amount = 300 ether;
        IGraphTallyCollector.SignedRAV memory signedRav =
            _signRAV(_buildRAV(payer, indexer, COLLECTION_ID, amount), signerPk);

        vm.prank(indexer);
        svc.collect(indexer, IGraphPayments.PaymentTypes.QueryFee, _collectData(signedRav, 1_000_000));

        assertEq(grt.balanceOf(address(svc)), amount, "all tokens should go to data service");
        assertEq(grt.balanceOf(payee), 0, "payee should receive nothing");
    }

    /// @notice Cumulative RAV: second collect sends only the delta since the first.
    function test_collect_cumulativeRav_deltaTransferred() public {
        uint128 first  = 400 ether;
        uint128 second = 700 ether;  // delta = 300 ether

        // Pre-compute signed RAVs BEFORE vm.prank so encodeRAV() doesn't consume the prank.
        IGraphTallyCollector.SignedRAV memory firstRav =
            _signRAV(_buildRAV(payer, indexer, COLLECTION_ID, first), signerPk);
        IGraphTallyCollector.SignedRAV memory secondRav =
            _signRAV(_buildRAV(payer, indexer, COLLECTION_ID, second), signerPk);

        vm.prank(indexer);
        svc.collect(indexer, IGraphPayments.PaymentTypes.QueryFee, _collectData(firstRav, 0));

        uint256 payeeAfterFirst = grt.balanceOf(payee);

        vm.prank(indexer);
        svc.collect(indexer, IGraphPayments.PaymentTypes.QueryFee, _collectData(secondRav, 0));

        assertEq(grt.balanceOf(payee), payeeAfterFirst + (second - first), "only delta should transfer");
    }

    // ─────────────────────────────────────────────────────────────────────────
    // Authorization errors
    // ─────────────────────────────────────────────────────────────────────────

    /// @notice RAV signed by an unauthorized key reverts.
    function test_collect_unauthorizedSigner_reverts() public {
        (, uint256 rogueKey) = makeAddrAndKey("rogue");
        IGraphTallyCollector.SignedRAV memory signedRav =
            _signRAV(_buildRAV(payer, indexer, COLLECTION_ID, 100 ether), rogueKey);

        vm.expectRevert(IGraphTallyCollector.GraphTallyCollectorInvalidRAVSigner.selector);
        vm.prank(indexer);
        svc.collect(indexer, IGraphPayments.PaymentTypes.QueryFee, _collectData(signedRav, 0));
    }

    /// @notice RAV signed by the correct key but for the wrong payer reverts.
    function test_collect_wrongSignerKey_reverts() public {
        (, uint256 otherKey) = makeAddrAndKey("otherPayer");
        // signerPk is authorized for `payer`, but we put a different payer in the RAV
        address otherPayer = makeAddr("otherPayer");
        IGraphTallyCollector.SignedRAV memory signedRav =
            _signRAV(_buildRAV(otherPayer, indexer, COLLECTION_ID, 100 ether), signerPk);

        // signer is not authorized for otherPayer
        vm.expectRevert(IGraphTallyCollector.GraphTallyCollectorInvalidRAVSigner.selector);
        vm.prank(indexer);
        svc.collect(indexer, IGraphPayments.PaymentTypes.QueryFee, _collectData(signedRav, 0));
        otherKey; // silence unused-variable warning
    }

    /// @notice authorizeSigner with an invalid proof reverts.
    function test_authorizeSigner_badProof_reverts() public {
        (address newSigner,) = makeAddrAndKey("newSigner");
        (, uint256 wrongKey) = makeAddrAndKey("wrongKey");

        uint256 deadline = block.timestamp + 1 hours;
        // Proof signed by wrongKey instead of newSigner's key — verification should fail
        bytes memory badProof = _signerProof(wrongKey, payer, deadline);

        vm.expectRevert(abi.encodeWithSignature("AuthorizableInvalidSignerProof()"));
        vm.prank(payer);
        tallyCollector.authorizeSigner(newSigner, deadline, badProof);
    }

    /// @notice After thawSigner + revokeAuthorizedSigner, collect reverts.
    function test_collect_revokedSigner_reverts() public {
        // Thaw the signer
        vm.prank(payer);
        tallyCollector.thawSigner(signer);

        // Fast-forward past the 24h thawing period
        vm.warp(block.timestamp + 24 hours + 1);

        // Revoke
        vm.prank(payer);
        tallyCollector.revokeAuthorizedSigner(signer);

        IGraphTallyCollector.SignedRAV memory signedRav =
            _signRAV(_buildRAV(payer, indexer, COLLECTION_ID, 100 ether), signerPk);

        vm.expectRevert(IGraphTallyCollector.GraphTallyCollectorInvalidRAVSigner.selector);
        vm.prank(indexer);
        svc.collect(indexer, IGraphPayments.PaymentTypes.QueryFee, _collectData(signedRav, 0));
    }

    // ─────────────────────────────────────────────────────────────────────────
    // RAV validity errors
    // ─────────────────────────────────────────────────────────────────────────

    /// @notice RAV with serviceProvider != indexer reverts with IndexerMismatch.
    function test_collect_serviceProviderMismatch_reverts() public {
        address wrongIndexer = makeAddr("wrongIndexer");
        IGraphTallyCollector.SignedRAV memory signedRav =
            _signRAV(_buildRAV(payer, wrongIndexer, COLLECTION_ID, 100 ether), signerPk);

        vm.expectRevert(
            abi.encodeWithSelector(
                FirehoseDataService.FirehoseDataServiceIndexerMismatch.selector,
                wrongIndexer,
                indexer
            )
        );
        vm.prank(indexer);
        svc.collect(indexer, IGraphPayments.PaymentTypes.QueryFee, _collectData(signedRav, 0));
    }

    /// @notice RAV targeting a different data service address reverts.
    function test_collect_wrongDataService_reverts() public {
        address wrongSvc = makeAddr("wrongSvc");
        IGraphTallyCollector.ReceiptAggregateVoucher memory rav = IGraphTallyCollector.ReceiptAggregateVoucher({
            collectionId:    COLLECTION_ID,
            payer:           payer,
            serviceProvider: indexer,
            dataService:     wrongSvc,          // <-- wrong
            timestampNs:     uint64(block.timestamp * 1_000_000_000),
            valueAggregate:  100 ether,
            metadata:        ""
        });
        IGraphTallyCollector.SignedRAV memory signedRav = _signRAV(rav, signerPk);

        // The collector rejects it because msg.sender (svc) != rav.dataService (wrongSvc)
        vm.expectRevert(
            abi.encodeWithSelector(
                IGraphTallyCollector.GraphTallyCollectorCallerNotDataService.selector,
                address(svc),
                wrongSvc
            )
        );
        vm.prank(indexer);
        svc.collect(indexer, IGraphPayments.PaymentTypes.QueryFee, _collectData(signedRav, 0));
    }

    /// @notice Monotonicity check: a second RAV with a lower value aggregate reverts.
    function test_collect_ravMonotonicity_reverts() public {
        uint128 first  = 700 ether;
        uint128 second = 400 ether;  // lower — must be rejected

        // Pre-compute both signed RAVs before issuing any prank.
        IGraphTallyCollector.SignedRAV memory firstRav =
            _signRAV(_buildRAV(payer, indexer, COLLECTION_ID, first), signerPk);

        vm.prank(indexer);
        svc.collect(indexer, IGraphPayments.PaymentTypes.QueryFee, _collectData(firstRav, 0));

        IGraphTallyCollector.SignedRAV memory secondRav =
            _signRAV(_buildRAV(payer, indexer, COLLECTION_ID, second), signerPk);

        vm.expectRevert(
            abi.encodeWithSelector(
                IGraphTallyCollector.GraphTallyCollectorInconsistentRAVTokens.selector,
                uint256(second),
                uint256(first)
            )
        );
        vm.prank(indexer);
        svc.collect(indexer, IGraphPayments.PaymentTypes.QueryFee, _collectData(secondRav, 0));
    }

    /// @notice Non-QueryFee payment type reverts with UnsupportedPaymentType.
    function test_collect_unsupportedPaymentType_reverts() public {
        IGraphTallyCollector.SignedRAV memory signedRav =
            _signRAV(_buildRAV(payer, indexer, COLLECTION_ID, 100 ether), signerPk);

        vm.expectRevert(
            abi.encodeWithSelector(
                FirehoseDataService.FirehoseDataServiceUnsupportedPaymentType.selector,
                IGraphPayments.PaymentTypes.IndexingFee
            )
        );
        vm.prank(indexer);
        svc.collect(indexer, IGraphPayments.PaymentTypes.IndexingFee, _collectData(signedRav, 0));
    }

    /// @notice Collecting more than the escrow balance reverts.
    function test_collect_insufficientEscrow_reverts() public {
        // Request more than the ESCROW_AMOUNT deposited
        uint128 overAmount = uint128(ESCROW_AMOUNT + 1 ether);

        // We need enough GRT minted for payer to actually create the RAV; the escrow check happens
        // inside MockPaymentsEscrow.collect() which is called by GraphTallyCollector after the RAV
        // checks pass.  The escrow currently holds ESCROW_AMOUNT for this payer/collector/indexer.
        IGraphTallyCollector.SignedRAV memory signedRav =
            _signRAV(_buildRAV(payer, indexer, COLLECTION_ID, overAmount), signerPk);

        vm.expectRevert(
            abi.encodeWithSelector(
                bytes4(keccak256("PaymentsEscrowInsufficientBalance(uint256,uint256)")),
                ESCROW_AMOUNT,
                overAmount
            )
        );
        vm.prank(indexer);
        svc.collect(indexer, IGraphPayments.PaymentTypes.QueryFee, _collectData(signedRav, 0));
    }

    // ─────────────────────────────────────────────────────────────────────────
    // Data-service lifecycle guards
    // ─────────────────────────────────────────────────────────────────────────

    /// @notice Unregistered indexer cannot collect.
    /// @dev collect() checks modifiers in order: onlyAuthorizedForProvision → onlyValidProvision
    ///      → onlyRegisteredIndexer.  We give `stranger` a valid provision so the first two pass
    ///      and the registration check is the one that fires.
    function test_collect_unregisteredIndexer_reverts() public {
        address stranger = makeAddr("stranger");

        // Give stranger a valid provision so the ProvisionManager checks pass.
        staking.setProvision(stranger, address(svc), IHorizonStakingTypes.Provision({
            tokens:                  25_000 ether,
            tokensThawing:           0,
            sharesThawing:           0,
            maxVerifierCut:          500_000,
            thawingPeriod:           21 days,
            createdAt:               uint64(block.timestamp),
            maxVerifierCutPending:   500_000,
            thawingPeriodPending:    21 days,
            lastParametersStagedAt:  uint64(block.timestamp),
            thawingNonce:            0
        }));

        IGraphTallyCollector.SignedRAV memory signedRav =
            _signRAV(_buildRAV(payer, stranger, COLLECTION_ID, 100 ether), signerPk);

        vm.expectRevert(
            abi.encodeWithSelector(
                FirehoseDataService.FirehoseDataServiceIndexerNotRegistered.selector,
                stranger
            )
        );
        vm.prank(stranger);
        svc.collect(stranger, IGraphPayments.PaymentTypes.QueryFee, _collectData(signedRav, 0));
    }

    /// @notice collect() is only callable by an authorised operator/indexer.
    function test_collect_unauthorizedOperator_reverts() public {
        address rando = makeAddr("rando");
        IGraphTallyCollector.SignedRAV memory signedRav =
            _signRAV(_buildRAV(payer, indexer, COLLECTION_ID, 100 ether), signerPk);

        // rando is not an authorized operator for indexer's provision
        vm.expectRevert(); // DataServiceNotAuthorized or similar from ProvisionManager
        vm.prank(rando);
        svc.collect(indexer, IGraphPayments.PaymentTypes.QueryFee, _collectData(signedRav, 0));
    }
}
