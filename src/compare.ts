// Pure comparison model for the compare view: token extraction, the LCS diff
// into runs, and the Change/PageDiff helpers the UI reads. No React, no pdf.js
// runtime, so it can be unit-tested on plain data.
import type { TextItem, TextMarkedContent } from "pdfjs-dist/types/src/display/api";

export type Box = [number, number, number, number];
export type View = "side" | "old" | "new" | "overlay";
export type Side = "old" | "new";
export type ChangeKind = "replaced" | "inserted" | "deleted" | "pageInserted" | "pageDeleted";
export type PageStatus = "same" | "changed" | "added" | "removed";

export interface Change {
  id: number;
  page: number; // 0-based row
  kind: ChangeKind;
  leftBoxes: Box[];
  rightBoxes: Box[];
  oldText: string;
  newText: string;
}

export interface PageDiff {
  status: PageStatus;
  changes: Change[];
}

export interface PageTokens {
  tokens: string[];
  boxes: Box[];
}

export interface Run {
  li: number[];
  ri: number[];
}

export type KindFilter = Record<"replaced" | "inserted" | "deleted" | "pages", boolean>;
export type KindCounts = Record<ChangeKind, number>;

export const KIND_LABEL = {
  replaced: "cmpReplaced",
  inserted: "cmpInserted",
  deleted: "cmpDeleted",
  pageInserted: "cmpPageInserted",
  pageDeleted: "cmpPageDeleted",
} as const;

export const ALL_KINDS: KindFilter = { replaced: true, inserted: true, deleted: true, pages: true };

// ---------- token extraction ----------

export function itemBox(it: Pick<TextItem, "transform" | "width" | "height">): Box {
  const tx = it.transform;
  const x = tx[4];
  const y = tx[5];
  const w = it.width || 0;
  const h = it.height || Math.abs(tx[3]) || 8;
  return [x, y, x + w, y + h];
}

// Keeps the non-blank text items of a page as whitespace-normalised tokens
// alongside their PDF-space boxes.
export function tokensFromItems(items: ReadonlyArray<TextItem | TextMarkedContent>): PageTokens {
  const kept = items.filter((it): it is TextItem => "str" in it && it.str.trim().length > 0);
  return {
    tokens: kept.map((it) => it.str.replace(/\s+/g, " ").trim()),
    boxes: kept.map(itemBox),
  };
}

// ---------- diff ----------

// Above this many cell comparisons the LCS table is too big; fall back to a
// positional comparison.
export const LCS_LIMIT = 600000;

// Aligns two token lists by LCS and returns the runs of unmatched tokens
// between matches: both sides non-empty = replacement, one side = insert/delete.
export function diffRuns(a: string[], b: string[]): Run[] {
  const n = a.length;
  const m = b.length;
  const runs: Run[] = [];
  let cur: Run | null = null;
  const push = (side: "li" | "ri", idx: number) => {
    if (!cur) cur = { li: [], ri: [] };
    cur[side].push(idx);
  };
  const flush = () => {
    if (cur && (cur.li.length || cur.ri.length)) runs.push(cur);
    cur = null;
  };

  if (n * m > LCS_LIMIT) {
    const k = Math.min(n, m);
    for (let i = 0; i < k; i++) {
      if (a[i] === b[i]) flush();
      else {
        push("li", i);
        push("ri", i);
      }
    }
    for (let i = k; i < n; i++) push("li", i);
    for (let j = k; j < m; j++) push("ri", j);
    flush();
    return runs;
  }

  const dp: Int32Array[] = Array.from({ length: n + 1 }, () => new Int32Array(m + 1));
  for (let i = n - 1; i >= 0; i--)
    for (let j = m - 1; j >= 0; j--)
      dp[i][j] = a[i] === b[j] ? dp[i + 1][j + 1] + 1 : Math.max(dp[i + 1][j], dp[i][j + 1]);
  let i = 0;
  let j = 0;
  while (i < n || j < m) {
    if (i < n && j < m && a[i] === b[j]) {
      flush();
      i++;
      j++;
    } else if (j >= m || (i < n && dp[i + 1][j] >= dp[i][j + 1])) {
      push("li", i);
      i++;
    } else {
      push("ri", j);
      j++;
    }
  }
  flush();
  return runs;
}

export const runKind = (r: Run): ChangeKind =>
  r.li.length && r.ri.length ? "replaced" : r.li.length ? "deleted" : "inserted";

