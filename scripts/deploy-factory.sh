#!/usr/bin/env bash
# Deploys the immutable payrole-factory and stores the payrole-nft child wasm in one init call.
#
# Usage:
#   MASTER_ACCOUNT=dao.testnet ./scripts/deploy-factory.sh
#   MASTER_ACCOUNT=dao.testnet FACTORY_ACCOUNT=payrole-factory.testnet ./scripts/deploy-factory.sh
#
# The factory has no owner and no way to replace the child code after this call. If the account
# already exists this script stops instead of risking a mixed or unexpected deployment. The init
# deposit (default 5 NEAR) covers the stored wasm (~4.1 NEAR); anything left over is permanently
# locked because the factory has no withdrawal method.
set -euo pipefail

cd "$(dirname "$0")/.."

NETWORK="${NEAR_NETWORK:-testnet}"
FACTORY_ACCOUNT="${FACTORY_ACCOUNT:-payrole-factory.$NETWORK}"
MASTER_ACCOUNT="${MASTER_ACCOUNT:-}"
CHILD_WASM="${CHILD_WASM:-res/payrole_nft.wasm}"
FACTORY_WASM="${FACTORY_WASM:-res/payrole_factory.wasm}"
INIT_BALANCE="${INIT_BALANCE:-1}"
INIT_DEPOSIT="${INIT_DEPOSIT:-5}"

if [ -z "$MASTER_ACCOUNT" ]; then
  echo "Set MASTER_ACCOUNT to the account that creates $FACTORY_ACCOUNT." >&2
  exit 1
fi
if [ ! -f "$CHILD_WASM" ]; then
  echo "Missing $CHILD_WASM; run ./scripts/build-contract.sh first." >&2
  exit 1
fi
if [ ! -f "$FACTORY_WASM" ]; then
  echo "Missing $FACTORY_WASM; run ./scripts/build-contract.sh first." >&2
  exit 1
fi
if near state "$FACTORY_ACCOUNT" --networkId "$NETWORK" >/dev/null 2>&1; then
  echo "$FACTORY_ACCOUNT already exists." >&2
  echo "This factory is immutable. Choose a new FACTORY_ACCOUNT instead." >&2
  exit 1
fi

INIT_ARGS="$(
  python3 - "$CHILD_WASM" <<'PY'
import base64
import json
import sys

with open(sys.argv[1], "rb") as wasm:
    code = base64.b64encode(wasm.read()).decode("ascii")
print(json.dumps({"code": code}))
PY
)"

near create-account "$FACTORY_ACCOUNT" \
  --masterAccount "$MASTER_ACCOUNT" \
  --initialBalance "$INIT_BALANCE" \
  --networkId "$NETWORK"

near deploy "$FACTORY_ACCOUNT" "$FACTORY_WASM" \
  --initFunction new \
  --initArgs "$INIT_ARGS" \
  --initDeposit "$INIT_DEPOSIT" \
  --networkId "$NETWORK"

echo "Deployed $FACTORY_ACCOUNT with child code from $CHILD_WASM"
echo "Verify the pinned hash with: near view $FACTORY_ACCOUNT code_hash --networkId $NETWORK"
