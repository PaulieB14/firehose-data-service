#!/usr/bin/env bash
#
# Refreshes the vendored copy of @graphprotocol/contracts (horizon + interfaces).
# Prunes everything that isn't required to compile FirehoseDataService.sol.
#
# Run from `contracts/`:
#     ./scripts/update-horizon.sh [ref]
#
# `ref` defaults to `main`.

set -euo pipefail

REF="${1:-main}"
TMP="$(mktemp -d)"
DEST="$(cd "$(dirname "$0")/.." && pwd)/lib/contracts"

cleanup() { rm -rf "$TMP"; }
trap cleanup EXIT

echo "→ Cloning graphprotocol/contracts@$REF"
git clone --depth 1 --branch "$REF" https://github.com/graphprotocol/contracts.git "$TMP/contracts" 2>&1 | tail -2

echo "→ Wiping previous vendor at $DEST"
rm -rf "$DEST"
mkdir -p "$DEST/packages"

echo "→ Copying horizon + interfaces"
cp -r "$TMP/contracts/packages/horizon" "$DEST/packages/horizon"
cp -r "$TMP/contracts/packages/interfaces" "$DEST/packages/interfaces"

echo "→ Pruning non-essentials"
( cd "$DEST/packages/horizon" && rm -rf ignition audits test scripts tasks types )
( cd "$DEST/packages/horizon/contracts" && rm -rf mocks staking )
rm -f "$DEST/packages/horizon/contracts/payments/GraphPayments.sol"
rm -f "$DEST/packages/horizon/contracts/payments/PaymentsEscrow.sol"
rm -f "$DEST/packages/horizon/contracts/payments/collectors/RecurringCollector.sol"
( cd "$DEST/packages/interfaces/contracts" && rm -rf toolshed issuance subgraph-service token-distribution contracts/curation )

# Remove any other files that import the legacy @graphprotocol/contracts root
# (the legacy v0.7 stack that we don't vendor).
grep -rl "@graphprotocol/contracts/" "$DEST/packages/horizon" 2>/dev/null | xargs rm -f || true

echo "✓ Updated horizon vendor at $DEST"
du -sh "$DEST"
