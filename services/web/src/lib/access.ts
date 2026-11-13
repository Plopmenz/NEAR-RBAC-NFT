/**
 * Local OIDC access-label mapping.
 *
 * The mapping is intentionally browser-local: it is a presentation layer on top of on-chain role
 * NFTs, not another contract setting. A role such as `admin` can grant labels such as
 * `admin-panel` or `billing:read`, and the access page shows those labels with a check or cross
 * for the signed-in account.
 */

export type RoleAccessMap = Record<string, string[]>;
export type AccessMap = Record<string, RoleAccessMap>;

const STORAGE_PREFIX = "payrole-nft:access:v1";

export interface AccessItem {
  label: string;
  granted: boolean;
  /** Roles that grant this label. Empty for the role-name fallback. */
  roles: string[];
}

export function accessStorageKey(network: string): string {
  return `${STORAGE_PREFIX}:${network}`;
}

export function normalizeLabels(input: string): string[] {
  const labels = input
    .split(",")
    .map((label) => label.trim())
    .filter((label) => label.length > 0);
  return [...new Set(labels)];
}

export function parseAccessMap(raw: string | null): AccessMap {
  if (!raw) return {};
  let parsed: unknown;
  try {
    parsed = JSON.parse(raw);
  } catch {
    return {};
  }
  if (!parsed || typeof parsed !== "object" || Array.isArray(parsed)) return {};

  const result: AccessMap = {};
  for (const [contract, roles] of Object.entries(parsed as Record<string, unknown>)) {
    if (!roles || typeof roles !== "object" || Array.isArray(roles)) continue;
    const roleMap: RoleAccessMap = {};
    for (const [role, labels] of Object.entries(roles as Record<string, unknown>)) {
      if (!Array.isArray(labels)) continue;
      const cleaned = labels.filter(
        (label): label is string => typeof label === "string" && label.trim().length > 0,
      );
      if (cleaned.length > 0) roleMap[role] = [...new Set(cleaned.map((label) => label.trim()))];
    }
    if (Object.keys(roleMap).length > 0) result[contract] = roleMap;
  }
  return result;
}

export function loadAccessMap(storage: Storage, network: string): AccessMap {
  return parseAccessMap(storage.getItem(accessStorageKey(network)));
}

export function saveAccessMap(storage: Storage, network: string, map: AccessMap): void {
  storage.setItem(accessStorageKey(network), JSON.stringify(map));
}

export function labelsForRole(
  map: AccessMap,
  contractId: string,
  role: string,
): string[] {
  return map[contractId]?.[role] ?? [];
}

export function setRoleLabels(
  storage: Storage,
  network: string,
  contractId: string,
  role: string,
  labels: string[],
): AccessMap {
  const map = loadAccessMap(storage, network);
  const contract = { ...(map[contractId] ?? {}) };
  const cleaned = [...new Set(labels.map((label) => label.trim()).filter(Boolean))];
  if (cleaned.length === 0) delete contract[role];
  else contract[role] = cleaned;

  if (Object.keys(contract).length === 0) delete map[contractId];
  else map[contractId] = contract;

  saveAccessMap(storage, network, map);
  return map;
}

/**
 * Builds the access list for one contract.
 *
 * When no labels are configured, role names are used as the sample labels so the page still
 * demonstrates checks and crosses. Configured labels are aggregated across roles.
 */
export function accessItems(
  map: AccessMap,
  contractId: string,
  allRoles: string[],
  heldRoles: string[],
): AccessItem[] {
  const mapping = map[contractId] ?? {};
  const held = new Set(heldRoles);
  const configured = [
    ...new Set(allRoles.flatMap((role) => mapping[role] ?? [])),
  ].sort((left, right) => left.localeCompare(right));

  if (configured.length === 0) {
    return allRoles
      .map((role) => ({ label: role, granted: held.has(role), roles: [] }))
      .sort((left, right) => left.label.localeCompare(right.label));
  }

  return configured.map((label) => {
    const roles = allRoles.filter((role) => (mapping[role] ?? []).includes(label));
    return { label, granted: roles.some((role) => held.has(role)), roles };
  });
}
