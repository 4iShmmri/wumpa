import * as pdfjs from "pdfjs-dist";
import type { PDFDocumentProxy, PDFPageProxy } from "pdfjs-dist";
import workerUrl from "pdfjs-dist/build/pdf.worker.min.mjs?url";
import { loadPdfBytes } from "./backend";

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

export const docPath = (t: { path: string; previewPath: string | null }) =>
  t.previewPath ?? t.path;

export async function getDoc(tabId: string, path: string, version: number): Promise<DocEntry> {
  const existing = cache.get(tabId);
  if (existing && existing.version === version) return existing;
  if (existing) existing.doc.loadingTask.destroy().catch(() => {});

  const buf = await loadPdfBytes(path);
  const doc = await pdfjs.getDocument({
    data: new Uint8Array(buf),
    cMapUrl: new URL("pdfjs/cmaps/", document.baseURI).href,
    cMapPacked: true,
    standardFontDataUrl: new URL("pdfjs/standard_fonts/", document.baseURI).href,
  }).promise;

  const dims: PageDims[] = [];
  for (let i = 1; i <= doc.numPages; i++) {
    const page = await doc.getPage(i);
    const vp = page.getViewport({ scale: 1 });
    dims.push({ w: vp.width, h: vp.height, rotate: page.rotate });
  }

  const entry: DocEntry = {
    doc,
    version,
    numPages: doc.numPages,
    dims,
    pageCache: new Map(),
    textCache: new Map(),
    annotsCache: new Map(),
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

export async function getPageText(entry: DocEntry, src: number): Promise<string> {
  let t = entry.textCache.get(src);
  if (t === undefined) {
    const page = await getPage(entry, src);
    const content = await page.getTextContent();
    t = content.items.map((it) => ("str" in it ? it.str : "")).join(" ");
    entry.textCache.set(src, t);
  }
  return t;
}

function normRect(r: number[]): [number, number, number, number] {
  return [
    Math.min(r[0], r[2]),
    Math.min(r[1], r[3]),
    Math.max(r[0], r[2]),
    Math.max(r[1], r[3]),
  ];
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
): Promise<{ text: string; size: number }> {
  const page = await getPage(entry, src);
  const content = await page.getTextContent();
  const grow = 2;
  const [bx1, by1, bx2, by2] = [bbox[0] - grow, bbox[1] - grow, bbox[2] + grow, bbox[3] + grow];

  interface It { str: string; tx: number; ty: number; w: number; h: number; fs: number }
  const kept: It[] = [];
  for (const it of content.items) {
    if (!("str" in it) || !it.str) continue;
    const tx = it.transform[4];
    const ty = it.transform[5];
    const w = it.width || 0;
    const h = it.height || Math.abs(it.transform[3]) || 0;
    const cx = tx + w / 2;
    const cy = ty + h / 2;
    if (cx < bx1 || cx > bx2 || cy < by1 || cy > by2) continue;
    kept.push({ str: it.str, tx, ty, w, h, fs: Math.abs(it.transform[3]) });
  }
  if (!kept.length) return { text: "", size: 0 };

  const lines: It[][] = [];
  for (const k of [...kept].sort((a, b) => b.ty - a.ty || a.tx - b.tx)) {
    const line = lines.find((l) => Math.abs(l[0].ty - k.ty) < 0.45 * Math.max(k.h, l[0].h, 1));
    if (line) line.push(k);
    else lines.push([k]);
  }

  const fss = kept.map((k) => k.fs).filter((v) => v > 0).sort((a, b) => a - b);
  const median = fss.length ? fss[Math.floor(fss.length / 2)] : 0;
  const fallback = (bbox[3] - bbox[1]) / Math.max(1, lines.length) / 1.35;
  const size = Math.max(6, Math.min(120, median || fallback));

  const text = lines
    .map((l) => {
      l.sort((a, b) => a.tx - b.tx);
      let out = "";
      let prevEnd: number | null = null;
      for (const k of l) {
        if (prevEnd != null && k.tx - prevEnd > 0.3 * size && out && !out.endsWith(" ")) out += " ";
        out += k.str;
        prevEnd = k.tx + k.w;
      }
      return out;
    })
    .join("\n");
  return { text, size };
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

export function evictDoc(tabId: string) {
  const e = cache.get(tabId);
  if (e) {
    e.doc.loadingTask.destroy().catch(() => {});
    cache.delete(tabId);
  }
}

export { pdfjs };
