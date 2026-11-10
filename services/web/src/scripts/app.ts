import type { WalletSelector } from "@near-wallet-selector/core";
import type { WalletSelectorModal } from "@near-wallet-selector/modal-ui";

import { childAccountId, isValidAccountId, subaccountNameError } from "../lib/accounts";
import { CLAIM_GAS, DEPLOY_GAS, WRITE_GAS, loadConfig } from "../lib/config";
import {
  factoryExplorerUrl,
  listFactoryContracts,
  loadFactoryInfo,
  transactionExplorerUrl,
} from "../lib/factory";
import { loadFavorites, saveFavorites, toggleFavorite } from "../lib/favorites";
import {
  formatDurationNs,
  formatNear,
  formatPercent,
  formatTimestampNs,
  formatToken,
  formatUnits,
  intervalProgress,
  parseNear,
  parseUnits,
  shortenAccount,
} from "../lib/format";
import {
  canManageContract,
  canManageRole,
  loadMembershipContracts,
  loadOwnedContracts,
  manageableRoles,
} from "../lib/rbac";
import { getFinalBlockTimestampNs } from "../lib/rpc";
import type {
  AppConfig,
  ContractSnapshot,
  FactoryContractRecord,
  FactoryInfo,
  MembershipContract,
  OwnedContract,
  RoleView,
} from "../lib/types";
import {
  getWalletAccountId,
  initWallet,
  signAndSendFunctionCall,
  transactionHash,
  walletErrorMessage,
} from "../lib/wallet";

type Tab = "deploy" | "contracts" | "roles";
type Tone = "muted" | "ok" | "error";

