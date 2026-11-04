#!/usr/bin/env bash
# Builds the RBAC contract and factory for NEAR and copies the artifacts to res/.
set -euo pipefail

cd "$(dirname "$0")/.."

cargo build -p rbac-nft -p rbac-factory --target wasm32-unknown-unknown --release

mkdir -p res
cp target/wasm32-unknown-unknown/release/rbac_nft.wasm res/rbac_nft.wasm
cp target/wasm32-unknown-unknown/release/rbac_factory.wasm res/rbac_factory.wasm

echo "Built res/rbac_nft.wasm and res/rbac_factory.wasm"
