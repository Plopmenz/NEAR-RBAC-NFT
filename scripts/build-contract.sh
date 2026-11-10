#!/usr/bin/env bash
# Builds the RBAC contract and factory for NEAR and copies the artifacts to res/.
#
# If wasm-opt is installed it is applied with -Oz; otherwise the Rust release profile
# (opt-level = "z", LTO, symbol stripping) is used as-is.
set -euo pipefail

cd "$(dirname "$0")/.."

cargo build -p rbac-nft -p rbac-factory --target wasm32-unknown-unknown --release

mkdir -p res
cp target/wasm32-unknown-unknown/release/rbac_nft.wasm res/rbac_nft.wasm
cp target/wasm32-unknown-unknown/release/rbac_factory.wasm res/rbac_factory.wasm

if command -v wasm-opt >/dev/null 2>&1; then
  wasm-opt -Oz --strip-debug --strip-producers \
    res/rbac_nft.wasm -o res/rbac_nft.wasm
  wasm-opt -Oz --strip-debug --strip-producers \
    res/rbac_factory.wasm -o res/rbac_factory.wasm
  echo "Optimized artifacts with wasm-opt -Oz"
else
  echo "wasm-opt not found; using the Rust release profile only"
fi

echo "Built res/rbac_nft.wasm and res/rbac_factory.wasm"