interface State {
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

const state: State = {
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

let registryCache: { fetchedAt: number; records: FactoryContractRecord[] } | null = null;
let ownedRun = 0;
let rolesRun = 0;
let noticeTimer: number | undefined;
let lastAutoDeposit = "";

const INTERVAL_UNITS: Record<string, bigint> = {
  seconds: 1_000_000_000n,
  minutes: 60_000_000_000n,
  hours: 3_600_000_000_000n,
  days: 86_400_000_000_000n,
  weeks: 604_800_000_000_000n,
};

function byId<T extends HTMLElement>(id: string): T {
  const element = document.getElementById(id);
  if (!element) throw new Error(`Missing element #${id}`);
  return element as T;
}

function str(value: FormDataEntryValue | null | undefined): string {
  return typeof value === "string" ? value : "";
}

function escapeHtml(value: unknown): string {
  return String(value ?? "")
    .replaceAll("&", "&amp;")
    .replaceAll("<", "&lt;")
    .replaceAll(">", "&gt;")
    .replaceAll('"', "&quot;")
    .replaceAll("'", "&#39;");
}

function setStatus(element: HTMLElement, message: string, tone: Tone = "muted"): void {
  element.className = tone === "muted" ? "status" : `status ${tone}`;
  element.textContent = message;
}

function setStatusLink(element: HTMLElement, message: string, url: string): void {
  element.className = "status ok";
  element.innerHTML = `${escapeHtml(message)} <a href="${escapeHtml(url)}" target="_blank" rel="noreferrer">View transaction ↗</a>`;
}

function showNotice(message: string, tone: "ok" | "error" = "ok"): void {
  const notice = byId<HTMLElement>("global-notice");
  notice.hidden = false;
  notice.className = `notice ${tone}`;
  notice.textContent = message;
  if (noticeTimer) window.clearTimeout(noticeTimer);
  if (tone === "ok") {
    noticeTimer = window.setTimeout(() => {
      notice.hidden = true;
    }, 7_000);
  }
}

function accountLink(accountId: string): string {
  return `<a href="${escapeHtml(
    factoryExplorerUrl(state.config.explorerUrl, accountId),
  )}" target="_blank" rel="noreferrer">${escapeHtml(accountId)}</a>`;
}

function tokenLabel(token: { symbol: string; fallback: boolean }): string {
  const suffix = token.fallback
    ? ' <span class="muted">(metadata unavailable; raw units)</span>'
    : "";
  return `${escapeHtml(token.symbol)}${suffix}`;
}

function roleOwnerLabel(snapshot: ContractSnapshot, role: RoleView): string {
  if (role.configured_owner) {
    return `<span class="tag owner" title="${escapeHtml(role.owner)}">${escapeHtml(
      shortenAccount(role.owner),
    )}</span>`;
  }
  return `<span class="muted" title="${escapeHtml(snapshot.ownerId)}">contract owner</span>`;
}

function renderMeta(snapshot: ContractSnapshot): string {
  return `<div class="meta-grid">
    <div><span class="meta-label">Owner</span><span class="meta-value">${accountLink(
      snapshot.ownerId,
    )}</span></div>
    <div><span class="meta-label">Salary token</span><span class="meta-value">${accountLink(
      snapshot.salaryToken,
    )}<br />${tokenLabel(snapshot.token)}</span></div>
    <div><span class="meta-label">Payout interval</span><span class="meta-value">${escapeHtml(
      formatDurationNs(snapshot.payoutIntervalNs),
    )}</span></div>
    <div><span class="meta-label">Roles</span><span class="meta-value">${
      snapshot.roles.length
    }</span></div>
  </div>`;
}

function preferredInterval(ns: bigint | string): { value: string; unit: string } {
  const value = BigInt(ns);
  for (const unit of ["weeks", "days", "hours", "minutes", "seconds"]) {
    const factor = INTERVAL_UNITS[unit];
    if (value > 0n && value % factor === 0n) {
      return { value: (value / factor).toString(), unit };
    }
  }
  return { value: (Number(value) / 1e9).toString(), unit: "seconds" };
}

function intervalInputs(value: bigint | string): string {
  const preferred = preferredInterval(value);
  const units = ["seconds", "minutes", "hours", "days", "weeks"];
  return `<div class="inline-fields">
    <input name="intervalValue" type="number" min="1" step="1" value="${escapeHtml(
      preferred.value,
    )}" required />
    <select name="intervalUnit">
      ${units
        .map(
          (unit) =>
            `<option value="${unit}"${unit === preferred.unit ? " selected" : ""}>${unit}</option>`,
        )
        .join("")}
    </select>
  </div>`;
}

function parseInterval(value: string, unit: string): bigint {
  const raw = value.trim();
  if (!/^\d+$/.test(raw)) throw new Error("The payout interval must be a whole number.");
  const amount = BigInt(raw);
  if (amount <= 0n) throw new Error("The payout interval must be positive.");
  const factor = INTERVAL_UNITS[unit];
  if (!factor) throw new Error("Choose a valid interval unit.");
  return amount * factor;
}

function roleActions(
  snapshot: ContractSnapshot,
  role: RoleView,
): string {
  if (!canManageRole(snapshot, role, state.walletAccount)) return '<span class="muted">—</span>';
  return `<div class="role-actions">
    <form class="inline-form" data-form="mint" data-contract="${escapeHtml(
      snapshot.accountId,
    )}" data-role="${escapeHtml(role.name)}">
      <input name="account" placeholder="account to mint" aria-label="Account to mint ${escapeHtml(
        role.name,
      )}" required />
      <button class="btn small" type="submit">Mint</button>
    </form>
    <form class="inline-form" data-form="burn" data-contract="${escapeHtml(
      snapshot.accountId,
    )}" data-role="${escapeHtml(role.name)}">
      <input name="account" placeholder="account to burn" aria-label="Account to burn ${escapeHtml(
        role.name,
      )}" required />
      <button class="btn small danger" type="submit">Burn</button>
    </form>
  </div>`;
}

function roleTable(snapshot: ContractSnapshot, roles: RoleView[]): string {
  if (roles.length === 0) {
    return '<p class="muted">No roles configured yet. The contract owner can create one below.</p>';
  }
  return `<div class="table-wrap"><table>
    <thead><tr><th>Role</th><th>Salary</th><th>Holders</th><th>Owner</th><th>Membership actions</th></tr></thead>
    <tbody>
      ${roles
        .map(
          (role) => `<tr>
            <td><code>${escapeHtml(role.name)}</code><span class="muted">id ${role.role_id}</span></td>
            <td>${escapeHtml(
              formatToken(role.salary, snapshot.token, 5),
            )}<span class="muted">per ${escapeHtml(
              formatDurationNs(snapshot.payoutIntervalNs),
            )}</span></td>
            <td>${role.holders}</td>
            <td>${roleOwnerLabel(snapshot, role)}</td>
            <td>${roleActions(snapshot, role)}</td>
          </tr>`,
        )
        .join("")}
    </tbody>
  </table></div>`;
}

function ownerAdminForms(snapshot: ContractSnapshot): string {
  const roleNames = snapshot.roles.map((role) => role.name).join(", ") || "none yet";
  return `<div class="admin-grid">
    <form class="admin-form" data-form="role-salary" data-contract="${escapeHtml(
      snapshot.accountId,
    )}">
      <h4>Create / update role</h4>
      <label class="field"><span>Role name</span><input name="role" placeholder="admin" required /></label>
      <label class="field"><span>Salary per interval (${escapeHtml(
        snapshot.token.symbol,
      )})</span><input name="amount" inputmode="decimal" placeholder="0.0" required /></label>
      <button class="btn small" type="submit">Save role</button>
      <small class="muted">Existing roles: ${escapeHtml(roleNames)}. A salary of 0 is allowed.</small>
    </form>

    <form class="admin-form" data-form="role-owner" data-contract="${escapeHtml(
      snapshot.accountId,
    )}">
      <h4>Delegate a role</h4>
      <label class="field"><span>Role name</span><input name="role" placeholder="admin" required /></label>
      <label class="field"><span>Role owner (blank = contract owner)</span><input name="owner" placeholder="hr.testnet" /></label>
      <button class="btn small" type="submit">Set role owner</button>
    </form>

    <form class="admin-form" data-form="payout" data-contract="${escapeHtml(
      snapshot.accountId,
    )}">
      <h4>Payroll configuration</h4>
      <label class="field"><span>Salary token</span><input name="token" value="${escapeHtml(
        snapshot.salaryToken,
      )}" required /></label>
      <div class="field"><span>Payout interval</span>${intervalInputs(
        snapshot.payoutIntervalNs,
      )}</div>
      <button class="btn small" type="submit">Update payroll</button>
      <small class="muted">Changes apply to unclaimed elapsed time. Claim first if that matters.</small>
    </form>

    <form class="admin-form" data-form="transfer-owner" data-contract="${escapeHtml(
      snapshot.accountId,
    )}">
      <h4>Transfer contract ownership</h4>
      <label class="field"><span>New owner</span><input name="owner" placeholder="new-dao.testnet" required /></label>
      <button class="btn small danger" type="submit">Transfer ownership</button>
      <small class="muted">Normally the owner is a DAO/Trezu account.</small>
    </form>
  </div>`;
}

function ownedCard(item: OwnedContract): string {
  const snapshot = item.snapshot;
  const favorite = state.favorites.has(item.record.account_id);
  const canAdmin = canManageContract(snapshot, state.walletAccount);
  return `<article class="card">
    <div class="card-head">
      <div>
        <h3>${escapeHtml(item.record.name)}</h3>
        <div class="contract-address">${escapeHtml(snapshot.accountId)}</div>
      </div>
      <div class="card-actions">
        <button class="btn icon${favorite ? " active" : ""}" type="button" data-action="favorite" data-contract="${escapeHtml(
          item.record.account_id,
        )}" aria-label="${favorite ? "Remove from favorites" : "Add to favorites"}">${favorite ? "★" : "☆"}</button>
        <a class="btn small ghost" href="${escapeHtml(
          factoryExplorerUrl(state.config.explorerUrl, snapshot.accountId),
        )}" target="_blank" rel="noreferrer">Explorer</a>
      </div>
    </div>
    ${renderMeta(snapshot)}
    ${roleTable(snapshot, snapshot.roles)}
    ${
      canAdmin
        ? `<details class="details"><summary>Contract administration</summary>${ownerAdminForms(
            snapshot,
          )}</details>`
        : ""
    }
  </article>`;
}

function membershipTable(item: MembershipContract): string {
  const snapshot = item.snapshot;
  const accountId = state.rolesAddress;
  const rows = item.memberships
    .map((membership) => {
      const progress = intervalProgress(
        membership.lastClaim,
        snapshot.payoutIntervalNs,
        state.blockTimestampNs,
      );
      const percent = progress.fraction * 100;
      let claimable = 0n;
      try {
        claimable = BigInt(membership.claimable);
      } catch {
        claimable = 0n;
      }
      return `<tr>
        <td><code>${escapeHtml(membership.role.name)}</code><span class="muted">id ${
          membership.role.role_id
        }</span></td>
        <td>${escapeHtml(formatToken(membership.role.salary, snapshot.token, 5))}</td>
        <td class="claim-cell">
          <span class="claim-amount">${escapeHtml(
            formatToken(claimable, snapshot.token, 6),
          )}</span>
          <button class="btn small" type="button" data-action="claim" data-contract="${escapeHtml(
            snapshot.accountId,
          )}" data-account="${escapeHtml(accountId)}" data-role="${escapeHtml(
            membership.role.name,
          )}">Claim</button>
        </td>
        <td>
          <div class="progress-wrap">
            <div class="progress-bar" role="progressbar" aria-valuemin="0" aria-valuemax="100" aria-valuenow="${percent.toFixed(
              1,
            )}"><span class="progress-fill" style="width:${percent.toFixed(1)}%"></span></div>
            <div class="progress-meta"><span>${formatPercent(
              progress.fraction,
            )}</span><span>${escapeHtml(formatDurationNs(progress.remainingNs))} left</span></div>
          </div>
          <span class="muted">Last claim: ${
            membership.lastClaim ? escapeHtml(formatTimestampNs(membership.lastClaim)) : "never"
          }</span>
        </td>
      </tr>`;
    })
    .join("");

  return `<div class="table-wrap"><table>
    <thead><tr><th>Role</th><th>Salary / interval</th><th>Claimable</th><th>Payout progress</th></tr></thead>
    <tbody>${rows}</tbody>
  </table></div>`;
}

function membershipCard(item: MembershipContract): string {
  const snapshot = item.snapshot;
  const managed = manageableRoles(snapshot, state.walletAccount);
  const canAdmin = canManageContract(snapshot, state.walletAccount);
  const accountId = state.rolesAddress;
  return `<article class="card">
    <div class="card-head">
      <div>
        <h3>${escapeHtml(item.record?.name ?? snapshot.accountId)}</h3>
        <div class="contract-address">${escapeHtml(snapshot.accountId)}</div>
      </div>
      <div class="card-actions">
        ${
          item.memberships.length > 1
            ? `<button class="btn small" type="button" data-action="claim-all" data-contract="${escapeHtml(
                snapshot.accountId,
              )}" data-account="${escapeHtml(accountId)}">Claim all</button>`
            : ""
        }
        <a class="btn small ghost" href="${escapeHtml(
          factoryExplorerUrl(state.config.explorerUrl, snapshot.accountId),
        )}" target="_blank" rel="noreferrer">Explorer</a>
      </div>
    </div>
    ${renderMeta(snapshot)}
    ${membershipTable(item)}
    ${
      managed.length > 0
        ? `<details class="details"><summary>Role administration</summary>
            ${roleTable(snapshot, managed)}
            ${canAdmin ? `<div class="subtle-divider"></div>${ownerAdminForms(snapshot)}` : ""}
          </details>`
        : ""
    }
  </article>`;
}

function renderTabs(): void {
  document.querySelectorAll<HTMLButtonElement>("[data-tab]").forEach((button) => {
    const selected = button.dataset.tab === state.tab;
    button.setAttribute("aria-selected", String(selected));
  });
  document.querySelectorAll<HTMLElement>("[data-panel]").forEach((panel) => {
    panel.hidden = panel.dataset.panel !== state.tab;
  });
}

function renderWallet(): void {
  const connected = state.walletAccount;
  const account = byId<HTMLElement>("wallet-account");
  account.textContent = connected ? shortenAccount(connected, 10) : "Not connected";
  account.title = connected ?? "";
  const connect = byId<HTMLButtonElement>("wallet-button");
  connect.textContent = connected ? "Switch wallet" : "Connect wallet";
  connect.disabled = !state.modal;
  byId<HTMLButtonElement>("wallet-disconnect").hidden = !connected;
}

function renderFactory(): void {
  const warning = byId<HTMLElement>("config-warning");
  const factoryId = byId<HTMLElement>("factory-id");
  const submit = byId<HTMLButtonElement>("deploy-submit");
  const note = byId<HTMLElement>("factory-code-note");
  const hint = byId<HTMLElement>("deploy-deposit-hint");
  const config = state.config;

  factoryId.textContent = config.factoryContractId || "not configured";

  if (!config.factoryContractId) {
    warning.hidden = false;
    warning.textContent =
      "No factory contract is configured. Set PUBLIC_FACTORY_CONTRACT_ID in services/web/.env and rebuild.";
    submit.disabled = true;
    note.textContent = "Deployment is disabled until the factory is configured.";
    return;
  }

  if (state.factoryError) {
    warning.hidden = false;
    warning.textContent = `Could not load the factory: ${state.factoryError}`;
    submit.disabled = true;
    note.textContent = "";
    return;
  }

  if (!state.factoryInfo) {
    warning.hidden = true;
    submit.disabled = true;
    note.textContent = "Loading factory status…";
    hint.textContent = "The factory reports the minimum deposit after loading.";
    return;
  }

  const info = state.factoryInfo;
  if (info.codeLen === 0) {
    warning.hidden = false;
    warning.textContent =
      "The factory has no rbac-nft wasm. It was not initialized correctly; deploy a new factory account.";
    submit.disabled = true;
    note.textContent = `Registry: ${info.contractCount} contract(s).`;
    return;
  }

  const expected = config.expectedChildCodeHash;
  if (expected && info.codeHash.toLowerCase() !== expected) {
    warning.hidden = false;
    warning.textContent = `Pinned child wasm hash ${expected} does not match the factory's ${info.codeHash}.`;
    submit.disabled = true;
    note.textContent = `Child wasm: ${(info.codeLen / 1024).toFixed(0)} KiB · registry: ${
      info.contractCount
    } contract(s).`;
    return;
  }

  warning.hidden = true;
  submit.disabled = false;
  const pin = expected ? " (pinned)" : "";
  note.textContent = `Child wasm: ${(info.codeLen / 1024).toFixed(0)} KiB · hash ${info.codeHash.slice(
    0,
    12,
  )}…${pin} · registry: ${info.contractCount} contract(s).`;

  const required = BigInt(info.requiredDeposit);
  const deposit = byId<HTMLInputElement>("deploy-deposit");
  const depositValue = formatUnits(required, 24, 6).replaceAll(",", "");
  if (!deposit.value || deposit.value === lastAutoDeposit) {
    deposit.value = depositValue;
    lastAutoDeposit = depositValue;
  }
  hint.textContent = `Factory minimum: ${formatNear(required, 4)}. Excess is forwarded to the child.`;
}

