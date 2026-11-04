import { describe, expect, it } from "vitest";

import {
  formatDurationNs,
  formatPercent,
  formatUnits,
  intervalProgress,
  parseNear,
  parseUnits,
  shortenAccount,
} from "../src/lib/format";

describe("formatUnits", () => {
  it("formats whole and fractional token amounts", () => {
    expect(formatUnits("1000000000000000000", 18)).toBe("1");
    expect(formatUnits("1234567890000000000", 18, 4)).toBe("1.2345");
    expect(formatUnits("1500000", 6)).toBe("1.5");
    expect(formatUnits("1234567", 6, 2)).toBe("1.23");
    expect(formatUnits("1234", 0)).toBe("1,234");
    expect(formatUnits(0n, 24)).toBe("0");
  });

  it("handles negative values and malformed strings", () => {
    expect(formatUnits("-1500000", 6)).toBe("-1.5");
    expect(formatUnits("not-a-number", 6)).toBe("not-a-number");
  });

  it("rejects invalid decimals", () => {
    expect(() => formatUnits(1n, -1)).toThrow();
    expect(() => formatUnits(1n, 1.5)).toThrow();
  });
});

describe("parseUnits", () => {
  it("parses decimal input into raw units", () => {
    expect(parseUnits("1.5", 6)).toBe(1_500_000n);
    expect(parseUnits(".5", 2)).toBe(50n);
    expect(parseUnits("1.", 0)).toBe(1n);
    expect(parseUnits("0", 24)).toBe(0n);
    expect(parseNear("1")).toBe(10n ** 24n);
  });

  it("rejects malformed and over-precise input", () => {
    expect(() => parseUnits("", 6)).toThrow();
    expect(() => parseUnits(".", 6)).toThrow();
    expect(() => parseUnits("-1", 6)).toThrow();
    expect(() => parseUnits("1.1234567", 6)).toThrow();
    expect(() => parseUnits("1e6", 6)).toThrow();
  });
});

describe("durations", () => {
  it("uses the largest appropriate unit", () => {
    expect(formatDurationNs(604_800_000_000_000n)).toBe("1 week");
    expect(formatDurationNs(1_209_600_000_000_000n)).toBe("2 weeks");
    expect(formatDurationNs(86_400_000_000_000n)).toBe("1 day");
    expect(formatDurationNs(5_400_000_000_000n)).toBe("1.5 hours");
    expect(formatDurationNs(0n)).toBe("0 seconds");
  });
});

describe("intervalProgress", () => {
  it("reports progress toward the next payout", () => {
    const progress = intervalProgress(0n, 100n, 250n);
    expect(progress.completedPeriods).toBe(2n);
    expect(progress.fraction).toBeCloseTo(0.5);
    expect(progress.remainingNs).toBe(50n);
  });

  it("treats a missing claim timestamp as zero progress", () => {
    const progress = intervalProgress(null, 100n, 250n);
    expect(progress.fraction).toBe(0);
    expect(progress.remainingNs).toBe(100n);
  });

  it("does not go backwards when the clock is earlier than the claim", () => {
    const progress = intervalProgress(500n, 100n, 250n);
    expect(progress.fraction).toBe(0);
    expect(progress.elapsedNs).toBe(0n);
  });
});

describe("display helpers", () => {
  it("formats percentages in range", () => {
    expect(formatPercent(0.123)).toBe("12.3%");
    expect(formatPercent(-1)).toBe("0.0%");
    expect(formatPercent(2)).toBe("100.0%");
  });

  it("shortens long accounts", () => {
    expect(shortenAccount("alice.near")).toBe("alice.near");
    expect(shortenAccount("a".repeat(40), 8)).toBe(`${"a".repeat(8)}…${"a".repeat(8)}`);
  });
});
