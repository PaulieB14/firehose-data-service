// SPDX-License-Identifier: GPL-2.0-or-later
pragma solidity 0.8.27;

// Spin up the full GRC-006 Phase 0 happy path on a local Anvil node.
//
// Usage:
//
//   # 1) start anvil in another terminal
//   anvil --chain-id 421614 --port 8545
//
//   # 2) run the devnet bring-up
//   forge script script/LocalDevnet.s.sol:LocalDevnet \
//     --rpc-url http://127.0.0.1:8545 \
//     --broadcast \
//     --private-key 0xac0974bec39a17e36ba4a6b4d238ff944bacb478cbed5efcae784d7bf4f2ff80 \
//     -vvvv
//
// The script deploys the FirehoseDataService against in-memory mocks for the
// Horizon dependencies, registers Ethereum mainnet, brings up one indexer,
// collects a signed RAV, and prints the live addresses + tx hashes. The
// same flow runs in seconds inside `forge test`
// (`test/FirehoseDataServiceIntegration.t.sol`) but this script gives you
// a *live* anvil deployment to point mainline-service / SDK clients at.

import { Script } from "forge-std/Script.sol";
import { console2 } from "forge-std/console2.sol";

import { IHorizonStakingTypes } from "@graphprotocol/interfaces/contracts/horizon/internal/IHorizonStakingTypes.sol";
import { IGraphPayments } from "@graphprotocol/interfaces/contracts/horizon/IGraphPayments.sol";
import { IGraphTallyCollector } from "@graphprotocol/interfaces/contracts/horizon/IGraphTallyCollector.sol";

import { FirehoseDataService } from "../FirehoseDataService.sol";
import {
    MockController, MockHorizonStaking, MockGraphTallyCollector
} from "../test/FirehoseDataServiceIntegration.t.sol";

contract LocalDevnet is Script {
    bytes32 internal constant ETHEREUM_MAINNET = bytes32(uint256(1));
    uint64 internal constant ADVERTISED_LIB = 18_000_000;
    uint256 internal constant TOKENS_COLLECTED = 1_000_000;

    function run() external {
        vm.startBroadcast();

        // ── Deploy the mock Horizon stack ─────────────────────────────────
        MockController controller = new MockController();
        MockHorizonStaking staking = new MockHorizonStaking();
        MockGraphTallyCollector collector = new MockGraphTallyCollector();

        controller.setContractProxy(keccak256("Staking"), address(staking));
        // GraphDirectory probes these via the controller; an address(0xdead)
        // is fine because we never call into them on the happy path.
        controller.setContractProxy(keccak256("GraphPayments"), address(0xdead));
        controller.setContractProxy(keccak256("PaymentsEscrow"), address(0xdead));
        controller.setContractProxy(keccak256("GraphToken"), address(0xdead));
        controller.setContractProxy(keccak256("EpochManager"), address(0xdead));
        controller.setContractProxy(keccak256("RewardsManager"), address(0xdead));
        controller.setContractProxy(keccak256("GraphTokenGateway"), address(0xdead));
        controller.setContractProxy(keccak256("GraphProxyAdmin"), address(0xdead));
        controller.setContractProxy(keccak256("Curation"), address(0xdead));

        address deployer = msg.sender;
        FirehoseDataService svc = new FirehoseDataService(address(controller), address(collector), deployer);

        console2.log("== devnet deploy ==");
        console2.log("FirehoseDataService:", address(svc));
        console2.log("MockController:     ", address(controller));
        console2.log("MockHorizonStaking: ", address(staking));
        console2.log("MockGraphTallyColl: ", address(collector));
        console2.log("Governance:         ", deployer);

        // ── Register Ethereum mainnet ─────────────────────────────────────
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
        console2.log("Registered chain: ethereum-mainnet (0x...01)");

        // ── Authorise & register a single indexer ─────────────────────────
        address indexer = deployer; // simplest: reuse the deployer EOA
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

        svc.register(
            indexer,
            abi.encode(
                "https://indexer.local",
                FirehoseDataService.Tier.Reputation,
                uint32(0),
                indexer // payments destination
            )
        );
        svc.startService(indexer, "");
        svc.advertiseChain(ETHEREUM_MAINNET, ADVERTISED_LIB);
        console2.log("Indexer registered + started:", indexer);
        console2.log("Advertised LIB:              ", uint256(ADVERTISED_LIB));

        // ── Settle a signed RAV ───────────────────────────────────────────
        IGraphTallyCollector.ReceiptAggregateVoucher memory rav = IGraphTallyCollector.ReceiptAggregateVoucher({
            collectionId: bytes32(uint256(1)),
            payer: indexer,
            serviceProvider: indexer,
            dataService: address(svc),
            timestampNs: uint64(block.timestamp * 1e9),
            valueAggregate: uint128(TOKENS_COLLECTED),
            metadata: ""
        });
        IGraphTallyCollector.SignedRAV memory signedRav =
            IGraphTallyCollector.SignedRAV({ rav: rav, signature: bytes(hex"deadbeef") });
        collector.setStubbedReturn(TOKENS_COLLECTED);
        uint256 paid = svc.collect(indexer, IGraphPayments.PaymentTypes.QueryFee, abi.encode(signedRav, uint256(0)));
        console2.log("RAV settled. tokens collected:", paid);

        console2.log("");
        console2.log("Devnet ready. Point mainline-service at:");
        console2.log("  MAINLINE_FDS_ADDRESS=", address(svc));
        console2.log("  MAINLINE_GRAPH_TALLY_COLLECTOR=", address(collector));
        console2.log("  MAINLINE_SETTLEMENT_CHAIN_ID=421614  # anvil default");

        vm.stopBroadcast();
    }
}