function renderOwned(): void {
  const count = byId<HTMLElement>("favorites-count");
  count.textContent = `${state.favorites.size} favorite${state.favorites.size === 1 ? "" : "s"}`;

  const summary = byId<HTMLElement>("owned-summary");
  const list = byId<HTMLElement>("owned-list");

  if (!state.ownedAddress) {
    summary.innerHTML = "";
    list.innerHTML = `<div class="empty"><strong>No address selected</strong>Enter an owner account or connect a wallet to scan the factory registry.</div>`;
    return;
  }

  if (state.ownedLoading) {
    summary.innerHTML = "";
    list.innerHTML = `<div class="empty"><span class="loading">Scanning factory registry and verifying owners…</span></div>`;
    return;
  }

  const filtered = state.owned
    .filter((item) => !state.favoritesOnly || state.favorites.has(item.record.account_id))
    .sort((left, right) => {
      const leftFavorite = state.favorites.has(left.record.account_id) ? 1 : 0;
      const rightFavorite = state.favorites.has(right.record.account_id) ? 1 : 0;
      return (
        rightFavorite - leftFavorite ||
        left.record.account_id.localeCompare(right.record.account_id)
      );
    });

  if (state.ownedScanned) {
    summary.innerHTML = `<p class="muted small">Owned by <code>${escapeHtml(
      state.ownedAddress,
    )}</code>: <strong>${state.owned.length}</strong> contract(s)${
      state.favoritesOnly ? `, filtered to ${filtered.length} favorite(s)` : ""
    }. Favorites are stored locally and keep spam contracts out of the way.</p>`;
  } else {
    summary.innerHTML = "";
  }

  if (!state.ownedScanned) {
    list.innerHTML = `<div class="empty"><strong>Ready to scan</strong>Press Scan to read the factory registry for <code>${escapeHtml(
      state.ownedAddress,
    )}</code>.</div>`;
    return;
  }

  if (filtered.length === 0) {
    list.innerHTML = state.favoritesOnly
      ? `<div class="empty"><strong>No favorites</strong>Favorite a contract to keep it visible while filtering out spam.</div>`
      : `<div class="empty"><strong>No contracts found</strong>This address does not currently own a registry contract. Deploy one in the first tab.</div>`;
    return;
  }

  list.innerHTML = filtered.map(ownedCard).join("");
}

