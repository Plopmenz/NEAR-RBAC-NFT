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
  const siteUrl = (env.PUBLIC_SITE_URL?.trim() || "").replace(/\/+$/, "");

  return {
    network,
    rpcUrl: env.PUBLIC_NEAR_RPC_URL?.trim() || DEFAULT_RPC[network],
    factoryContractId: env.PUBLIC_FACTORY_CONTRACT_ID?.trim() || "",
    expectedChildCodeHash: env.PUBLIC_EXPECTED_CHILD_CODE_HASH?.trim().toLowerCase() || "",
    explorerUrl: (env.PUBLIC_EXPLORER_URL?.trim() || DEFAULT_EXPLORER[network]).replace(
      /\/+$/,
      "",
    ),
    siteUrl,
    salaryTokenId: env.PUBLIC_SALARY_TOKEN_ID?.trim() || "",
    salaryTokenDecimals: parseDecimals(env.PUBLIC_SALARY_TOKEN_DECIMALS, 6),
    salaryTokenSymbol: env.PUBLIC_SALARY_TOKEN_SYMBOL?.trim() || "MOCK",
    demoFundAmount: env.PUBLIC_DEMO_FUND_AMOUNT?.trim() || "100",
    oidcIssuer: (env.PUBLIC_OIDC_ISSUER?.trim() || "").replace(/\/+$/, ""),
    oidcClientId: env.PUBLIC_OIDC_CLIENT_ID?.trim() || "",
    oidcScopes: env.PUBLIC_OIDC_SCOPES?.trim() || "openid profile roles",
    oidcRedirectUri:
      env.PUBLIC_OIDC_REDIRECT_URI?.trim() || (siteUrl ? `${siteUrl}/oidc/callback` : ""),
  };
}

function parseDecimals(value: string | undefined, fallback: number): number {
  const parsed = Number(value);
  return Number.isInteger(parsed) && parsed >= 0 && parsed <= 36 ? parsed : fallback;
}

/** 30 days in nanoseconds: the default deploy payout interval. */
export const DEFAULT_PAYOUT_INTERVAL_NS = 2_592_000_000_000_000n;

export const WRITE_GAS = 100_000_000_000_000n;
export const CLAIM_GAS = 100_000_000_000_000n;
export const DEPLOY_GAS = 300_000_000_000_000n;
