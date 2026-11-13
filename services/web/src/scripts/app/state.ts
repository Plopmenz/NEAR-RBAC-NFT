import type { WalletSelector } from "@near-wallet-selector/core";
import type { WalletSelectorModal } from "@near-wallet-selector/modal-ui";

import { loadConfig } from "../../lib/config";
import type {
  AppConfig,
  FactoryContractRecord,
  FactoryInfo,
  MembershipContract,
  OwnedContract,
} from "../../lib/types";

export type Tab = "deploy" | "contracts" | "roles";
export type Tone = "muted" | "ok" | "error";

export interface State {
  config: AppConfig;
  selector: WalletSelector | null;
  modal: WalletSelectorModal | null;
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
}

export const state: State = {
  config: loadConfig(),
  selector: null,
  modal: null,
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
};

/**
 * Mutable values that support the state object but are not rendered directly. Keeping them in a
 * single record makes the cross-module ownership explicit.
 */
export const runtime: {
  registryCache: { fetchedAt: number; records: FactoryContractRecord[] } | null;
  ownedRun: number;
  rolesRun: number;
  noticeTimer: number | undefined;
  lastAutoDeposit: string;
} = {
  registryCache: null,
  ownedRun: 0,
  rolesRun: 0,
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
