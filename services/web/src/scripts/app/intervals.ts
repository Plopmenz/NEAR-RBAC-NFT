import { escapeHtml } from "./dom";
import { INTERVAL_UNITS } from "./state";

export function preferredInterval(ns: bigint | string): { value: string; unit: string } {
  const value = BigInt(ns);
  for (const unit of ["weeks", "days", "hours", "minutes", "seconds"]) {
    const factor = INTERVAL_UNITS[unit];
    if (value > 0n && value % factor === 0n) {
      return { value: (value / factor).toString(), unit };
    }
  }
  return { value: (Number(value) / 1e9).toString(), unit: "seconds" };
}

export function intervalInputs(value: bigint | string): string {
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

export function parseInterval(value: string, unit: string): bigint {
  const raw = value.trim();
  if (!/^\d+$/.test(raw)) throw new Error("The payout interval must be a whole number.");
  const amount = BigInt(raw);
  if (amount <= 0n) throw new Error("The payout interval must be positive.");
  const factor = INTERVAL_UNITS[unit];
  if (!factor) throw new Error("Choose a valid interval unit.");
  return amount * factor;
}
