# Convenience targets for local development.
#
# `make devnet` brings up an Anvil node and deploys the full Mainline contract
# stack against in-memory Horizon mocks, mirroring the Phase 0 happy path
# end-to-end on your laptop. Useful when you want to point mainline-service
# at a *real* chain without the cost of an Arbitrum Sepolia round-trip.

.PHONY: help test build devnet devnet-up devnet-deploy lint subgraph

ANVIL_PORT ?= 8545
ANVIL_CHAIN_ID ?= 421614
ANVIL_DEPLOYER_KEY ?= 0xac0974bec39a17e36ba4a6b4d238ff944bacb478cbed5efcae784d7bf4f2ff80

help:
	@echo "Targets:"
	@echo "  test            Run the full test suite (contracts, rust crates, ts sdk, subgraph)"
	@echo "  build           Compile all packages"
	@echo "  devnet          Bring up anvil + run the LocalDevnet script end-to-end"
	@echo "  subgraph        Build the network subgraph (graph codegen + graph build)"

build:
	cd contracts && forge build
	cd mainline-service && cargo build
	cd mainline-gateway && cargo build
	cd mainline-sdk/rust && cargo build
	cd mainline-sdk/typescript && npm install --silent && npx tsc --noEmit
	$(MAKE) subgraph

test:
	cd contracts && forge test
	cd mainline-service && cargo test --no-fail-fast
	cd mainline-gateway && cargo test --no-fail-fast
	cd mainline-sdk/rust && cargo test --no-fail-fast
	cd mainline-sdk/typescript && npm install --silent && npx tsc --noEmit

subgraph:
	cd subgraph && npm install --silent && npx graph codegen && npx graph build

# One-shot devnet: starts anvil, runs the bring-up script, leaves anvil running.
# Stop with `pkill anvil` when you're done.
devnet:
	@if pgrep -f "anvil --port $(ANVIL_PORT)" >/dev/null; then \
		echo "anvil already running on port $(ANVIL_PORT)"; \
	else \
		echo "starting anvil on port $(ANVIL_PORT) (chain-id $(ANVIL_CHAIN_ID))..."; \
		anvil --port $(ANVIL_PORT) --chain-id $(ANVIL_CHAIN_ID) --silent & \
		sleep 2; \
	fi
	$(MAKE) devnet-deploy

devnet-deploy:
	cd contracts && forge script script/LocalDevnet.s.sol:LocalDevnet \
	  --rpc-url http://127.0.0.1:$(ANVIL_PORT) \
	  --broadcast \
	  --private-key $(ANVIL_DEPLOYER_KEY) \
	  -vvv

# Helper to read the devnet deployment addresses out of the broadcast log.
# Useful for piping into mainline-service env vars.
devnet-env:
	@jq -r '.transactions[] | select(.transactionType=="CREATE") | "\(.contractName)=\(.contractAddress)"' \
	  contracts/broadcast/LocalDevnet.s.sol/$(ANVIL_CHAIN_ID)/run-latest.json
