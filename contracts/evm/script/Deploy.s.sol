// SPDX-License-Identifier: MIT
pragma solidity 0.8.24;

import {Script} from "forge-std/Script.sol";
import {IntentManager} from "../src/IntentManager.sol";

contract Deploy is Script {
    function run() external {
        vm.startBroadcast();
        new IntentManager(
            0x8cC88Bd20AB788c61Efd92Db8B680FeEc9E3AeBA,
            0x6B42cc3091Df5B54F24455B936ea556dA3d6dfd1,
            0x8cC88Bd20AB788c61Efd92Db8B680FeEc9E3AeBA
        );
        vm.stopBroadcast();
    }
}
