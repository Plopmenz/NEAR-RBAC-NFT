import {
  actionCreators,
  getActiveAccount,
  setupWalletSelector,
  type FinalExecutionOutcome,
  type Network,
  type WalletSelector,
} from "@near-wallet-selector/core";
import { setupModal, type WalletSelectorModal } from "@near-wallet-selector/modal-ui";
import { setupMyNearWallet } from "@near-wallet-selector/my-near-wallet";

import type { AppConfig } from "./types";

export interface WalletHandle {
  selector: WalletSelector;
  modal: WalletSelectorModal;
}

export async function initWallet(config: AppConfig): Promise<WalletHandle> {
  const network: Network = {
    networkId: config.network,
    nodeUrl: config.rpcUrl,
    helperUrl: `https://helper.${config.network}.near.org`,
    explorerUrl: config.explorerUrl,
    indexerUrl: "https://api.kitwallet.app",
  };

  const selector = await setupWalletSelector({
    network,
    modules: [setupMyNearWallet()],
  });
  // Wallet modules require a contract for the sign-in state. Use the factory as the stable
  // anchor; calls to individual payrole-nft children still work and fall back to a wallet
  // confirmation when they are outside the factory's function-call key scope.
  const modal = setupModal(selector, {
    contractId: config.factoryContractId || undefined,
    theme: "dark",
    description: "Sign in to deploy and manage PayRole contracts.",
  });

  return { selector, modal };
}

export function getWalletAccountId(selector: WalletSelector): string | null {
  return getActiveAccount(selector.store.getState())?.accountId ?? null;
}

export interface FunctionCallParams {
  contractId: string;
  methodName: string;
  args?: Record<string, unknown>;
  deposit?: bigint;
  gas?: bigint;
}

export async function signAndSendFunctionCall(
  selector: WalletSelector,
  signerId: string,
  params: FunctionCallParams,
): Promise<FinalExecutionOutcome | void> {
  const wallet = await selector.wallet();
  return wallet.signAndSendTransaction({
    signerId,
    receiverId: params.contractId,
    actions: [
      actionCreators.functionCall(
        params.methodName,
        params.args ?? {},
        params.gas ?? 100_000_000_000_000n,
        params.deposit ?? 0n,
      ),
    ],
  });
}

export function transactionHash(outcome: FinalExecutionOutcome | void): string | null {
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