function renderRoles(): void {
  const summary = byId<HTMLElement>("roles-summary");
  const list = byId<HTMLElement>("roles-list");

  if (!state.rolesAddress) {
    summary.innerHTML = "";
    list.innerHTML = `<div class="empty"><strong>No address selected</strong>Enter an account or connect a wallet to find role memberships.</div>`;
    return;
  }

  if (state.rolesLoading) {
    summary.innerHTML = "";
    list.innerHTML = `<div class="empty"><span class="loading">Reading role memberships from the registry…</span></div>`;
    return;
  }

  const roleCount = state.memberships.reduce((total, item) => total + item.memberships.length, 0);
  if (state.rolesScanned) {
    summary.innerHTML = `<p class="muted small"><code>${escapeHtml(
      state.rolesAddress,
    )}</code> holds <strong>${roleCount}</strong> role(s) across <strong>${
      state.memberships.length
    }</strong> contract(s).</p>`;
  } else {
    summary.innerHTML = "";
  }

  if (!state.rolesScanned) {
    list.innerHTML = `<div class="empty"><strong>Ready to scan</strong>Press Scan to read roles for <code>${escapeHtml(
      state.rolesAddress,
    )}</code>.</div>`;
    return;
  }

  if (state.memberships.length === 0) {
    list.innerHTML = `<div class="empty"><strong>No roles found</strong>This address does not hold any role NFTs in registry contracts.</div>`;
    return;
  }

  list.innerHTML = state.memberships.map(membershipCard).join("");
}

