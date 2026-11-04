const WEEK_NS = 604_800_000_000_000n;
const DAY_NS = 86_400_000_000_000n;
const HOUR_NS = 3_600_000_000_000n;
const MINUTE_NS = 60_000_000_000n;
const SECOND_NS = 1_000_000_000n;

function toBigInt(value: bigint | string | number): bigint {
  if (typeof value === "bigint") return value;
  if (typeof value === "number") return BigInt(Math.trunc(value));
  return BigInt(value);
}

function groupThousands(value: string): string {
  return value.replace(/\B(?=(\d{3})+(?!\d))/g, ",");
}

/** Formats a raw integer amount with token decimals, truncating extra fractional digits. */
export function formatUnits(
  value: bigint | string | number,
  decimals: number,
  maxFractionDigits = 4,
): string {
  if (!Number.isInteger(decimals) || decimals < 0) {
    throw new Error("decimals must be a non-negative integer");
  }
  let raw: bigint;
  try {
    raw = toBigInt(value);
  } catch {
    return String(value);
  }

  const negative = raw < 0n;
  const absolute = negative ? -raw : raw;
  const base = 10n ** BigInt(decimals);
  const whole = absolute / base;
  let fraction = decimals === 0 ? "" : (absolute % base).toString().padStart(decimals, "0");

  if (maxFractionDigits < decimals) {
    fraction = fraction.slice(0, Math.max(0, maxFractionDigits));
  }
  fraction = fraction.replace(/0+$/, "");

  const sign = negative ? "-" : "";
  const wholeText = groupThousands(whole.toString());
  return fraction ? `${sign}${wholeText}.${fraction}` : `${sign}${wholeText}`;
}

export function formatToken(
  value: bigint | string | number,
  token: { decimals: number; symbol: string },
  maxFractionDigits = 4,
): string {
  return `${formatUnits(value, token.decimals, maxFractionDigits)} ${token.symbol}`;
}

export function formatNear(value: bigint | string | number, maxFractionDigits = 4): string {
  return `${formatUnits(value, 24, maxFractionDigits)} NEAR`;
}

function trimDecimal(value: number, digits = 2): string {
  return value.toFixed(digits).replace(/\.?0+$/, "");
}

/** Formats a nanosecond duration in the largest sensible unit. */
export function formatDurationNs(value: bigint | string | number): string {
  const ns = toBigInt(value);
  if (ns <= 0n) return "0 seconds";

  const units: Array<[bigint, string]> = [
    [WEEK_NS, "week"],
    [DAY_NS, "day"],
    [HOUR_NS, "hour"],
    [MINUTE_NS, "minute"],
    [SECOND_NS, "second"],
  ];

  for (const [unit, name] of units) {
    if (ns >= unit) {
      const amount = Number(ns) / Number(unit);
      const text = trimDecimal(amount);
      return `${text} ${name}${text === "1" ? "" : "s"}`;
    }
  }
  return `${ns.toString()} ns`;
}

export interface IntervalProgress {
  /** Fraction of the current interval that has elapsed, in [0, 1). */
  fraction: number;
  elapsedNs: bigint;
  remainingNs: bigint;
  completedPeriods: bigint;
}

/** Computes progress toward the next salary payout. */
export function intervalProgress(
  lastClaimNs: bigint | string | null,
  intervalNs: bigint | string,
  nowNs: bigint | string,
): IntervalProgress {
  const interval = toBigInt(intervalNs);
  const now = toBigInt(nowNs);
  if (interval <= 0n) {
    return { fraction: 0, elapsedNs: 0n, remainingNs: 0n, completedPeriods: 0n };
  }

  const last = lastClaimNs === null ? now : toBigInt(lastClaimNs);
  const elapsed = now > last ? now - last : 0n;
  const completedPeriods = elapsed / interval;
  const remainder = elapsed % interval;
  return {
    fraction: Number(remainder) / Number(interval),
    elapsedNs: elapsed,
    remainingNs: interval - remainder,
    completedPeriods,
  };
}

/** Parses a human decimal amount into raw token units. Throws on malformed input. */
export function parseUnits(value: string, decimals: number): bigint {
  if (!Number.isInteger(decimals) || decimals < 0) {
    throw new Error("decimals must be a non-negative integer");
  }
  const trimmed = value.trim();
  if (!/^\d*(?:\.\d*)?$/.test(trimmed) || trimmed === "" || trimmed === ".") {
    throw new Error("Enter a positive decimal amount.");
  }

  const [wholePart = "0", fractionPart = ""] = trimmed.split(".");
  if (fractionPart.length > decimals) {
    throw new Error(`Use at most ${decimals} decimal places.`);
  }
  const whole = BigInt(wholePart || "0");
  const fraction = BigInt((fractionPart + "0".repeat(decimals)).slice(0, decimals) || "0");
  return whole * 10n ** BigInt(decimals) + fraction;
}

export function parseNear(value: string): bigint {
  return parseUnits(value, 24);
}

/** Formats a block timestamp (ns) for display. */
export function formatTimestampNs(value: bigint | string): string {
  const milliseconds = Number(toBigInt(value) / 1_000_000n);
  if (!Number.isFinite(milliseconds) || milliseconds <= 0) return "unknown";
  return new Date(milliseconds).toLocaleString();
}

/** Shortens a long account id for compact display. */
export function shortenAccount(value: string, keep = 8): string {
  if (value.length <= keep * 2 + 1) return value;
  return `${value.slice(0, keep)}…${value.slice(-keep)}`;
}

export function formatPercent(fraction: number): string {
  return `${(Math.max(0, Math.min(1, fraction)) * 100).toFixed(1)}%`;
}
