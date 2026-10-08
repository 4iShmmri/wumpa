import type { PDFDocumentProxy } from "pdfjs-dist";
import type { Bookmark, LinkItem, LinkRemoval, PageSpec } from "./types";
import type { WireOutline, WireLink, WireLinkRemoval } from "./backend";

let uid = 0;
const nextId = () => `bm${Date.now().toString(36)}${(uid++).toString(36)}`;

// 0-based index of a source page in the output order; null if dropped.
export function srcToOut(pages: PageSpec[], src: number): number | null {
  const i = pages.findIndex((p) => p.src === src);
  return i >= 0 ? i : null;
}

// eslint-disable-next-line @typescript-eslint/no-explicit-any
async function destToTarget(doc: PDFDocumentProxy, raw: any): Promise<{ src: number; top: number | null } | null> {
  try {
    const dest = typeof raw === "string" ? await doc.getDestination(raw) : raw;
    if (!Array.isArray(dest) || !dest.length) return null;
    const ref = dest[0];
    const idx = await doc.getPageIndex(ref);
    const top = dest[1]?.name === "XYZ" && typeof dest[3] === "number" ? dest[3] : null;
    return { src: idx + 1, top };
  } catch {
    return null;
  }
}

// eslint-disable-next-line @typescript-eslint/no-explicit-any
async function convert(doc: PDFDocumentProxy, items: any[]): Promise<Bookmark[]> {
  const out: Bookmark[] = [];
  for (const it of items) {
    const target = it.dest ? await destToTarget(doc, it.dest) : null;
    out.push({
      id: nextId(),
      title: it.title ?? "",
      src: target?.src ?? 0,
      top: target?.top ?? null,
      url: it.url ?? null,
      open: it.items?.length ? true : false,
      children: it.items?.length ? await convert(doc, it.items) : [],
    });
  }
  return out;
}

export async function loadBookmarks(doc: PDFDocumentProxy): Promise<Bookmark[]> {
  try {
    const items = await doc.getOutline();
    if (!items || !items.length) return [];
    return await convert(doc, items);
  } catch {
    return [];
  }
}

function bmToWire(bm: Bookmark, pages: PageSpec[]): WireOutline {
  const url = bm.url && bm.url.length ? bm.url : null;
  return {
    title: bm.title,
    page: url ? null : bm.src > 0 ? srcToOut(pages, bm.src) : null,
    top: url ? null : bm.top ?? null,
    url,
    open: !!bm.open,
    children: bm.children.map((c) => bmToWire(c, pages)),
  };
}

export const outlineToWire = (bookmarks: Bookmark[], pages: PageSpec[]): WireOutline[] =>
  bookmarks.map((b) => bmToWire(b, pages));

export function linksToWire(links: LinkItem[], pages: PageSpec[]): WireLink[] {
  const out: WireLink[] = [];
  for (const l of links) {
    const page = srcToOut(pages, l.src);
    if (page === null) continue;
    const url = l.url && l.url.length ? l.url : null;
    const destPage = l.destSrc ? srcToOut(pages, l.destSrc) : null;
    if (!url && destPage === null) continue;
    out.push({ page, rect: l.rect, url, destPage });
  }
  return out;
}

export function linkRemovalsToWire(removals: LinkRemoval[], pages: PageSpec[]): WireLinkRemoval[] {
  const out: WireLinkRemoval[] = [];
  for (const r of removals) {
    const page = srcToOut(pages, r.src);
    if (page === null) continue;
    out.push({ page, rect: r.rect });
  }
  return out;
}

export const newBookmark = (title: string, src: number, top: number | null): Bookmark => ({
  id: nextId(),
  title,
  src,
  top,
  url: null,
  open: true,
  children: [],
});
