export type NetworkId = "testnet" | "mainnet";

export interface AppConfig {
  network: NetworkId;
  rpcUrl: string;
  factoryContractId: string;
  expectedChildCodeHash: string;
  explorerUrl: string;
  /** Absolute site URL used for canonical and social preview tags. Empty when unset. */
  siteUrl: string;
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
  /** Deposit required by `mint_role`, in yoctoNEAR, as reported by the contract. */
  requiredMintDeposit: string;
  /** Contract's balance of the salary token in raw units, or null when the read failed. */
  ftBalance: string | null;
  /** Salary accrued but not yet claimed in raw token units, or null when the read failed. */
  accruedLiabilities: string | null;
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
