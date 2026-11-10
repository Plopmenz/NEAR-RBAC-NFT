export type NetworkId = "testnet" | "mainnet";

export interface AppConfig {
  network: NetworkId;
  rpcUrl: string;
  factoryContractId: string;
  expectedChildCodeHash: string;
  explorerUrl: string;
}

export interface FactoryContractRecord {
  account_id: string;
  name: string;
  owner_id: string;
  salary_token: string;
  payout_interval_ns: string;
  deployer_id: string;
  deployed_at_ns: string;
}

export interface FtMetadata {
  decimals: number;
  symbol: string;
  name: string;
  fallback: boolean;
}

export interface RoleView {
  role_id: string;
  name: string;
  owner: string;
  configured_owner: string | null;
  salary: string;
  holders: number;
}

export interface ContractSnapshot {
  accountId: string;
  ownerId: string;
  salaryToken: string;
  payoutIntervalNs: string;
  roles: RoleView[];
  token: FtMetadata;
}

export interface RoleMembership {
  role: RoleView;
  claimable: string;
  lastClaim: string | null;
}

export interface OwnedContract {
  record: FactoryContractRecord;
  snapshot: ContractSnapshot;
  favorite: boolean;
}

export interface MembershipContract {
  record: FactoryContractRecord | null;
  snapshot: ContractSnapshot;
  memberships: RoleMembership[];
}

export interface FactoryInfo {
  codeLen: number;
  codeHash: string;
  contractCount: number;
  requiredDeposit: string;
}