function renderAll(): void {
  renderTabs();
  renderWallet();
  renderFactory();
  renderOwned();
  renderRoles();
}

function findSnapshot(contractId: string): ContractSnapshot | null {
  return (
    state.owned.find((item) => item.record.account_id === contractId)?.snapshot ??
    state.memberships.find((item) => item.snapshot.accountId === contractId)?.snapshot ??
    null
  );
}

function activeStatus(): HTMLElement {
  if (state.tab === "contracts") return byId("owned-status");
  if (state.tab === "roles") return byId("roles-status");
  return byId("deploy-status");
}

function updateUrl(): void {
  const url = new URL(window.location.href);
  url.searchParams.set("tab", state.tab);
  if (state.tab === "contracts" && state.ownedAddress) {
    url.searchParams.set("account", state.ownedAddress);
  } else if (state.tab === "roles" && state.rolesAddress) {
    url.searchParams.set("account", state.rolesAddress);
  } else {
    url.searchParams.delete("account");
  }
  window.history.replaceState({}, "", url);
}

function restoreFromUrl(): void {
  const params = new URLSearchParams(window.location.search);
  const tab = params.get("tab");
  if (tab === "deploy" || tab === "contracts" || tab === "roles") {
    state.tab = tab;
  }
  const account = params.get("account")?.trim() ?? "";
  if (account) {
    state.ownedAddress = account;
    state.rolesAddress = account;
    byId<HTMLInputElement>("owned-address").value = account;
    byId<HTMLInputElement>("roles-address").value = account;
  }
  if (params.get("favorites") === "1") {
    state.favoritesOnly = true;
    byId<HTMLInputElement>("favorites-only").checked = true;
  }
}

async function getRegistry(force = false): Promise<FactoryContractRecord[]> {
  if (!state.config.factoryContractId) {
    throw new Error("Factory contract is not configured.");
  }
  if (!force && registryCache && Date.now() - registryCache.fetchedAt < 60_000) {
    return registryCache.records;
  }
  const records = await listFactoryContracts(
    state.config.rpcUrl,
    state.config.factoryContractId,
  );
  registryCache = { fetchedAt: Date.now(), records };
  return records;
}

async function latestBlockTimestamp(): Promise<bigint> {
  try {
    return await getFinalBlockTimestampNs(state.config.rpcUrl);
  } catch {
    return BigInt(Date.now()) * 1_000_000n;
  }
}

async function refreshFactory(): Promise<void> {
  if (!state.config.factoryContractId) {
    renderFactory();
    return;
  }
  try {
    state.factoryInfo = await loadFactoryInfo(
      state.config.rpcUrl,
      state.config.factoryContractId,
    );
    state.factoryError = "";
  } catch (error) {
    state.factoryInfo = null;
    state.factoryError = walletErrorMessage(error);
  }
  renderFactory();
}

async function scanOwned(force = false): Promise<void> {
  const address = state.ownedAddress.trim();
  const status = byId<HTMLElement>("owned-status");
  if (!isValidAccountId(address)) {
    setStatus(status, "Enter a valid NEAR account id.", "error");
    return;
  }

  state.ownedAddress = address;
  const run = ++ownedRun;
  state.ownedLoading = true;
  state.ownedScanned = false;
  state.owned = [];
  state.favorites = loadFavorites(localStorage, state.config.network, address);
  renderOwned();
  updateUrl();
  setStatus(status, `Scanning the factory registry for ${address}…`);

  try {
    const [records, blockTimestamp] = await Promise.all([
      getRegistry(force),
      latestBlockTimestamp(),
    ]);
    const owned = await loadOwnedContracts(state.config.rpcUrl, records, address);
    if (run !== ownedRun) return;
    state.blockTimestampNs = blockTimestamp;
    state.owned = owned;
    state.ownedScanned = true;
    renderOwned();
    setStatus(
      status,
      records.length === 0
        ? "The factory registry is empty. Deploy the first contract in the first tab."
        : `Found ${owned.length} contract(s) currently owned by ${address}.`,
      "ok",
    );
  } catch (error) {
    if (run !== ownedRun) return;
    state.owned = [];
    state.ownedScanned = true;
    renderOwned();
    setStatus(status, walletErrorMessage(error), "error");
  } finally {
    if (run === ownedRun) {
      state.ownedLoading = false;
      renderOwned();
    }
  }
}

