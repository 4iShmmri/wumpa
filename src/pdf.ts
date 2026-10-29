import * as pdfjs from "pdfjs-dist";
import type { PDFDocumentProxy, PDFPageProxy } from "pdfjs-dist";
import type { TextContent, TextItem } from "pdfjs-dist/types/src/display/api";
import workerUrl from "pdfjs-dist/build/pdf.worker.min.mjs?url";
import { loadPdfBytes, baseName, passwordStatus, passwordAttempt } from "./backend";
import { t } from "./i18n";
import { flowError } from "./errors";
import { useStore } from "./store";
import type { PageSpec } from "./types";
import { extractTextInRect, type ExtractedText } from "./textExtract";

pdfjs.GlobalWorkerOptions.workerSrc = workerUrl;

export interface PageDims {
  w: number; // CSS px at scale 1, including the page's own /Rotate
  h: number;
  rotate: number; // the page's own /Rotate
}

export interface DocEntry {
  doc: PDFDocumentProxy;
  version: number;
  numPages: number;
  dims: PageDims[];
  pageCache: Map<number, PDFPageProxy>;
  textCache: Map<number, string>;
  annotsCache: Map<number, ExistingAnnot[]>;
  linksCache: Map<number, ExistingLink[]>;
}

export interface ExistingLink {
  rect: [number, number, number, number]; // normalized PDF coords
  url: string | null;
  destSrc: number | null; // 1-based source page a GoTo link targets
}

export interface ExistingAnnot {
  subtype: "Highlight" | "Text";
  rect: [number, number, number, number]; // normalized PDF coords
  quads: [number, number, number, number][]; // [minX, minY, maxX, maxY] each
  contents: string;
  author: string;
  hasAppearance: boolean;
  cssColor: string;
}

const cache = new Map<string, DocEntry>();
// Bumped on every load start and eviction so a load that finishes late can
// tell it lost the race and destroy its document instead of leaking it.
const generation = new Map<string, number>();

export const pageRotation = (base: number, delta: number) => (((base + delta) % 360) + 360) % 360;

export const docPath = (t: { path: string; previewPath: string | null }) => t.previewPath ?? t.path;

export function specBase(entry: Pick<DocEntry, "dims"> | null, spec: PageSpec): PageDims | null {
  if (spec.src <= 0) {
    return {
      w: spec.w && spec.w > 0 ? spec.w : 612,
      h: spec.h && spec.h > 0 ? spec.h : 792,
      rotate: 0,
    };
  }
  return entry?.dims[spec.src - 1] ?? null;
}

const pending = new Map<string, Promise<DocEntry>>();

// `eager` measures every page before resolving (the compare view lays out all
// pages at once); tabs measure the first pages and the rest in the background.
export function getDoc(tabId: string, path: string, version: number, eager = false): Promise<DocEntry> {
  const existing = cache.get(tabId);
  if (existing && existing.version === version) return Promise.resolve(existing);
  const key = `${tabId}:${version}:${path}`;
  let p = pending.get(key);
  if (!p) {
    p = loadDoc(tabId, path, version, eager).finally(() => pending.delete(key));
    pending.set(key, p);
  }
  return p;
}

const EAGER_PAGES = 40;

