#!/usr/bin/env bash
# Deploys the permissionless-mint demo NEP-141 token used by the hosted PayRole walkthrough.
#
# Usage:
#   MASTER_ACCOUNT=dao.testnet ./scripts/deploy-salary-token.sh
#   MASTER_ACCOUNT=dao.testnet TOKEN_ACCOUNT=payrole-demo-token.dao.testnet \
#     ./scripts/deploy-salary-token.sh
#
# The token is the bundled test-contracts/mock-ft contract. Anyone can mint it, which is what
# lets the web app offer a free "Fund 100" button for demo contracts. Never use it for real
# value. The script prints the PUBLIC_* variables to set when building the web app.
set -euo pipefail

cd "$(dirname "$0")/.."

NETWORK="${NEAR_NETWORK:-testnet}"
MASTER_ACCOUNT="${MASTER_ACCOUNT:-plopmenz.$NETWORK}"
TOKEN_ACCOUNT="${TOKEN_ACCOUNT:-payrole-mock-ft.$NETWORK}"
TOKEN_WASM="${TOKEN_WASM:-res/mock_ft.wasm}"
INITIAL_BALANCE="${INITIAL_BALANCE:-3}"
DEMO_FUND_AMOUNT="${DEMO_FUND_AMOUNT:-100}"

if [ -z "$MASTER_ACCOUNT" ]; then
  echo "Set MASTER_ACCOUNT to the account that creates $TOKEN_ACCOUNT." >&2
  exit 1
fi
if [ ! -f "$TOKEN_WASM" ]; then
  echo "Missing $TOKEN_WASM; run ./scripts/build-contract.sh first." >&2
  exit 1
fi
if near state "$TOKEN_ACCOUNT" --networkId "$NETWORK" >/dev/null 2>&1; then
  echo "$TOKEN_ACCOUNT already exists." >&2
  echo "Choose another TOKEN_ACCOUNT or reuse the existing token." >&2
  exit 1
fi

near create-account "$TOKEN_ACCOUNT" \
  --masterAccount "$MASTER_ACCOUNT" \
  --initialBalance "$INITIAL_BALANCE" \
  --networkId "$NETWORK"

near deploy "$TOKEN_ACCOUNT" "$TOKEN_WASM" \
  --initFunction new \
  --initArgs '{"seed":1}' \
  --networkId "$NETWORK"

echo "Deployed demo NEP-141 token $TOKEN_ACCOUNT"
echo
echo "Set these when building the web app:"
echo "PUBLIC_SALARY_TOKEN_ID=$TOKEN_ACCOUNT"
echo "PUBLIC_SALARY_TOKEN_DECIMALS=6"
echo "PUBLIC_SALARY_TOKEN_SYMBOL=MOCK"
echo "PUBLIC_DEMO_FUND_AMOUNT=$DEMO_FUND_AMOUNT"
echo
echo "Verify with:"
echo "near view $TOKEN_ACCOUNT ft_metadata --networkId $NETWORK"
