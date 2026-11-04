import { describe, expect, it } from "vitest";

import { mapLimit } from "../src/lib/async";

describe("mapLimit", () => {
  it("preserves order", async () => {
    const result = await mapLimit([1, 2, 3, 4], 2, async (value) => value * 2);
    expect(result).toEqual([2, 4, 6, 8]);
  });

  it("bounds concurrency", async () => {
    let active = 0;
    let peak = 0;
    await mapLimit([1, 2, 3, 4, 5, 6], 2, async (value) => {
      active += 1;
      peak = Math.max(peak, active);
      await new Promise((resolve) => setTimeout(resolve, 2));
      active -= 1;
      return value;
    });
    expect(peak).toBeLessThanOrEqual(2);
    expect(peak).toBeGreaterThan(1);
  });

  it("handles empty input", async () => {
    expect(await mapLimit([], 3, async () => 1)).toEqual([]);
  });
});