async function loadDoc(tabId: string, path: string, version: number, eager: boolean): Promise<DocEntry> {
  evictDoc(tabId);
  const gen = (generation.get(tabId) ?? 0) + 1;
  generation.set(tabId, gen);

  const buf = await loadPdfBytes(path);
  const task = pdfjs.getDocument({
    data: new Uint8Array(buf),
    cMapUrl: new URL("pdfjs/cmaps/", document.baseURI).href,
    cMapPacked: true,
    standardFontDataUrl: new URL("pdfjs/standard_fonts/", document.baseURI).href,
  });
  let needsPassword = false;
  let wiped = false;
  task.onPassword = (updatePassword: (pw: string) => void, reason: number) => {
    needsPassword = true;
    void (async () => {
      const wrong = reason === 2;
      let status = wrong ? await passwordAttempt(path, false) : await passwordStatus(path);
      if (status?.wiped) {
        wiped = true;
        useStore.getState().setStatus(t("stFileWiped", { name: baseName(path) }));
        void task.destroy();
        return;
      }
      if (wrong && status) status = (await passwordStatus(path)) ?? status;
      const pw = await useStore.getState().askPassword(baseName(path), wrong, status);
      if (pw === null) void task.destroy();
      else updatePassword(pw);
    })();
  };
  let doc: PDFDocumentProxy;
  try {
    doc = await task.promise;
  } catch (e) {
    throw wiped ? flowError("FileWiped") : e;
  }
  if (needsPassword) void passwordAttempt(path, true);

  const pageCache = new Map<number, PDFPageProxy>();
  const measure = async (from: number, to: number, dims: PageDims[]) => {
    const pages = await Promise.all(Array.from({ length: to - from }, (_, i) => doc.getPage(from + i + 1)));
    pages.forEach((page, i) => {
      pageCache.set(from + i + 1, page);
      const vp = page.getViewport({ scale: 1 });
      dims[from + i] = { w: vp.width, h: vp.height, rotate: page.rotate };
    });
  };
  const total = doc.numPages;
  const head = eager ? total : Math.min(total, EAGER_PAGES);
  const dims: PageDims[] = [];
  await measure(0, head, dims);

  if (generation.get(tabId) !== gen) {
    doc.loadingTask.destroy().catch(() => {});
    throw flowError("LoadCancelled");
  }
  if (head < total) {
    // Unmeasured pages borrow the last known size until their real one arrives.
    const guess = dims[head - 1] ?? { w: 612, h: 792, rotate: 0 };
    for (let i = head; i < total; i++) dims[i] = guess;
    void (async () => {
      for (let from = head; from < total; from += 50) {
        if (generation.get(tabId) !== gen) return;
        await measure(from, Math.min(total, from + 50), dims);
      }
      if (generation.get(tabId) === gen) useStore.getState().patchTab(tabId, { layoutNonce: Date.now() });
    })().catch(() => {});
  }
  const entry: DocEntry = {
    doc,
    version,
    numPages: doc.numPages,
    dims,
    pageCache,
    textCache: new Map(),
    annotsCache: new Map(),
    linksCache: new Map(),
  };
  cache.set(tabId, entry);
  return entry;
}

export async function getPage(entry: DocEntry, src: number): Promise<PDFPageProxy> {
  let p = entry.pageCache.get(src);
  if (!p) {
    p = await entry.doc.getPage(src);
    entry.pageCache.set(src, p);
  }
  return p;
}

// Text items and styles of a page. pdf.js's own getTextContent iterates
// its stream with `for await`, which WebKit does not support, so the
// stream is read by hand.
export async function readTextContent(page: PDFPageProxy): Promise<TextContent> {
  const reader = page.streamTextContent().getReader();
  const content: TextContent = { items: [], styles: {}, lang: null };
  for (;;) {
    const { value, done } = await reader.read();
    if (done) break;
    content.items.push(...value.items);
    Object.assign(content.styles, value.styles);
    content.lang ??= value.lang ?? null;
  }
  return content;
}

export async function getPageText(entry: DocEntry, src: number): Promise<string> {
  let t = entry.textCache.get(src);
  if (t === undefined) {
    const page = await getPage(entry, src);
    const content = await readTextContent(page);
    t = content.items.map((it) => ("str" in it ? it.str : "")).join(" ");
    entry.textCache.set(src, t);
  }
  return t;
}

function normRect(r: number[]): [number, number, number, number] {
  return [Math.min(r[0], r[2]), Math.min(r[1], r[3]), Math.max(r[0], r[2]), Math.max(r[1], r[3])];
}

