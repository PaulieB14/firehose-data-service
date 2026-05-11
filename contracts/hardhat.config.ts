// Hardhat config stub. Real config should match graphprotocol/contracts
// SubgraphService config closely.
import type { HardhatUserConfig } from "hardhat/config";

const config: HardhatUserConfig = {
  solidity: {
    version: "0.8.27",
    settings: {
      optimizer: { enabled: true, runs: 200 },
    },
  },
  paths: {
    sources: "./",
    tests: "./test",
  },
  networks: {
    // arbitrumSepolia: { url: process.env.ARBITRUM_SEPOLIA_RPC, accounts: [process.env.DEPLOYER_KEY!] },
    // arbitrumOne:     { url: process.env.ARBITRUM_ONE_RPC,     accounts: [process.env.DEPLOYER_KEY!] },
  },
};

export default config;