// Turns the runs of one page pair into Change records, numbering from firstId.
export function runsToChanges(
  runs: Run[],
  L: PageTokens,
  R: PageTokens,
  page: number,
  firstId: number,
): Change[] {
  return runs.map((r, k) => ({
    id: firstId + k,
    page,
    kind: runKind(r),
    leftBoxes: r.li.map((i) => L.boxes[i]),
    rightBoxes: r.ri.map((i) => R.boxes[i]),
    oldText: r.li.map((i) => L.tokens[i]).join(" "),
    newText: r.ri.map((i) => R.tokens[i]).join(" "),
  }));
}

export function pageChange(kind: "pageInserted" | "pageDeleted", page: number, id: number): Change {
  return { id, page, kind, leftBoxes: [], rightBoxes: [], oldText: "", newText: "" };
}

export function diffPage(L: PageTokens, R: PageTokens, page: number, firstId: number): PageDiff {
  const changes = runsToChanges(diffRuns(L.tokens, R.tokens), L, R, page, firstId);
  return { status: changes.length ? "changed" : "same", changes };
}

// Compares two documents page by page. `tokensFor` supplies a page's tokens
// (1-based page number); `alive` lets a caller abandon a stale run, in which
// case null is returned.
export async function compareDocs(
  leftPages: number,
  rightPages: number,
  tokensFor: (side: Side, page: number) => Promise<PageTokens>,
  alive: () => boolean = () => true,
): Promise<PageDiff[] | null> {
  const pages = Math.max(leftPages, rightPages);
  const out: PageDiff[] = [];
  let id = 0;
  for (let i = 0; i < pages; i++) {
    if (!alive()) return null;
    if (i >= leftPages) {
      out.push({ status: "added", changes: [pageChange("pageInserted", i, id++)] });
      continue;
    }
    if (i >= rightPages) {
      out.push({ status: "removed", changes: [pageChange("pageDeleted", i, id++)] });
      continue;
    }
    const [L, R] = await Promise.all([tokensFor("old", i + 1), tokensFor("new", i + 1)]);
    const d = diffPage(L, R, i, id);
    id += d.changes.length;
    out.push(d);
  }
  return alive() ? out : null;
}

// ---------- reference helpers ----------

export const isPageChange = (c: Change) => c.kind === "pageInserted" || c.kind === "pageDeleted";

// Whether a change lives on the new file (insertions) rather than the old one.
export const onNewSide = (c: Change) => c.kind === "inserted" || c.kind === "pageInserted";
// Whether a change lives only on the old file (deletions).
export const onOldSide = (c: Change) => c.kind === "deleted" || c.kind === "pageDeleted";

export const boxesFor = (c: Change, side: Side): Box[] => (side === "old" ? c.leftBoxes : c.rightBoxes);

// Whether a side draws this change: the old file shows deletions and
// replacements, the new file insertions and replacements.
export const sideShows = (c: Change, side: Side) =>
  c.kind === "replaced" || (side === "old" ? c.kind === "deleted" : c.kind === "inserted");

export const passesFilter = (c: Change, f: KindFilter) =>
  c.kind === "replaced"
    ? f.replaced
    : c.kind === "inserted"
      ? f.inserted
      : c.kind === "deleted"
        ? f.deleted
        : f.pages;

export function countKinds(changes: Change[]): KindCounts {
  const c: KindCounts = { replaced: 0, inserted: 0, deleted: 0, pageInserted: 0, pageDeleted: 0 };
  for (const ch of changes) c[ch.kind]++;
  return c;
}

// Groups changes by page, keeping only those whose text contains the query.
export function groupByPage(changes: Change[], query = ""): [number, Change[]][] {
  const q = query.trim().toLowerCase();
  const m = new Map<number, Change[]>();
  for (const c of changes) {
    if (q && !(c.oldText.toLowerCase().includes(q) || c.newText.toLowerCase().includes(q))) continue;
    const arr = m.get(c.page) ?? [];
    arr.push(c);
    m.set(c.page, arr);
  }
  return [...m.entries()];
}

// Wraps an index into [0, n).
export const wrapIndex = (i: number, n: number) => (n ? ((i % n) + n) % n : -1);

// The banner a side shows for a whole-page change, if any.
export function pageBannerFor(
  status: PageStatus | undefined,
  side: Side,
): "cmpPageInserted" | "cmpPageDeleted" | null {
  if (status === "added" && side === "new") return "cmpPageInserted";
  if (status === "removed" && side === "old") return "cmpPageDeleted";
  return null;
}

export const kindGlyph = (kind: ChangeKind) =>
  kind === "inserted" || kind === "pageInserted"
    ? "+"
    : kind === "deleted" || kind === "pageDeleted"
      ? "×"
      : "⟳";
