import { viewFunction } from "./rpc";
import type { FactoryContractRecord, FactoryInfo } from "./types";

const PAGE_SIZE = 100;

export async function loadFactoryInfo(
  rpcUrl: string,
  factoryContractId: string,
): Promise<FactoryInfo> {
  const [codeLen, codeHash, contractCount, requiredDeposit] = await Promise.all([
    viewFunction<string>(rpcUrl, factoryContractId, "code_len"),
    viewFunction<string>(rpcUrl, factoryContractId, "code_hash"),
    viewFunction<number | string>(rpcUrl, factoryContractId, "contract_count"),
    viewFunction<string>(rpcUrl, factoryContractId, "required_deploy_deposit"),
  ]);

  return {
    codeLen: Number(codeLen),
    codeHash: String(codeHash),
    contractCount: Number(contractCount),
    requiredDeposit: String(requiredDeposit),
  };
}

export async function loadFactoryPage(
  rpcUrl: string,
  factoryContractId: string,
  fromIndex: number,
  limit = PAGE_SIZE,
): Promise<FactoryContractRecord[]> {
  return viewFunction<FactoryContractRecord[]>(rpcUrl, factoryContractId, "get_contracts", {
    from_index: fromIndex.toString(),
    limit,
  });
}

/** Reads the full factory registry. The registry is append-only, so pages are stable. */
export async function listFactoryContracts(
  rpcUrl: string,
  factoryContractId: string,
): Promise<FactoryContractRecord[]> {
  const count = Number(
    await viewFunction<number | string>(rpcUrl, factoryContractId, "contract_count"),
  );
  if (count <= 0) return [];

  const records: FactoryContractRecord[] = [];
  for (let from = 0; from < count; from += PAGE_SIZE) {
    const page = await loadFactoryPage(rpcUrl, factoryContractId, from, PAGE_SIZE);
    if (page.length === 0) break;
    records.push(...page);
  }
  return records;
}

export function factoryExplorerUrl(explorerUrl: string, accountId: string): string {
  return `${explorerUrl}/address/${encodeURIComponent(accountId)}`;
}

export function transactionExplorerUrl(explorerUrl: string, txHash: string): string {
  return `${explorerUrl}/txns/${encodeURIComponent(txHash)}`;
}
