import { mapLimit } from "./async";
import { viewFunction } from "./rpc";
import type {
  ContractSnapshot,
  FactoryContractRecord,
  FtMetadata,
  MembershipContract,
  OwnedContract,
  RoleMembership,
  RoleView,
} from "./types";

const PAGE_SIZE = 100;
const READ_CONCURRENCY = 6;

export async function loadFtMetadata(
  rpcUrl: string,
  tokenAccountId: string,
): Promise<FtMetadata> {
  try {
    const metadata = await viewFunction<{
      decimals?: number;
      symbol?: string;
      name?: string;
    }>(rpcUrl, tokenAccountId, "ft_metadata");
    const decimals = Number(metadata.decimals);
    if (!Number.isInteger(decimals) || decimals < 0 || decimals > 36) {
      throw new Error("invalid decimals");
    }
    return {
      decimals,
      symbol: metadata.symbol || tokenAccountId,
      name: metadata.name || tokenAccountId,
      fallback: false,
    };
  } catch {
    return {
      decimals: 0,
      symbol: tokenAccountId,
      name: tokenAccountId,
      fallback: true,
    };
  }
}

async function loadAllRoles(rpcUrl: string, accountId: string): Promise<RoleView[]> {
  const count = Number(await viewFunction<number | string>(rpcUrl, accountId, "role_count"));
  if (count <= 0) return [];

  const roles: RoleView[] = [];
  for (let from = 0; from < count; from += PAGE_SIZE) {
    const page = await viewFunction<RoleView[]>(rpcUrl, accountId, "get_roles", {
      from_index: from.toString(),
      limit: PAGE_SIZE,
    });
    roles.push(...page);
    if (page.length === 0) break;
  }
  return roles;
}

export async function loadContractSnapshot(
  rpcUrl: string,
  accountId: string,
): Promise<ContractSnapshot> {
  const [ownerId, salaryToken, payoutIntervalNs, roles] = await Promise.all([
    viewFunction<string>(rpcUrl, accountId, "owner_id"),
    viewFunction<string>(rpcUrl, accountId, "salary_token"),
    viewFunction<string>(rpcUrl, accountId, "payout_interval_ns"),
    loadAllRoles(rpcUrl, accountId),
  ]);
  const token = await loadFtMetadata(rpcUrl, salaryToken);
  return { accountId, ownerId, salaryToken, payoutIntervalNs, roles, token };
}

/**
 * Verifies the current owner of every registry entry, then loads full snapshots only for the
 * contracts owned by `accountId`.
 */
export async function loadOwnedContracts(
  rpcUrl: string,
  records: FactoryContractRecord[],
  accountId: string,
): Promise<OwnedContract[]> {
  const ownership = await mapLimit(records, READ_CONCURRENCY, async (record) => {
    try {
      const ownerId = await viewFunction<string>(rpcUrl, record.account_id, "owner_id");
      return { record, ownerId };
    } catch {
      return { record, ownerId: null };
    }
  });

  const owned = ownership.filter((entry) => entry.ownerId === accountId);
  return mapLimit(owned, READ_CONCURRENCY, async ({ record }) => {
    const snapshot = await loadContractSnapshot(rpcUrl, record.account_id);
    return { record, snapshot, favorite: false };
  });
}

/** Loads every role membership held by `accountId`, plus the contract snapshot for admin UI. */
export async function loadMembershipContracts(
  rpcUrl: string,
  records: FactoryContractRecord[],
  accountId: string,
): Promise<MembershipContract[]> {
  const held = await mapLimit(records, READ_CONCURRENCY, async (record) => {
    try {
      const roles = await viewFunction<string[]>(rpcUrl, record.account_id, "roles_of", {
        account_id: accountId,
      });
      return { record, roles };
    } catch {
      return { record, roles: [] as string[] };
    }
  });

  const withRoles = held.filter((entry) => entry.roles.length > 0);
  return mapLimit(withRoles, READ_CONCURRENCY, async ({ record, roles }) => {
    const snapshot = await loadContractSnapshot(rpcUrl, record.account_id);
    const memberships = await loadRoleMemberships(rpcUrl, snapshot, roles, accountId);
    return { record, snapshot, memberships };
  });
}

async function loadRoleMemberships(
  rpcUrl: string,
  snapshot: ContractSnapshot,
  roleNames: string[],
  accountId: string,
): Promise<RoleMembership[]> {
  const roles = roleNames
    .map((name) => snapshot.roles.find((role) => role.name === name))
    .filter((role): role is RoleView => Boolean(role));

  return mapLimit(roles, READ_CONCURRENCY, async (role) => {
    const [claimable, lastClaim] = await Promise.all([
      viewFunction<string>(rpcUrl, snapshot.accountId, "claimable_role", {
        account_id: accountId,
        role: role.name,
      }),
      viewFunction<string | null>(rpcUrl, snapshot.accountId, "last_claim", {
        account_id: accountId,
        role: role.name,
      }),
    ]);
    return { role, claimable: String(claimable), lastClaim };
  });
}

export function isRoleOwner(role: RoleView, walletAccountId: string | null): boolean {
  return Boolean(walletAccountId && role.owner === walletAccountId);
}

export function canManageRole(
  snapshot: ContractSnapshot,
  role: RoleView,
  walletAccountId: string | null,
): boolean {
  if (!walletAccountId) return false;
  return snapshot.ownerId === walletAccountId || role.owner === walletAccountId;
}

export function canManageContract(
  snapshot: ContractSnapshot,
  walletAccountId: string | null,
): boolean {
  return Boolean(walletAccountId && snapshot.ownerId === walletAccountId);
}

export function manageableRoles(
  snapshot: ContractSnapshot,
  walletAccountId: string | null,
): RoleView[] {
  return snapshot.roles.filter((role) => canManageRole(snapshot, role, walletAccountId));
}

export function findRole(snapshot: ContractSnapshot, name: string): RoleView | undefined {
  return snapshot.roles.find((role) => role.name === name);
}