// quadPoints: flat array of x,y pairs (8 numbers = 1 quad), or legacy
// array-of-point-objects groups, or absent.
function normQuads(qp: unknown, fallback: [number, number, number, number]) {
  const quads: [number, number, number, number][] = [];
  if (qp && typeof (qp as { length?: number }).length === "number") {
    const arr = qp as ArrayLike<unknown>;
    if (arr.length > 0 && typeof arr[0] === "number") {
      const flat = arr as ArrayLike<number>;
      for (let i = 0; i + 7 < flat.length; i += 8) {
        const xs = [flat[i], flat[i + 2], flat[i + 4], flat[i + 6]];
        const ys = [flat[i + 1], flat[i + 3], flat[i + 5], flat[i + 7]];
        quads.push([Math.min(...xs), Math.min(...ys), Math.max(...xs), Math.max(...ys)]);
      }
    } else {
      for (let i = 0; i < arr.length; i++) {
        const group = arr[i];
        const pts = (Array.isArray(group) ? group : [group]) as { x: number; y: number }[];
        const xs = pts.map((p) => p?.x).filter((v) => typeof v === "number");
        const ys = pts.map((p) => p?.y).filter((v) => typeof v === "number");
        if (xs.length && ys.length) {
          quads.push([Math.min(...xs), Math.min(...ys), Math.max(...xs), Math.max(...ys)]);
        }
      }
    }
  }
  return quads.length ? quads : [fallback];
}

export async function getTextInRect(
  entry: DocEntry,
  src: number,
  bbox: [number, number, number, number],
): Promise<ExtractedText> {
  const page = await getPage(entry, src);
  const content = await readTextContent(page);
  const items = content.items.filter((it): it is TextItem => "str" in it);
  return extractTextInRect(items, content.styles ?? {}, bbox);
}

export async function getAnnots(entry: DocEntry, src: number): Promise<ExistingAnnot[]> {
  const cached = entry.annotsCache.get(src);
  if (cached) return cached;
  const page = await getPage(entry, src);
  // eslint-disable-next-line @typescript-eslint/no-explicit-any
  const raw: any[] = await page.getAnnotations();
  const out: ExistingAnnot[] = [];
  for (const a of raw) {
    if (a?.subtype !== "Highlight" && a?.subtype !== "Text") continue;
    if (!Array.isArray(a.rect) || a.rect.length < 4) continue;
    const rect = normRect(a.rect);
    const c = a.color;
    out.push({
      subtype: a.subtype,
      rect,
      quads: a.subtype === "Highlight" ? normQuads(a.quadPoints, rect) : [rect],
      contents: a.contentsObj?.str ?? "",
      author: a.titleObj?.str ?? "",
      hasAppearance: !!a.hasAppearance,
      cssColor:
        c && typeof c.length === "number" && c.length >= 3
          ? `rgb(${c[0]},${c[1]},${c[2]})`
          : "rgb(255,200,0)",
    });
  }
  entry.annotsCache.set(src, out);
  return out;
}

export async function getLinks(entry: DocEntry, src: number): Promise<ExistingLink[]> {
  const cached = entry.linksCache.get(src);
  if (cached) return cached;
  const page = await getPage(entry, src);
  // eslint-disable-next-line @typescript-eslint/no-explicit-any
  const raw: any[] = await page.getAnnotations();
  const out: ExistingLink[] = [];
  for (const a of raw) {
    if (a?.subtype !== "Link") continue;
    if (!Array.isArray(a.rect) || a.rect.length < 4) continue;
    const rect = normRect(a.rect);
    let url: string | null = a.url ?? a.unsafeUrl ?? null;
    let destSrc: number | null = null;
    if (!url && a.dest != null) {
      try {
        const dest = typeof a.dest === "string" ? await entry.doc.getDestination(a.dest) : a.dest;
        if (Array.isArray(dest) && dest[0]) destSrc = (await entry.doc.getPageIndex(dest[0])) + 1;
      } catch {
        /* unresolved destination */
      }
    }
    if (typeof url === "string" && url.length === 0) url = null;
    if (url || destSrc) out.push({ rect, url, destSrc });
  }
  entry.linksCache.set(src, out);
  return out;
}

export function evictDoc(tabId: string) {
  generation.set(tabId, (generation.get(tabId) ?? 0) + 1);
  const e = cache.get(tabId);
  if (e) {
    e.doc.loadingTask.destroy().catch(() => {});
    cache.delete(tabId);
  }
}

export { pdfjs };
