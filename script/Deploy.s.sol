// SPDX-License-Identifier: Apache-2.0
pragma solidity ^0.8.27;

import {Script, console} from "forge-std/Script.sol";
import {ERC1967Proxy} from "@openzeppelin/contracts/proxy/ERC1967/ERC1967Proxy.sol";
import {SolanaDataService} from "../contracts/SolanaDataService.sol";

/// @notice Deploys SolanaDataService as a UUPS proxy and configures the initial program allowlist.
///
/// Required env vars:
///   CONTROLLER          — Horizon GraphController address on target network
///   GRAPH_TALLY_COLLECTOR — GraphTallyCollector address on target network
///   OWNER               — Initial owner (multisig or deployer EOA)
///   PAUSE_GUARDIAN      — Address authorised to pause the contract
///
/// Optional:
///   PROGRAMS_JSON       — comma-separated list of Solana program IDs to allowlist (default: pump.fun, raydium, jupiter)
///
/// Usage (Arbitrum Sepolia):
///   forge script script/Deploy.s.sol \
///     --rpc-url $ARB_SEPOLIA_RPC \
///     --broadcast \
///     --verify \
///     --etherscan-api-key $ARBISCAN_KEY
///
contract Deploy is Script {
    // Solana program IDs for the initial allowlist
    string[] internal programs;

    function run() external {
        address controller = vm.envAddress("CONTROLLER");
        address graphTallyCollector = vm.envAddress("GRAPH_TALLY_COLLECTOR");
        address owner = vm.envAddress("OWNER");
        address pauseGuardian = vm.envAddress("PAUSE_GUARDIAN");

        // Default program allowlist: the three highest-volume Solana DEX programs
        programs.push("6EF8rrecthR5Dkzon8Nwu78hRvfCKubJ14M5uBEwF6P"); // Pump.fun
        programs.push("CAMMCzo5YL8w4VFF8KVHrK22GGUsp5VTaW7grrKgrWqK"); // Raydium CLMM
        programs.push("JUP6LkbZbjS1jKKwapdHNy74zcZ3tLUZoi5QNyVTaV4"); // Jupiter v6

        vm.startBroadcast();

        // 1. Deploy implementation
        SolanaDataService impl = new SolanaDataService(controller, graphTallyCollector);

        // 2. Deploy UUPS proxy, calling initialize in the same tx
        bytes memory initData = abi.encodeCall(SolanaDataService.initialize, (owner, pauseGuardian));
        SolanaDataService proxy = SolanaDataService(address(new ERC1967Proxy(address(impl), initData)));

        // 3. Allowlist programs (owner is msg.sender during broadcast)
        for (uint256 i = 0; i < programs.length; i++) {
            proxy.addProgram(programs[i]);
        }

        vm.stopBroadcast();

        console.log("SolanaDataService implementation:", address(impl));
        console.log("SolanaDataService proxy:         ", address(proxy));
        console.log("Owner:                           ", owner);
        console.log("PauseGuardian:                   ", pauseGuardian);
        console.log("Programs allowlisted:            ", programs.length);
    }
}