async function scanRoles(force = false): Promise<void> {
  const address = state.rolesAddress.trim();
  const status = byId<HTMLElement>("roles-status");
  if (!isValidAccountId(address)) {
    setStatus(status, "Enter a valid NEAR account id.", "error");
    return;
  }

  state.rolesAddress = address;
  const run = ++rolesRun;
  state.rolesLoading = true;
  state.rolesScanned = false;
  state.memberships = [];
  renderRoles();
  updateUrl();
  setStatus(status, `Reading roles for ${address} across the factory registry…`);

  try {
    const [records, blockTimestamp] = await Promise.all([
      getRegistry(force),
      latestBlockTimestamp(),
    ]);
    const memberships = await loadMembershipContracts(
      state.config.rpcUrl,
      records,
      address,
    );
    if (run !== rolesRun) return;
    state.blockTimestampNs = blockTimestamp;
    state.memberships = memberships;
    state.rolesScanned = true;
    renderRoles();
    const roleCount = memberships.reduce((total, item) => total + item.memberships.length, 0);
    setStatus(
      status,
      records.length === 0
        ? "The factory registry is empty. Deploy a contract first."
        : `Found ${roleCount} role membership(s) across ${memberships.length} contract(s).`,
      "ok",
    );
  } catch (error) {
    if (run !== rolesRun) return;
    state.memberships = [];
    state.rolesScanned = true;
    renderRoles();
    setStatus(status, walletErrorMessage(error), "error");
  } finally {
    if (run === rolesRun) {
      state.rolesLoading = false;
      renderRoles();
    }
  }
}

function selectTab(tab: Tab): void {
  state.tab = tab;
  renderTabs();
  updateUrl();
  if (tab === "contracts" && state.ownedAddress && !state.ownedScanned && !state.ownedLoading) {
    void scanOwned();
  }
  if (tab === "roles" && state.rolesAddress && !state.rolesScanned && !state.rolesLoading) {
    void scanRoles();
  }
}

function updateWallet(accountId: string | null): void {
  state.walletAccount = accountId;
  renderWallet();

  if (accountId) {
    const deployOwner = byId<HTMLInputElement>("deploy-owner");
    if (!deployOwner.value) deployOwner.value = accountId;

    if (!state.ownedAddress) {
      state.ownedAddress = accountId;
      byId<HTMLInputElement>("owned-address").value = accountId;
    }
    if (!state.rolesAddress) {
      state.rolesAddress = accountId;
      byId<HTMLInputElement>("roles-address").value = accountId;
    }
  }

  // Admin controls depend on the connected account.
  renderOwned();
  renderRoles();

  if (
    accountId &&
    state.tab === "contracts" &&
    state.ownedAddress &&
    !state.ownedScanned &&
    !state.ownedLoading
  ) {
    void scanOwned();
  }
  if (
    accountId &&
    state.tab === "roles" &&
    state.rolesAddress &&
    !state.rolesScanned &&
    !state.rolesLoading
  ) {
    void scanRoles();
  }
}

async function executeWrite(
  status: HTMLElement,
  description: string,
  contractId: string,
  methodName: string,
  args: Record<string, unknown>,
  options: { deposit?: bigint; gas?: bigint } = {},
): Promise<boolean> {
  if (!state.selector) {
    setStatus(status, "Connect a NEAR wallet first.", "error");
    return false;
  }
  if (!state.walletAccount) {
    setStatus(status, "Connect a NEAR wallet first.", "error");
    return false;
  }

  setStatus(status, `${description} — confirm in your wallet…`);
  try {
    const outcome = await signAndSendFunctionCall(state.selector, state.walletAccount, {
      contractId,
      methodName,
      args,
      deposit: options.deposit ?? 0n,
      gas: options.gas ?? WRITE_GAS,
    });
    const hash = transactionHash(outcome);
    if (hash) {
      setStatusLink(status, `${description} submitted.`, transactionExplorerUrl(state.config.explorerUrl, hash));
    } else {
      setStatus(
        status,
        `${description} submitted. If your wallet opened another tab, return here after approving.`,
        "ok",
      );
    }
    registryCache = null;
    window.setTimeout(() => {
      void refreshFactory();
      if (state.tab === "contracts" && state.ownedAddress) void scanOwned(true);
      if (state.tab === "roles" && state.rolesAddress) void scanRoles(true);
    }, 1_800);
    return true;
  } catch (error) {
    setStatus(status, walletErrorMessage(error), "error");
    return false;
  }
}

async function handleDeploy(form: HTMLFormElement): Promise<void> {
  const status = byId<HTMLElement>("deploy-status");
  if (!state.config.factoryContractId) {
    setStatus(status, "The factory contract is not configured.", "error");
    return;
  }
  if (!state.factoryInfo || state.factoryInfo.codeLen === 0) {
    setStatus(status, "The factory is not ready to deploy contracts.", "error");
    return;
  }
  if (!state.selector || !state.walletAccount) {
    setStatus(status, "Connect a NEAR wallet to pay for the deployment.", "error");
    return;
  }

  const data = new FormData(form);
  const name = str(data.get("name")).trim();
  const owner = str(data.get("owner")).trim();
  const token = str(data.get("token")).trim();
  const nameError = subaccountNameError(name, state.config.factoryContractId);
  if (nameError) {
    setStatus(status, nameError, "error");
    return;
  }
  if (!isValidAccountId(owner)) {
    setStatus(status, "Enter a valid contract owner account id.", "error");
    return;
  }
  if (!isValidAccountId(token)) {
    setStatus(status, "Enter a valid NEP-141 salary token account id.", "error");
    return;
  }

  let interval: bigint;
  let deposit: bigint;
  try {
    interval = parseInterval(
      str(data.get("intervalValue")),
      str(data.get("intervalUnit")),
    );
    deposit = parseNear(str(data.get("deposit")));
  } catch (error) {
    setStatus(status, walletErrorMessage(error), "error");
    return;
  }

  const required = BigInt(state.factoryInfo.requiredDeposit);
  if (deposit < required) {
    setStatus(
      status,
      `Attach at least ${formatNear(required, 4)} to cover the wasm and child state.`,
      "error",
    );
    return;
  }

  const child = childAccountId(name, state.config.factoryContractId);
  setStatus(status, `Deploying ${child} — confirm in your wallet…`);
  try {
    const outcome = await signAndSendFunctionCall(state.selector, state.walletAccount, {
      contractId: state.config.factoryContractId,
      methodName: "deploy",
      args: {
        name,
        owner_id: owner,
        salary_token: token,
        payout_interval_ns: interval.toString(),
      },
      deposit,
      gas: DEPLOY_GAS,
    });
    const hash = transactionHash(outcome);
    if (hash) {
      setStatusLink(
        status,
        `Deployment of ${child} submitted. It appears after finality.`,
        transactionExplorerUrl(state.config.explorerUrl, hash),
      );
    } else {
      setStatus(
        status,
        `Deployment of ${child} submitted. Approve it in your wallet; this page will refresh when you return.`,
        "ok",
      );
    }

    if (owner === state.walletAccount) {
      const favorites = loadFavorites(localStorage, state.config.network, owner);
      favorites.add(child);
      saveFavorites(localStorage, state.config.network, owner, favorites);
    }
    showNotice(`Deployment of ${child} submitted.`);
    registryCache = null;
    form.reset();
    window.setTimeout(() => {
      void refreshFactory();
      if (state.ownedAddress === owner) void scanOwned(true);
    }, 3_000);
  } catch (error) {
    setStatus(status, walletErrorMessage(error), "error");
  }
}

