#!/usr/bin/env bash
# Builds the RBAC contract for NEAR and copies the artifact to res/.
set -euo pipefail

cd "$(dirname "$0")/.."

cargo build -p rbac-nft --target wasm32-unknown-unknown --release

mkdir -p res
cp target/wasm32-unknown-unknown/release/rbac_nft.wasm res/rbac_nft.wasm

echo "Built res/rbac_nft.wasm"
