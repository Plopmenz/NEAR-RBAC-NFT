#!/usr/bin/env bash
# Deploys the immutable payrole-factory and stores the payrole-nft child wasm in one init call.
#
# Usage:
#   MASTER_ACCOUNT=dao.testnet ./scripts/deploy-factory.sh
#   MASTER_ACCOUNT=dao.testnet FACTORY_ACCOUNT=payrole-factory.testnet ./scripts/deploy-factory.sh
#
# The factory has no owner and no way to replace the child code after this call. If the account
# already exists this script stops instead of risking a mixed or unexpected deployment. The init
# deposit (default 5 NEAR) covers the stored wasm (~3.3 NEAR for the current build); any
# remainder is permanently locked because the factory has no withdrawal method. A newly created
# account is funded with the init deposit plus 1 NEAR so it can pay for the deploy transaction.
set -euo pipefail

cd "$(dirname "$0")/.."

NETWORK="${NEAR_NETWORK:-testnet}"
FACTORY_ACCOUNT="${FACTORY_ACCOUNT:-payrole-factory.$NETWORK}"
MASTER_ACCOUNT="${MASTER_ACCOUNT:-plopmenz.$NETWORK}"
CHILD_WASM="${CHILD_WASM:-res/payrole_nft.wasm}"
FACTORY_WASM="${FACTORY_WASM:-res/payrole_factory.wasm}"
INIT_DEPOSIT="${INIT_DEPOSIT:-5}"
INIT_BALANCE="${INIT_BALANCE:-}"

if [ -z "$MASTER_ACCOUNT" ]; then
  echo "Set MASTER_ACCOUNT to the account that creates $FACTORY_ACCOUNT." >&2
  exit 1
fi

# The new account must be able to attach INIT_DEPOSIT and still pay the deploy fee. One extra
# NEAR covers the fee and the account's own storage.
if [ -z "$INIT_BALANCE" ]; then
  INIT_BALANCE="$(
    python3 - "$INIT_DEPOSIT" <<'PY'
from decimal import Decimal
import sys

print(Decimal(sys.argv[1]) + Decimal(1))
PY
  )"
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

# The child wasm is several hundred KiB, so its base64 init JSON is far larger than the kernel's
# per-argument limit (~128 KiB). Write it to a file and pass it with `file-args`.
INIT_ARGS_FILE="$(mktemp "${TMPDIR:-/tmp}/payrole-factory-init.XXXXXX.json")"
trap 'rm -f "$INIT_ARGS_FILE"' EXIT

python3 - "$CHILD_WASM" "$INIT_ARGS_FILE" <<'PY'
import base64
import json
import sys

with open(sys.argv[1], "rb") as wasm:
    code = base64.b64encode(wasm.read()).decode("ascii")
with open(sys.argv[2], "w", encoding="utf-8") as out:
    json.dump({"code": code}, out)
PY

near create-account "$FACTORY_ACCOUNT" \
 --masterAccount "$MASTER_ACCOUNT" \
 --initialBalance "$INIT_BALANCE" \
 --networkId "$NETWORK"

near contract deploy "$FACTORY_ACCOUNT" \
  use-file "$FACTORY_WASM" \
  with-init-call new file-args "$INIT_ARGS_FILE" \
  prepaid-gas '150 Tgas' \
  attached-deposit "$INIT_DEPOSIT NEAR" \
  network-config "$NETWORK" \
  sign-with-keychain send

echo "Deployed $FACTORY_ACCOUNT with child code from $CHILD_WASM"
echo "Verify the pinned hash with: near view $FACTORY_ACCOUNT code_hash --networkId $NETWORK"
