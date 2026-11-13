import { NearConnector, type NearWalletBase } from "@hot-labs/near-connect";

import type { AppConfig } from "./types";

export type TransactionOutcome = Awaited<
  ReturnType<NearWalletBase["signAndSendTransaction"]>
>;

export interface WalletSession {
  wallet: NearWalletBase;
  accountId: string;
}

/**
 * Builds the NEAR Connect connector. NEAR Connect replaces NEAR Wallet Selector, which is
 * deprecated. Wallets are loaded from a shared manifest at runtime and run sandboxed.
 */
export function initWallet(config: AppConfig): NearConnector {
  return new NearConnector({
    network: config.network,
    // Only offer wallets that can sign and send transactions on the configured network.
    features: {
      signAndSendTransaction: true,
      mainnet: config.network === "mainnet",
      testnet: config.network === "testnet",
    },
    footerBranding: null,
  });
}

/** Restores a previous NEAR Connect session, if the wallet still reports an account. */
export async function restoreWallet(connector: NearConnector): Promise<WalletSession | null> {
  try {
    const { wallet, accounts } = await connector.getConnectedWallet();
    const accountId = accounts[0]?.accountId;
    return accountId ? { wallet, accountId } : null;
  } catch {
    return null;
  }
}

export interface FunctionCallParams {
  contractId: string;
  methodName: string;
  args?: Record<string, unknown>;
  deposit?: bigint;
  gas?: bigint;
}

export async function signAndSendFunctionCall(
  wallet: NearWalletBase,
  signerId: string,
  params: FunctionCallParams,
): Promise<TransactionOutcome> {
  return wallet.signAndSendTransaction({
    signerId,
    receiverId: params.contractId,
    actions: [
      {
        type: "FunctionCall",
        params: {
          methodName: params.methodName,
          args: params.args ?? {},
          gas: (params.gas ?? 100_000_000_000_000n).toString(),
          deposit: (params.deposit ?? 0n).toString(),
        },
      },
    ],
  });
}

export function transactionHash(outcome: TransactionOutcome | void): string | null {
  if (!outcome) return null;
  return outcome.transaction_outcome?.id ?? outcome.transaction?.hash ?? null;
}

export function walletErrorMessage(error: unknown): string {
  if (error instanceof Error) {
    return error.message || "Wallet request failed.";
  }
  if (typeof error === "string") return error;
  return "Wallet request failed.";
}