async function handleAdminForm(kind: string, form: HTMLFormElement): Promise<void> {
  const contractId = form.dataset.contract ?? "";
  const snapshot = findSnapshot(contractId);
  const status = activeStatus();
  if (!snapshot) {
    setStatus(status, "Contract data is not loaded yet. Refresh and try again.", "error");
    return;
  }

  const data = new FormData(form);
  let ok = false;

  if (kind === "mint" || kind === "burn") {
    const role = form.dataset.role ?? "";
    const account = str(data.get("account")).trim();
    if (!isValidAccountId(account)) {
      setStatus(status, "Enter a valid account id.", "error");
      return;
    }
    if (kind === "burn" && !window.confirm(`Burn ${role} from ${account}? Unclaimed salary is forfeited.`)) {
      return;
    }
    ok = await executeWrite(
      status,
      `${kind === "mint" ? "Mint" : "Burn"} ${role} ${kind === "mint" ? "to" : "from"} ${account}`,
      contractId,
      kind === "mint" ? "mint_role" : "burn_role",
      { account_id: account, role },
    );
  } else if (kind === "role-salary") {
    const role = str(data.get("role")).trim();
    if (!role) {
      setStatus(status, "Enter a role name.", "error");
      return;
    }
    let salary: bigint;
    try {
      salary = parseUnits(str(data.get("amount")), snapshot.token.decimals);
    } catch (error) {
      setStatus(status, walletErrorMessage(error), "error");
      return;
    }
    ok = await executeWrite(status, `Save role ${role}`, contractId, "set_role_salary", {
      role,
      salary: salary.toString(),
    });
  } else if (kind === "role-owner") {
    const role = str(data.get("role")).trim();
    const owner = str(data.get("owner")).trim();
    if (!role) {
      setStatus(status, "Enter a role name.", "error");
      return;
    }
    if (owner && !isValidAccountId(owner)) {
      setStatus(status, "Enter a valid role owner, or leave it blank.", "error");
      return;
    }
    ok = await executeWrite(status, `Set owner for ${role}`, contractId, "set_role_owner", {
      role,
      owner: owner || null,
    });
  } else if (kind === "payout") {
    const token = str(data.get("token")).trim();
    if (!isValidAccountId(token)) {
      setStatus(status, "Enter a valid salary token account id.", "error");
      return;
    }
    let interval: bigint;
    try {
      interval = parseInterval(
        str(data.get("intervalValue")),
        str(data.get("intervalUnit")),
      );
    } catch (error) {
      setStatus(status, walletErrorMessage(error), "error");
      return;
    }
    ok = await executeWrite(status, "Update payroll configuration", contractId, "set_payout_config", {
      salary_token: token,
      payout_interval_ns: interval.toString(),
    });
  } else if (kind === "transfer-owner") {
    const owner = str(data.get("owner")).trim();
    if (!isValidAccountId(owner)) {
      setStatus(status, "Enter a valid new owner account id.", "error");
      return;
    }
    if (!window.confirm(`Transfer contract ownership to ${owner}?`)) return;
    ok = await executeWrite(status, `Transfer ownership to ${owner}`, contractId, "set_owner", {
      new_owner_id: owner,
    });
  }

  if (ok) {
    form.reset();
    showNotice("Transaction submitted.");
  }
}

async function handleClaim(button: HTMLButtonElement): Promise<void> {
  const contractId = button.dataset.contract ?? "";
  const account = button.dataset.account ?? "";
  const role = button.dataset.role;
  if (!isValidAccountId(contractId) || !isValidAccountId(account)) {
    showNotice("Invalid claim target.", "error");
    return;
  }

  button.disabled = true;
  const status = byId<HTMLElement>("roles-status");
  const ok = await executeWrite(
    status,
    role ? `Claim salary for ${role}` : "Claim all salary",
    contractId,
    role ? "claim_role" : "claim",
    role ? { account_id: account, role } : { account_id: account },
    { gas: CLAIM_GAS },
  );
  if (!ok) button.disabled = false;
}

