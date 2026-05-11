// SPDX-License-Identifier: GPL-2.0-or-later
pragma solidity 0.8.27;

// Foundry deployment script for FirehoseDataService.
//
// Usage (once @graphprotocol/horizon is wired):
//   forge script contracts/script/Deploy.s.sol:Deploy \
//     --rpc-url $ARBITRUM_SEPOLIA_RPC \
//     --broadcast \
//     --private-key $DEPLOYER_KEY
//
// Phase 0 target: Arbitrum Sepolia.
// Phase 1 target: Arbitrum One.

import "forge-std/Script.sol";
import { FirehoseDataService } from "../FirehoseDataService.sol";

contract Deploy is Script {
    function run() external {
        vm.startBroadcast();

        FirehoseDataService svc = new FirehoseDataService();

        // TODO once contract is initializable:
        //   svc.initialize({
        //     controller: address(...),     // Graph controller on the target chain
        //     minProvision: 25_000 ether,
        //     ...
        //   });

        console2.log("FirehoseDataService deployed at:", address(svc));

        vm.stopBroadcast();
    }
}
