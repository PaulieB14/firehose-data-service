// SPDX-License-Identifier: GPL-2.0-or-later
pragma solidity 0.8.27;

// Foundry deployment script for FirehoseDataService.
//
// Usage (Arbitrum Sepolia, Phase 0):
//   forge script script/Deploy.s.sol:Deploy \
//     --rpc-url $ARBITRUM_SEPOLIA_RPC \
//     --broadcast \
//     --private-key $DEPLOYER_KEY
//
// Required env vars:
//   GRAPH_CONTROLLER          Horizon controller address on the target chain.
//   GRAPH_TALLY_COLLECTOR     Deployed GraphTallyCollector address.
//   FIREHOSE_GOVERNANCE       Address allowed to register chain manifests in Phase 1.

import { Script } from "forge-std/Script.sol";
import { console2 } from "forge-std/console2.sol";

import { FirehoseDataService } from "../FirehoseDataService.sol";

contract Deploy is Script {
    function run() external {
        address controller = vm.envAddress("GRAPH_CONTROLLER");
        address graphTallyCollector = vm.envAddress("GRAPH_TALLY_COLLECTOR");
        address governance = vm.envAddress("FIREHOSE_GOVERNANCE");

        vm.startBroadcast();

        FirehoseDataService svc = new FirehoseDataService(controller, graphTallyCollector, governance);

        console2.log("FirehoseDataService deployed at:", address(svc));
        console2.log("  controller:", controller);
        console2.log("  graphTallyCollector:", graphTallyCollector);
        console2.log("  governance:", governance);

        vm.stopBroadcast();
    }
}
