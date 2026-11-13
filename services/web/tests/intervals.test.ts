import { describe, expect, it } from "vitest";

import { DEFAULT_PAYOUT_INTERVAL_NS } from "../src/lib/config";
import {
  intervalInputs,
  parseInterval,
  preferredInterval,
} from "../src/scripts/app/intervals";

describe("payout intervals", () => {
  it("defaults to 30 days", () => {
    expect(DEFAULT_PAYOUT_INTERVAL_NS).toBe(2_592_000_000_000_000n);
    expect(preferredInterval(DEFAULT_PAYOUT_INTERVAL_NS)).toEqual({ value: "30", unit: "days" });
    expect(parseInterval("30", "days")).toBe(DEFAULT_PAYOUT_INTERVAL_NS);
  });

  it("parses every supported unit", () => {
    expect(parseInterval("2", "seconds")).toBe(2_000_000_000n);
    expect(parseInterval("1", "minutes")).toBe(60_000_000_000n);
    expect(parseInterval("1", "hours")).toBe(3_600_000_000_000n);
    expect(parseInterval("1", "weeks")).toBe(604_800_000_000_000n);
  });

  it("rejects invalid values", () => {
    expect(() => parseInterval("1.5", "days")).toThrow("whole number");
    expect(() => parseInterval("0", "days")).toThrow("positive");
    expect(() => parseInterval("1", "fortnights")).toThrow("valid interval unit");
  });

  it("renders the preferred unit as selected", () => {
    const html = intervalInputs(DEFAULT_PAYOUT_INTERVAL_NS);
    expect(html).toContain('value="30"');
    expect(html).toContain('<option value="days" selected>days</option>');
  });
});
