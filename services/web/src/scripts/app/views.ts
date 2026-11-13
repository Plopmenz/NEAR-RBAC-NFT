import { factoryExplorerUrl } from "../../lib/factory";
import {
  formatDurationNs,
  formatNear,
  formatPercent,
  formatTimestampNs,
  formatToken,
  formatUnits,
  intervalProgress,
  shortenAccount,
} from "../../lib/format";
import {
  canManageContract,
  canManageRole,
  manageableRoles,
} from "../../lib/payrole";
import type {
  ContractSnapshot,
  MembershipContract,
  OwnedContract,
  RoleView,
} from "../../lib/types";
import { byId, escapeHtml } from "./dom";
import { intervalInputs } from "./intervals";
import { runtime, state } from "./state";

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

function fundingStatus(snapshot: ContractSnapshot): string {
  if (snapshot.ftBalance === null || snapshot.accruedLiabilities === null) {
    return '<span class="muted">Unavailable</span>';
  }
  let balance: bigint;
  let liabilities: bigint;
  try {
    balance = BigInt(snapshot.ftBalance);
    liabilities = BigInt(snapshot.accruedLiabilities);
  } catch {
    return '<span class="muted">Unavailable</span>';
  }
  const badge =
    liabilities > balance
      ? '<span class="tag error">Underfunded</span>'
      : '<span class="tag ok">Funded</span>';
  return `${badge}<br />${escapeHtml(
    formatToken(balance, snapshot.token, 4),
  )} balance<br /><span class="muted">${escapeHtml(
    formatToken(liabilities, snapshot.token, 4),
  )} accrued liabilities</span>`;
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
    <div><span class="meta-label">Salary funding</span><span class="meta-value">${fundingStatus(
      snapshot,
    )}</span></div>
    <div><span class="meta-label">Roles</span><span class="meta-value">${
      snapshot.roles.length
    }</span></div>
  </div>`;
}

function roleActions(snapshot: ContractSnapshot, role: RoleView): string {
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

export function renderTabs(): void {
  document.querySelectorAll<HTMLButtonElement>("[data-tab]").forEach((button) => {
    const selected = button.dataset.tab === state.tab;
    button.setAttribute("aria-selected", String(selected));
  });
  document.querySelectorAll<HTMLElement>("[data-panel]").forEach((panel) => {
    panel.hidden = panel.dataset.panel !== state.tab;
  });
}

export function renderWallet(): void {
  const connected = state.walletAccount;
  const account = byId<HTMLElement>("wallet-account");
  account.textContent = connected ? shortenAccount(connected, 10) : "Not connected";
  account.title = connected ?? "";
  const connect = byId<HTMLButtonElement>("wallet-button");
  connect.textContent = connected ? "Switch wallet" : "Connect wallet";
  connect.disabled = !state.modal;
  byId<HTMLButtonElement>("wallet-disconnect").hidden = !connected;
}

export function renderFactory(): void {
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
      "The factory has no payrole-nft wasm. It was not initialized correctly; deploy a new factory account.";
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
  if (!deposit.value || deposit.value === runtime.lastAutoDeposit) {
    deposit.value = depositValue;
    runtime.lastAutoDeposit = depositValue;
  }
  hint.textContent = `Factory minimum: ${formatNear(required, 4)}. Excess is forwarded to the child.`;
}

export function renderOwned(): void {
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

export function renderRoles(): void {
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

export function renderAll(): void {
  renderTabs();
  renderWallet();
  renderFactory();
  renderOwned();
  renderRoles();
}

export function findSnapshot(contractId: string): ContractSnapshot | null {
  return (
    state.owned.find((item) => item.record.account_id === contractId)?.snapshot ??
    state.memberships.find((item) => item.snapshot.accountId === contractId)?.snapshot ??
    null
  );
}

export function activeStatus(): HTMLElement {
  if (state.tab === "contracts") return byId("owned-status");
  if (state.tab === "roles") return byId("roles-status");
  return byId("deploy-status");
}
