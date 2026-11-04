/**
 * Role ids must match `rbac-common`:
 *
 *   role_id = u32::from_be_bytes(SHA-256(role_name)[0..4])
 */
export async function roleId(roleName: string): Promise<number> {
  const data = new TextEncoder().encode(roleName);
  const digest = await globalThis.crypto.subtle.digest("SHA-256", data);
  return new DataView(digest).getUint32(0, false);
}

/** Builds the NEP-245 token id used by the contract. */
export async function tokenId(roleName: string, accountId: string): Promise<string> {
  return `${await roleId(roleName)}:${accountId}`;
}
