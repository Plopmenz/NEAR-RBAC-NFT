import type { NearConnector, NearWalletBase } from "@hot-labs/near-connect";

import type { AccessMap } from "../../lib/access";
import { loadConfig } from "../../lib/config";
import type {
  AppConfig,
  FactoryContractRecord,
  FactoryInfo,
  MembershipContract,
  OidcSession,
  OwnedContract,
} from "../../lib/types";

export type Tab = "deploy" | "contracts" | "roles" | "access";
export type Tone = "muted" | "ok" | "error";

export interface State {
  config: AppConfig;
  connector: NearConnector | null;
  wallet: NearWalletBase | null;
  walletAccount: string | null;
  tab: Tab;
  factoryInfo: FactoryInfo | null;
  factoryError: string;
  ownedAddress: string;
  owned: OwnedContract[];
  ownedLoading: boolean;
  ownedScanned: boolean;
  rolesAddress: string;
  memberships: MembershipContract[];
  rolesLoading: boolean;
  rolesScanned: boolean;
  favorites: Set<string>;
  favoritesOnly: boolean;
  blockTimestampNs: bigint;
  accessMap: AccessMap;
  oidcSession: OidcSession | null;
  oidcError: string;
  accessMemberships: MembershipContract[];
  accessLoading: boolean;
  accessScanned: boolean;
}

const config = loadConfig();

export const state: State = {
  config,
  connector: null,
  wallet: null,
  walletAccount: null,
  tab: "deploy",
  factoryInfo: null,
  factoryError: "",
  ownedAddress: "",
  owned: [],
  ownedLoading: false,
  ownedScanned: false,
  rolesAddress: "",
  memberships: [],
  rolesLoading: false,
  rolesScanned: false,
  favorites: new Set(),
  favoritesOnly: false,
  blockTimestampNs: BigInt(Date.now()) * 1_000_000n,
  accessMap: {},
  oidcSession: null,
  oidcError: "",
  accessMemberships: [],
  accessLoading: false,
  accessScanned: false,
};

/**
 * Mutable values that support the state object but are not rendered directly. Keeping them in a
 * single record makes the cross-module ownership explicit.
 */
export const runtime: {
  registryCache: { fetchedAt: number; records: FactoryContractRecord[] } | null;
  ownedRun: number;
  rolesRun: number;
  accessRun: number;
  noticeTimer: number | undefined;
  lastAutoDeposit: string;
} = {
  registryCache: null,
  ownedRun: 0,
  rolesRun: 0,
  accessRun: 0,
  noticeTimer: undefined,
  lastAutoDeposit: "",
};

export const INTERVAL_UNITS: Record<string, bigint> = {
  seconds: 1_000_000_000n,
  minutes: 60_000_000_000n,
  hours: 3_600_000_000_000n,
  days: 86_400_000_000_000n,
  weeks: 604_800_000_000_000n,
};
