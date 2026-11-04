import type { AppConfig, NetworkId } from "./types";

const DEFAULT_RPC: Record<NetworkId, string> = {
  testnet: "https://rpc.testnet.near.org",
  mainnet: "https://rpc.mainnet.near.org",
};

const DEFAULT_EXPLORER: Record<NetworkId, string> = {
  testnet: "https://testnet.nearblocks.io",
  mainnet: "https://nearblocks.io",
};

/**
 * Reads public build-time configuration. Vite replaces `import.meta.env.PUBLIC_*` at build time.
 *
 * The factory is intentionally not defaulted: a missing factory disables discovery and
 * deployment and shows a configuration banner instead of silently querying the wrong contract.
 */
export function loadConfig(): AppConfig {
  const env = import.meta.env;
  const rawNetwork = (env.PUBLIC_NEAR_NETWORK ?? "testnet").toLowerCase();
  const network: NetworkId = rawNetwork === "mainnet" ? "mainnet" : "testnet";

  return {
    network,
    rpcUrl: env.PUBLIC_NEAR_RPC_URL?.trim() || DEFAULT_RPC[network],
    factoryContractId: env.PUBLIC_FACTORY_CONTRACT_ID?.trim() || "",
    explorerUrl: (env.PUBLIC_EXPLORER_URL?.trim() || DEFAULT_EXPLORER[network]).replace(
      /\/+$/,
      "",
    ),
  };
}

export const WRITE_GAS = 100_000_000_000_000n;
export const CLAIM_GAS = 100_000_000_000_000n;
export const DEPLOY_GAS = 300_000_000_000_000n;