function handleFavorite(button: HTMLButtonElement): void {
  const contractId = button.dataset.contract ?? "";
  if (!isValidAccountId(state.ownedAddress)) {
    showNotice("Enter an owner address before favoriting contracts.", "error");
    return;
  }
  state.favorites = toggleFavorite(
    localStorage,
    state.config.network,
    state.ownedAddress,
    contractId,
  );
  const active = state.favorites.has(contractId);
  showNotice(active ? "Added to favorites." : "Removed from favorites.");
  renderOwned();
}

function bindEvents(): void {
  document.querySelectorAll<HTMLButtonElement>("[data-tab]").forEach((button) => {
    button.addEventListener("click", () => {
      const tab = button.dataset.tab;
      if (tab === "deploy" || tab === "contracts" || tab === "roles") {
        selectTab(tab);
      }
    });
  });

  byId<HTMLButtonElement>("wallet-button").addEventListener("click", () => {
    if (!state.modal) {
      showNotice("Wallet selector is still loading. Try again in a moment.", "error");
      return;
    }
    state.modal.show();
  });

  byId<HTMLButtonElement>("wallet-disconnect").addEventListener("click", async () => {
    if (!state.selector) return;
    try {
      const wallet = await state.selector.wallet();
      await wallet.signOut();
      updateWallet(null);
    } catch (error) {
      showNotice(walletErrorMessage(error), "error");
    }
  });

  byId<HTMLButtonElement>("owned-use-wallet").addEventListener("click", () => {
    if (!state.walletAccount) {
      showNotice("Connect a wallet first.", "error");
      return;
    }
    state.ownedAddress = state.walletAccount;
    byId<HTMLInputElement>("owned-address").value = state.walletAccount;
    void scanOwned();
  });

  byId<HTMLButtonElement>("roles-use-wallet").addEventListener("click", () => {
    if (!state.walletAccount) {
      showNotice("Connect a wallet first.", "error");
      return;
    }
    state.rolesAddress = state.walletAccount;
    byId<HTMLInputElement>("roles-address").value = state.walletAccount;
    void scanRoles();
  });

  byId<HTMLButtonElement>("roles-refresh").addEventListener("click", () => {
    void scanRoles(true);
  });

  byId<HTMLInputElement>("favorites-only").addEventListener("change", (event) => {
    state.favoritesOnly = (event.target as HTMLInputElement).checked;
    const url = new URL(window.location.href);
    if (state.favoritesOnly) url.searchParams.set("favorites", "1");
    else url.searchParams.delete("favorites");
    window.history.replaceState({}, "", url);
    renderOwned();
  });

  byId<HTMLInputElement>("deploy-name").addEventListener("input", () => {
    const name = byId<HTMLInputElement>("deploy-name").value.trim();
    const preview = byId<HTMLElement>("deploy-preview");
    if (!state.config.factoryContractId) {
      preview.textContent = "Configure the factory contract to deploy.";
      return;
    }
    const error = subaccountNameError(name, state.config.factoryContractId);
    preview.textContent = error
      ? error
      : `Child account: ${childAccountId(name, state.config.factoryContractId)}`;
    preview.className = error ? "preview status error" : "preview";
  });

  document.addEventListener("submit", (event) => {
    const form = event.target;
    if (!(form instanceof HTMLFormElement)) return;

    if (form.id === "deploy-form") {
      event.preventDefault();
      void handleDeploy(form);
      return;
    }
    if (form.id === "owned-form") {
      event.preventDefault();
      state.ownedAddress = byId<HTMLInputElement>("owned-address").value.trim();
      void scanOwned();
      return;
    }
    if (form.id === "roles-form") {
      event.preventDefault();
      state.rolesAddress = byId<HTMLInputElement>("roles-address").value.trim();
      void scanRoles();
      return;
    }
    const kind = form.dataset.form;
    if (kind) {
      event.preventDefault();
      void handleAdminForm(kind, form);
    }
  });

  document.addEventListener("click", (event) => {
    const target = event.target;
    if (!(target instanceof HTMLElement)) return;
    const action = target.closest<HTMLElement>("[data-action]");
    if (!action) return;
    const button = action as HTMLButtonElement;
    const name = action.dataset.action;
    if (name === "favorite") {
      handleFavorite(button);
    } else if (name === "claim") {
      void handleClaim(button);
    } else if (name === "claim-all") {
      void handleClaim(button);
    }
  });
}

async function startWallet(): Promise<void> {
  if (!state.config.factoryContractId) {
    renderWallet();
  }
  try {
    const wallet = await initWallet(state.config);
    state.selector = wallet.selector;
    state.modal = wallet.modal;

    wallet.selector.on("signedIn", ({ accounts }) => {
      updateWallet(accounts[0]?.accountId ?? null);
      showNotice("Wallet connected.");
    });
    wallet.selector.on("signedOut", () => {
      updateWallet(null);
      showNotice("Wallet disconnected.");
    });
    wallet.selector.on("accountsChanged", ({ accounts }) => {
      updateWallet(accounts[0]?.accountId ?? null);
    });

    updateWallet(getWalletAccountId(wallet.selector));
    renderWallet();
  } catch (error) {
    renderWallet();
    showNotice(`Wallet selector failed to load: ${walletErrorMessage(error)}`, "error");
  }
}

async function init(): Promise<void> {
  byId<HTMLElement>("network-badge").textContent = state.config.network;
  bindEvents();
  restoreFromUrl();
  renderAll();

  if (state.tab === "contracts" && state.ownedAddress) {
    void scanOwned();
  }
  if (state.tab === "roles" && state.rolesAddress) {
    void scanRoles();
  }

  void refreshFactory();
  void startWallet();
}

void init();
