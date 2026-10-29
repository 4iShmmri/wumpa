import { describe, expect, it } from "vitest";
import {
  countKinds,
  diffRuns,
  groupByPage,
  passesFilter,
  runsToChanges,
  wrapIndex,
  ALL_KINDS,
  type PageTokens,
} from "./compare";

const page = (tokens: string[]): PageTokens => ({
  tokens,
  boxes: tokens.map((_, i) => [i * 10, 0, i * 10 + 8, 10] as [number, number, number, number]),
});

describe("diffRuns", () => {
  it("returns no runs for identical pages", () => {
    expect(diffRuns(["a", "b", "c"], ["a", "b", "c"])).toEqual([]);
  });
  it("classifies replacements, insertions and deletions", () => {
    const runs = diffRuns(["a", "b", "c", "d"], ["a", "x", "c", "d", "e"]);
    const changes = runsToChanges(runs, page(["a", "b", "c", "d"]), page(["a", "x", "c", "d", "e"]), 0, 0);
    expect(changes.map((c) => c.kind)).toEqual(["replaced", "inserted"]);
    expect(changes[0].oldText).toBe("b");
    expect(changes[0].newText).toBe("x");
    expect(changes[1].newText).toBe("e");
  });
  it("reports a deletion with the removed text", () => {
    const runs = diffRuns(["a", "b", "c"], ["a", "c"]);
    const [c] = runsToChanges(runs, page(["a", "b", "c"]), page(["a", "c"]), 2, 7);
    expect(c.kind).toBe("deleted");
    expect(c.oldText).toBe("b");
    expect(c.page).toBe(2);
    expect(c.id).toBe(7);
  });
});

describe("change helpers", () => {
  const runs = diffRuns(["a", "b"], ["a", "c", "d"]);
  const changes = runsToChanges(runs, page(["a", "b"]), page(["a", "c", "d"]), 0, 0);
  it("counts kinds", () => {
    expect(countKinds(changes).replaced).toBe(1);
  });
  it("filters by kind", () => {
    expect(changes.every((c) => passesFilter(c, ALL_KINDS))).toBe(true);
    expect(changes.some((c) => passesFilter(c, { ...ALL_KINDS, replaced: false }))).toBe(false);
  });
  it("groups by page and searches text", () => {
    expect(groupByPage(changes).length).toBe(1);
    expect(groupByPage(changes, "zzz").length).toBe(0);
    expect(groupByPage(changes, "c d").length).toBe(1);
  });
  it("wraps indexes", () => {
    expect(wrapIndex(-1, 3)).toBe(2);
    expect(wrapIndex(3, 3)).toBe(0);
  });
});
