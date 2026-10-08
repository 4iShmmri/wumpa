import { useStore, getActiveTab } from "./store";
import type { Tab, Note, ObjectEdit, StampItem, ShapeItem, Bookmark, LinkItem, Rect } from "./types";
import {
  pickPdfs, pickSavePath, pickSavePathWithExt, pickDirectory, saveDocument,
  mergeDocuments, splitDocument, baseName, dirName, confirmDialog,
  printDocument, savePng, inTauri, compressPdf, createBlankPdf,
  readImageB64, pickImagePath, type SaveExtras,
} from "./backend";
import { getDoc, getPage, getPageText, docPath, specBase } from "./pdf";
import {
  loadBookmarks, outlineToWire, linksToWire, linkRemovalsToWire, newBookmark,
} from "./outline";
import { report } from "./errors";
import { measureText, rasterizeStamps, addTextStampAt, addImageStamp, loadImage } from "./stamps";
import { t } from "./i18n";

const ARABIC_RE = /[؀-ۿݐ-ݿࢠ-ࣿﭐ-﷿ﹰ-﻿]/g;

async function detectTabLang(
  tabId: string,
  entry: Awaited<ReturnType<typeof getDoc>>,
) {
  try {
    let arabic = 0;
    let letters = 0;
    for (let p = 1; p <= Math.min(3, entry.numPages); p++) {
      const text = (await getPageText(entry, p)).slice(0, 6000);
      letters += (text.match(/\p{L}/gu) ?? []).length;
      arabic += (text.match(ARABIC_RE) ?? []).length;
    }
    const lang = letters > 0 && arabic / letters > 0.25 ? "ar" : "en";
    S().patchTab(tabId, { detectedLang: lang });
  } catch {
    /* detection is best-effort */
  }
}

const S = useStore.getState;

export interface RecentFile {
  path: string;
  name: string;
  time: number;
}

export function getRecentFiles(): RecentFile[] {
  try {
    const v = JSON.parse(localStorage.getItem("recentFiles") ?? "[]");
    return Array.isArray(v) ? v.filter((r) => r && typeof r.path === "string") : [];
  } catch {
    return [];
  }
}

function pushRecent(path: string, name: string) {
  try {
    const list = [
      { path, name, time: Date.now() },
      ...getRecentFiles().filter((r) => r.path !== path),
    ].slice(0, 10);
    localStorage.setItem("recentFiles", JSON.stringify(list));
  } catch {
    /* best effort */
  }
}

export function dropRecent(path: string) {
  try {
    localStorage.setItem(
      "recentFiles",
      JSON.stringify(getRecentFiles().filter((r) => r.path !== path)),
    );
  } catch {
    /* best effort */
  }
}

export async function openPaths(paths: string[]) {
  for (const path of paths) {
    const existing = S().tabs.find((t) => t.path === path);
    if (existing) {
      S().setActive(existing.id);
      continue;
    }
    const id = crypto.randomUUID();
    try {
      const entry = await getDoc(id, path, 0);
      const tab: Tab = {
        id, path, name: baseName(path),
        pages: Array.from({ length: entry.numPages }, (_, i) => ({ src: i + 1, rotation: 0 })),
        highlights: [], notes: [], edits: [], stamps: [], shapes: [],
        bookmarks: null, links: [], linkRemovals: [],
        previewPath: null, past: [], future: [],
        zoom: 1, currentPage: 1, selected: [],
        dirty: false, docVersion: 0,
      };
      S().addTab(tab);
      if (inTauri) pushRecent(path, tab.name);
      void detectTabLang(id, entry);
      void loadBookmarks(entry.doc).then((bms) => S().patchTab(id, { bookmarks: bms }));
    } catch (e) {
      report(t("stCouldNotOpen", { name: baseName(path) }), e);
    }
  }
}

export async function openFileDialog() {
  const paths = await pickPdfs(true);
  if (paths) await openPaths(paths);
}

export async function saveActive(saveAs: boolean) {
  const tab = getActiveTab();
  if (!tab) return;
  let outPath = tab.path;
  if (saveAs) {
    const picked = await pickSavePath(tab.name);
    if (!picked) return;
    outPath = picked.endsWith(".pdf") ? picked : picked + ".pdf";
  }
  try {
    const stamps = await rasterizeStamps(tab.stamps);
    await saveDocument(
      tab.path, outPath, tab.pages, tab.highlights,
      tab.notes.map(({ src, x, y, text }) => ({ src, x, y, text })),
      tab.edits,
      stamps,
      tab.shapes.map((sh) => ({
        src: sh.src, kind: sh.kind, rect: sh.rect, strokes: sh.strokes,
        color: sh.color, width: sh.width, opacity: sh.opacity,
      })),
      saveExtras(tab),
    );
    S().markSaved(tab.id, outPath, baseName(outPath));
    S().setStatus(t("stSaved", { name: baseName(outPath) }));
    void refreshBookmarks(tab.id);
  } catch (e) {
    report(t("stSaveFailed"), e);
  }
}

function saveExtras(tab: Tab): SaveExtras {
  return {
    outline: tab.bookmarks ? outlineToWire(tab.bookmarks, tab.pages) : null,
    links: linksToWire(tab.links, tab.pages),
    linkRemovals: linkRemovalsToWire(tab.linkRemovals, tab.pages),
  };
}

async function refreshBookmarks(tabId: string) {
  const tab = S().tabs.find((t) => t.id === tabId);
  if (!tab) return;
  try {
    const entry = await getDoc(tabId, docPath(tab), tab.docVersion);
    const bms = await loadBookmarks(entry.doc);
    S().patchTab(tabId, { bookmarks: bms });
  } catch {
    /* best effort */
  }
}

export async function closeTabSafe(id: string) {
  const tab = S().tabs.find((t) => t.id === id);
  if (!tab) return;
  if (tab.dirty) {
    const ok = await confirmDialog(t("cfCloseUnsaved", { name: tab.name }));
    if (!ok) return;
  }
  S().closeTab(id);
}

export async function mergeFiles() {
  const paths = await pickPdfs(true);
  if (!paths || paths.length < 2) {
    if (paths) S().setStatus(t("stPickTwo"));
    return;
  }
  const out = await pickSavePath("merged.pdf");
  if (!out) return;
  const outPath = out.endsWith(".pdf") ? out : out + ".pdf";
  try {
    await mergeDocuments(paths, outPath);
    S().setStatus(t("stMerged", { n: paths.length }));
    await openPaths([outPath]);
  } catch (e) {
    report(t("stMergeFailed"), e);
  }
}

export async function extractSelected() {
  const tab = getActiveTab();
  if (!tab) return;
  const indices = tab.selected.length ? [...tab.selected].sort((a, b) => a - b) : [tab.currentPage - 1];
  const specs = indices.map((i) => tab.pages[i]).filter(Boolean);
  if (!specs.length) return;
  const out = await pickSavePath(tab.name.replace(/\.pdf$/i, "") + "-extract.pdf");
  if (!out) return;
  const outPath = out.endsWith(".pdf") ? out : out + ".pdf";
  try {
    await saveDocument(tab.path, outPath, specs, [], [], tab.edits, [], []);
    S().setStatus(t("stExtracted", { n: specs.length }));
    await openPaths([outPath]);
  } catch (e) {
    report(t("stExtractFailed"), e);
  }
}

export async function splitEvery(n: number) {
  const tab = getActiveTab();
  if (!tab || n < 1) return;
  const dir = await pickDirectory();
  if (!dir) return;
  const base = tab.name.replace(/\.pdf$/i, "");
  const parts: { pages: number[]; out_path: string }[] = [];
  for (let i = 0; i < tab.pages.length; i += n) {
    const chunk = tab.pages.slice(i, i + n).map((p) => p.src);
    parts.push({ pages: chunk, out_path: `${dir}/${base}-part${parts.length + 1}.pdf` });
  }
  try {
    await splitDocument(tab.path, parts);
    S().setStatus(t("stSplitDone", { n: parts.length, dir: baseName(dirName(parts[0].out_path)) }));
  } catch (e) {
    report(t("stSplitFailed"), e);
  }
}

export function rotateSelection(delta: number) {
  const tab = getActiveTab();
  if (!tab) return;
  const indices = tab.selected.length ? tab.selected : [tab.currentPage - 1];
  S().mutateDoc(tab.id, (t) => ({
    pages: t.pages.map((p, i) =>
      indices.includes(i) ? { ...p, rotation: (p.rotation + delta + 360) % 360 } : p,
    ),
  }));
}

export function deleteSelection() {
  const tab = getActiveTab();
  if (!tab) return;
  const indices = tab.selected.length ? tab.selected : [tab.currentPage - 1];
  if (indices.length >= tab.pages.length) {
    S().setStatus(t("stCannotDeleteAll"));
    return;
  }
  S().mutateDoc(tab.id, (t) => ({
    pages: t.pages.filter((_, i) => !indices.includes(i)),
    selected: [],
    currentPage: Math.min(t.currentPage, t.pages.length - indices.length),
  }));
}

export async function insertBlankPage(tabId: string, atIndex: number) {
  const tab = S().tabs.find((t) => t.id === tabId);
  if (!tab) return;
  let w = 612;
  let h = 792;
  const ref = tab.pages[atIndex - 1] ?? tab.pages[atIndex] ?? tab.pages[0];
  if (ref) {
    try {
      const entry = ref.src === 0 ? null : await getDoc(tab.id, docPath(tab), tab.docVersion);
      const d = specBase(entry, ref);
      if (d) {
        const swap = ref.rotation % 180 !== 0;
        w = swap ? d.h : d.w;
        h = swap ? d.w : d.h;
      }
    } catch {
      /* default size */
    }
  }
  S().mutateDoc(tabId, (t) => {
    const pages = [...t.pages];
    pages.splice(atIndex, 0, { src: 0, rotation: 0, w, h });
    return { pages, selected: [atIndex] };
  });
}

export function duplicatePage(tabId: string, index: number) {
  S().mutateDoc(tabId, (t) => {
    const src = t.pages[index];
    if (!src) return {};
    const pages = [...t.pages];
    pages.splice(index + 1, 0, { ...src });
    return { pages, selected: [index + 1] };
  });
}

export async function exportPageAsPng(tabId: string, index: number) {
  const tab = S().tabs.find((t) => t.id === tabId);
  if (!tab) return;
  const spec = tab.pages[index];
  if (!spec) return;
  const out = await pickSavePathWithExt(
    `${tab.name.replace(/\.pdf$/i, "")}-page${index + 1}.png`,
    "png",
    "PNG image",
  );
  if (!out) return;
  const path = out.toLowerCase().endsWith(".png") ? out : out + ".png";
  try {
    const canvas = document.createElement("canvas");
    if (spec.src === 0) {
      const d = specBase(null, spec)!;
      const swap = spec.rotation % 180 !== 0;
      canvas.width = Math.round((swap ? d.h : d.w) * 2);
      canvas.height = Math.round((swap ? d.w : d.h) * 2);
      const ctx = canvas.getContext("2d")!;
      ctx.fillStyle = "#fff";
      ctx.fillRect(0, 0, canvas.width, canvas.height);
    } else {
      const entry = await getDoc(tab.id, docPath(tab), tab.docVersion);
      const page = await getPage(entry, spec.src);
      const rotation = (page.rotate + spec.rotation + 360) % 360;
      const vp = page.getViewport({ scale: 2, rotation });
      canvas.width = Math.floor(vp.width);
      canvas.height = Math.floor(vp.height);
      const ctx = canvas.getContext("2d")!;
      ctx.fillStyle = "#fff";
      ctx.fillRect(0, 0, canvas.width, canvas.height);
      await page.render({ canvas, viewport: vp }).promise;
    }
    const dataB64 = canvas.toDataURL("image/png").split(",")[1];
    await savePng(path, dataB64);
    S().setStatus(t("stExportedPng", { n: index + 1 }));
  } catch (e) {
    report(t("stPngFailed"), e);
  }
}

export async function printActive() {
  const tab = getActiveTab();
  if (!tab) return;
  try {
    const stamps = await rasterizeStamps(tab.stamps);
    await printDocument(
      tab.path, tab.pages, tab.highlights,
      tab.notes.map(({ src, x, y, text }) => ({ src, x, y, text })),
      tab.edits,
      stamps,
      tab.shapes.map((sh) => ({
        src: sh.src, kind: sh.kind, rect: sh.rect, strokes: sh.strokes,
        color: sh.color, width: sh.width, opacity: sh.opacity,
      })),
    );
    S().setStatus(t("stPrintOpening"));
  } catch (e) {
    report(t("stPrintFailed"), e);
  }
}

export function movePage(from: number, to: number) {
  const tab = getActiveTab();
  if (!tab || from === to) return;
  S().mutateDoc(tab.id, (t) => {
    const pages = [...t.pages];
    const [moved] = pages.splice(from, 1);
    pages.splice(to, 0, moved);
    return { pages, selected: [to] };
  });
}

export function addNote(tabId: string, note: Note) {
  S().mutateDoc(tabId, (t) => ({ notes: [...t.notes, note] }));
}

export function updateNote(tabId: string, id: string, text: string) {
  S().mutateDoc(tabId, (t) => ({
    notes: t.notes.map((n) => (n.id === id ? { ...n, text } : n)),
  }));
}

export function deleteNote(tabId: string, id: string) {
  S().mutateDoc(tabId, (t) => ({ notes: t.notes.filter((n) => n.id !== id) }));
}

export function addObjectEdits(tabId: string, newEdits: ObjectEdit[]) {
  S().mutateDoc(tabId, (t) => {
    const edits = [...t.edits];
    for (const e of newEdits) {
      const last = edits[edits.length - 1];
      if (
        e.action === "translate" && last && last.action === "translate" &&
        last.id === e.id && last.src === e.src
      ) {
        edits[edits.length - 1] = {
          ...last,
          dx: (last.dx ?? 0) + (e.dx ?? 0),
          dy: (last.dy ?? 0) + (e.dy ?? 0),
        };
      } else {
        edits.push(e);
      }
    }
    return { edits };
  });
}

export function addShape(tabId: string, shape: ShapeItem) {
  S().mutateDoc(tabId, (t) => ({ shapes: [...t.shapes, shape] }));
}

export function moveShape(tabId: string, id: string, dx: number, dy: number) {
  S().mutateDoc(tabId, (t) => ({
    shapes: t.shapes.map((sh) =>
      sh.id === id
        ? {
            ...sh,
            rect: [sh.rect[0] + dx, sh.rect[1] + dy, sh.rect[2] + dx, sh.rect[3] + dy] as [
              number, number, number, number,
            ],
            strokes: sh.strokes.map((st) => st.map(([x, y]) => [x + dx, y + dy] as [number, number])),
          }
        : sh,
    ),
  }));
}

export function deleteShape(tabId: string, id: string) {
  S().mutateDoc(tabId, (t) => ({ shapes: t.shapes.filter((sh) => sh.id !== id) }));
  S().setShapeSel(null);
}

export function deleteObject(tabId: string, src: number, id: number) {
  addObjectEdits(tabId, [{ src, id, action: "delete" }]);
  S().setObjSel(null);
}

export function replaceTextObject(
  tabId: string,
  src: number,
  id: number,
  bbox: [number, number, number, number],
  text: string,
  size: number,
  color: string,
  fontFamily?: string,
  lineHeight?: number,
) {
  S().mutateDoc(tabId, (t) => {
    const edits: ObjectEdit[] = [...t.edits, { src, id, action: "delete" }];
    if (!text.trim()) return { edits };
    const { w, h } = measureText(text, size, fontFamily, lineHeight);
    const stamp: StampItem = {
      id: crypto.randomUUID(),
      src,
      rect: [bbox[0], bbox[3] - h, bbox[0] + Math.max(w, 80), bbox[3]],
      kind: "text",
      text,
      size,
      color,
      fontFamily,
      lineHeight,
    };
    return { edits, stamps: [...t.stamps, stamp] };
  });
  S().setObjSel(null);
}

export const HIGHLIGHT_COLOR: [number, number, number] = [1.0, 0.82, 0.0];

export async function highlightSelection() {
  const tab = getActiveTab();
  if (!tab) return;
  const sel = window.getSelection();
  if (!sel || sel.isCollapsed || sel.rangeCount === 0) {
    S().setStatus(t("stSelectTextFirst"));
    return;
  }
  const entry = await getDoc(tab.id, docPath(tab), tab.docVersion);
  const pageEls = Array.from(
    document.querySelectorAll<HTMLElement>("[data-pageindex]"),
  ).map((el) => ({ el, index: Number(el.dataset.pageindex), box: el.getBoundingClientRect() }));

  const byIndex = new Map<number, DOMRect[]>();
  for (let r = 0; r < sel.rangeCount; r++) {
    for (const rect of Array.from(sel.getRangeAt(r).getClientRects())) {
      if (rect.width < 1 || rect.height < 1) continue;
      const cx = rect.left + rect.width / 2;
      const cy = rect.top + rect.height / 2;
      const hit = pageEls.find(
        (p) => cx >= p.box.left && cx <= p.box.right && cy >= p.box.top && cy <= p.box.bottom,
      );
      if (hit) {
        if (!byIndex.has(hit.index)) byIndex.set(hit.index, []);
        byIndex.get(hit.index)!.push(rect);
      }
    }
  }
  if (byIndex.size === 0) {
    S().setStatus(t("stSelectionMap"));
    return;
  }

  for (const [index, rects] of byIndex) {
    const spec = tab.pages[index];
    if (!spec) continue;
    const pageEl = pageEls.find((p) => p.index === index)!;
    const page = await getPage(entry, spec.src);
    const rotation = (page.rotate + spec.rotation + 360) % 360;
    const viewport = page.getViewport({ scale: tab.zoom, rotation });
    const pdfRects = rects.map((r) => {
      const [x1, y1] = viewport.convertToPdfPoint(r.left - pageEl.box.left, r.top - pageEl.box.top);
      const [x2, y2] = viewport.convertToPdfPoint(r.right - pageEl.box.left, r.bottom - pageEl.box.top);
      return [x1, y1, x2, y2] as [number, number, number, number];
    });
    S().mutateDoc(tab.id, (t) => ({
      highlights: [...t.highlights, { src: spec.src, rects: pdfRects, color: HIGHLIGHT_COLOR }],
    }));
  }
  sel.removeAllRanges();
  S().setStatus(t("stHighlightAdded"));
}

export async function runSearch(query: string) {
  const tab = getActiveTab();
  const store = S();
  store.setSearch({ query, matches: [], current: 0 });
  const q = query.trim().toLowerCase();
  if (!tab || q.length < 2) return;
  try {
    const entry = await getDoc(tab.id, docPath(tab), tab.docVersion);
    const matches: number[] = [];
    for (let i = 0; i < tab.pages.length; i++) {
      if (tab.pages[i].src === 0) continue;
      const text = (await getPageText(entry, tab.pages[i].src)).toLowerCase();
      let pos = 0;
      while ((pos = text.indexOf(q, pos)) !== -1) {
        matches.push(i);
        pos += q.length;
      }
    }
    if (S().search.query === query) {
      store.setSearch({ matches, current: 0 });
      if (matches.length) store.scrollTo(matches[0] + 1);
    }
  } catch (e) {
    report(t("stSearchFailed"), e);
  }
}

export function searchStep(dir: 1 | -1) {
  const { search, scrollTo, setSearch } = S();
  if (!search.matches.length) return;
  const current = (search.current + dir + search.matches.length) % search.matches.length;
  setSearch({ current });
  scrollTo(search.matches[current] + 1);
}

function humanSize(bytes: number): string {
  if (bytes >= 1024 * 1024) return `${(bytes / 1024 / 1024).toFixed(1)} MB`;
  return `${(bytes / 1024).toFixed(1)} KB`;
}

export async function compressActive(quality: number, maxDim: number) {
  const tab = getActiveTab();
  if (!tab) return;
  const out = await pickSavePathWithExt(
    tab.name.replace(/\.pdf$/i, "") + "-compressed.pdf",
    "pdf",
    "PDF",
  );
  if (!out) return;
  const outPath = out.endsWith(".pdf") ? out : out + ".pdf";
  try {
    const [orig, next] = await compressPdf(tab.path, outPath, quality, maxDim);
    const pct = orig > 0 ? Math.max(0, Math.round((1 - next / orig) * 100)) : 0;
    S().setStatus(t("stCompressed", { orig: humanSize(orig), new: humanSize(next), pct }));
    await openPaths([outPath]);
  } catch (e) {
    report(t("stCompressFailed"), e);
  }
}

export async function createNewPdf(w: number, h: number, pages: number) {
  try {
    const path = await createBlankPdf(w, h, pages);
    if (!path) return;
    await openPaths([path]);
    const tab = S().tabs.find((tb) => tb.path === path);
    if (tab) S().patchTab(tab.id, { name: t("untitled") });
  } catch (e) {
    report(t("stNewFailed"), e);
  }
}

async function currentPageSize(tab: Tab): Promise<[number, number]> {
  const spec = tab.pages[tab.currentPage - 1] ?? tab.pages[0];
  const entry = spec.src === 0 ? null : await getDoc(tab.id, docPath(tab), tab.docVersion);
  const base = specBase(entry, spec);
  const w = base?.w ?? 612;
  const h = base?.h ?? 792;
  return spec.rotation % 180 !== 0 ? [h, w] : [w, h];
}

export async function insertImageFromFile() {
  const tab = getActiveTab();
  if (!tab) return;
  const spec = tab.pages[tab.currentPage - 1];
  if (!spec) return;
  if (spec.src === 0) {
    S().setStatus(t("stCouldNotPlace"));
    return;
  }
  try {
    const path = await pickImagePath();
    if (!path) return;
    const dataUrl = await readImageB64(path);
    if (!dataUrl) return;
    const img = await loadImage(dataUrl);
    const [pw, ph] = await currentPageSize(tab);
    const aspect = img.naturalWidth / Math.max(1, img.naturalHeight);
    const w = Math.min(img.naturalWidth, pw * 0.6);
    const hh = w / Math.max(0.05, aspect);
    const x1 = (pw - w) / 2;
    const y1 = (ph - hh) / 2;
    addImageStamp(tab.id, spec.src, [x1, y1, x1 + w, y1 + hh], dataUrl);
  } catch (e) {
    report(t("stImageFailed"), e);
  }
}

export async function insertTableGrid(rows: number, cols: number) {
  const tab = getActiveTab();
  if (!tab) return;
  const spec = tab.pages[tab.currentPage - 1];
  if (!spec) return;
  if (spec.src === 0) {
    S().setStatus(t("stCouldNotPlace"));
    return;
  }
  try {
    const [pw, ph] = await currentPageSize(tab);
    const CELL_W = 90;
    const CELL_H = 28;
    let tw = cols * CELL_W;
    let th = rows * CELL_H;
    const scale = Math.min(1, (pw * 0.9) / tw);
    tw *= scale;
    th *= scale;
    const PX = 4;
    const canvas = document.createElement("canvas");
    canvas.width = Math.round(tw * PX);
    canvas.height = Math.round(th * PX);
    const ctx = canvas.getContext("2d")!;
    ctx.strokeStyle = "#333333";
    ctx.lineWidth = 1 * PX * scale;
    for (let r = 1; r < rows; r++) {
      const y = (th / rows) * r * PX;
      ctx.beginPath();
      ctx.moveTo(0, y);
      ctx.lineTo(canvas.width, y);
      ctx.stroke();
    }
    for (let c = 1; c < cols; c++) {
      const x = (tw / cols) * c * PX;
      ctx.beginPath();
      ctx.moveTo(x, 0);
      ctx.lineTo(x, canvas.height);
      ctx.stroke();
    }
    ctx.lineWidth = 1.5 * PX * scale;
    ctx.strokeRect(ctx.lineWidth / 2, ctx.lineWidth / 2, canvas.width - ctx.lineWidth, canvas.height - ctx.lineWidth);
    const dataUrl = canvas.toDataURL("image/png");
    const x1 = (pw - tw) / 2;
    const y1 = (ph - th) / 2;
    addImageStamp(tab.id, spec.src, [x1, y1, x1 + tw, y1 + th], dataUrl);
  } catch (e) {
    report(t("stImageFailed"), e);
  }
}

export function insertTextAtCenter(text: string, size: number) {
  const tab = getActiveTab();
  if (!tab) return;
  const spec = tab.pages[tab.currentPage - 1];
  if (!spec || spec.src === 0) return;
  void (async () => {
    try {
      const [pw, ph] = await currentPageSize(tab);
      const { w, h } = measureText(text, size);
      const x1 = (pw - w) / 2;
      const y2 = (ph + h) / 2;
      addTextStampAt(tab.id, spec.src, [x1, y2 - h, x1 + w, y2], text, size);
    } catch (e) {
      report(t("stCouldNotPlace"), e);
    }
  })();
}

let cmpSeq = 0;

export async function compareActive() {
  const tab = getActiveTab();
  if (!tab) return;
  try {
    const paths = await pickPdfs(false);
    if (!paths || !paths.length) return;
    const rightPath = paths[0];
    const seq = ++cmpSeq;
    S().openCompare({
      leftKey: `__cmpL__${seq}`,
      leftPath: docPath(tab),
      leftVersion: 0,
      leftName: tab.name,
      rightKey: `__cmpR__${seq}`,
      rightPath,
      rightVersion: 0,
      rightName: baseName(rightPath),
    });
  } catch (e) {
    report(t("stCmpFailed"), e);
  }
}

// ---- Bookmarks (outline) ----

function mapTree(nodes: Bookmark[], fn: (b: Bookmark) => Bookmark): Bookmark[] {
  return nodes.map((n) => fn({ ...n, children: mapTree(n.children, fn) }));
}

function removeNode(nodes: Bookmark[], id: string): Bookmark[] {
  return nodes
    .filter((n) => n.id !== id)
    .map((n) => ({ ...n, children: removeNode(n.children, id) }));
}

function currentBookmarks(tab: Tab): Bookmark[] {
  return tab.bookmarks ?? [];
}

export function addBookmarkHere() {
  const tab = getActiveTab();
  if (!tab) return;
  const spec = tab.pages[tab.currentPage - 1];
  const src = spec && spec.src > 0 ? spec.src : 0;
  const title = t("bmNewTitle", { n: tab.currentPage });
  S().mutateDoc(tab.id, (tt) => ({
    bookmarks: [...currentBookmarks(tt), newBookmark(title, src, null)],
  }));
}

export function renameBookmark(id: string, title: string) {
  const tab = getActiveTab();
  if (!tab) return;
  S().mutateDoc(tab.id, (tt) => ({
    bookmarks: mapTree(currentBookmarks(tt), (b) => (b.id === id ? { ...b, title } : b)),
  }));
}

export function retargetBookmark(id: string) {
  const tab = getActiveTab();
  if (!tab) return;
  const spec = tab.pages[tab.currentPage - 1];
  const src = spec && spec.src > 0 ? spec.src : 0;
  S().mutateDoc(tab.id, (tt) => ({
    bookmarks: mapTree(currentBookmarks(tt), (b) => (b.id === id ? { ...b, src, url: null, top: null } : b)),
  }));
}

export function deleteBookmark(id: string) {
  const tab = getActiveTab();
  if (!tab) return;
  S().mutateDoc(tab.id, (tt) => ({ bookmarks: removeNode(currentBookmarks(tt), id) }));
}

export function toggleBookmarkOpen(id: string) {
  const tab = getActiveTab();
  if (!tab) return;
  S().mutateDoc(tab.id, (tt) => ({
    bookmarks: mapTree(currentBookmarks(tt), (b) => (b.id === id ? { ...b, open: !b.open } : b)),
  }));
}

// Apply `fn` to the sibling list that contains `id`, wherever it sits in the tree.
function editSiblings(
  nodes: Bookmark[],
  id: string,
  fn: (list: Bookmark[], i: number) => Bookmark[] | null,
): Bookmark[] {
  const i = nodes.findIndex((n) => n.id === id);
  if (i >= 0) return fn([...nodes], i) ?? nodes;
  return nodes.map((n) => ({ ...n, children: editSiblings(n.children, id, fn) }));
}

export function moveBookmark(id: string, dir: -1 | 1) {
  const tab = getActiveTab();
  if (!tab) return;
  S().mutateDoc(tab.id, (tt) => ({
    bookmarks: editSiblings(currentBookmarks(tt), id, (list, i) => {
      const j = i + dir;
      if (j < 0 || j >= list.length) return null;
      [list[i], list[j]] = [list[j], list[i]];
      return list;
    }),
  }));
}

export function indentBookmark(id: string) {
  const tab = getActiveTab();
  if (!tab) return;
  S().mutateDoc(tab.id, (tt) => ({
    bookmarks: editSiblings(currentBookmarks(tt), id, (list, i) => {
      if (i === 0) return null;
      const prev = list[i - 1];
      list[i - 1] = { ...prev, open: true, children: [...prev.children, list[i]] };
      list.splice(i, 1);
      return list;
    }),
  }));
}

function liftOut(nodes: Bookmark[], id: string): Bookmark[] {
  for (let p = 0; p < nodes.length; p++) {
    const parent = nodes[p];
    const node = parent.children.find((c) => c.id === id);
    if (node) {
      const out = [...nodes];
      out[p] = { ...parent, children: parent.children.filter((c) => c.id !== id) };
      out.splice(p + 1, 0, node);
      return out;
    }
  }
  return nodes.map((n) => ({ ...n, children: liftOut(n.children, id) }));
}

export function outdentBookmark(id: string) {
  const tab = getActiveTab();
  if (!tab) return;
  S().mutateDoc(tab.id, (tt) => ({ bookmarks: liftOut(currentBookmarks(tt), id) }));
}

export function gotoBookmark(bm: Bookmark) {
  const tab = getActiveTab();
  if (!tab) return;
  if (bm.url) {
    void openLinkUrl(bm.url);
    return;
  }
  if (bm.src > 0) {
    const out = tab.pages.findIndex((p) => p.src === bm.src);
    if (out >= 0) S().scrollTo(out + 1);
  }
}

// ---- Interactive links ----

export function addLink(src: number, rect: Rect, target: { url?: string; destSrc?: number }) {
  const tab = getActiveTab();
  if (!tab) return;
  const link: LinkItem = {
    id: crypto.randomUUID(),
    src,
    rect,
    url: target.url ?? null,
    destSrc: target.destSrc ?? null,
  };
  S().mutateDoc(tab.id, (tt) => ({ links: [...tt.links, link] }));
}

export function deleteLink(id: string) {
  const tab = getActiveTab();
  if (!tab) return;
  S().mutateDoc(tab.id, (tt) => ({ links: tt.links.filter((l) => l.id !== id) }));
}

// An existing link baked into the file: record a removal to apply on save.
export function removeExistingLink(src: number, rect: Rect) {
  const tab = getActiveTab();
  if (!tab) return;
  S().mutateDoc(tab.id, (tt) => ({ linkRemovals: [...tt.linkRemovals, { src, rect }] }));
}

export async function openLinkUrl(url: string) {
  const safe = /^https?:\/\//i.test(url);
  if (!safe) {
    S().setStatus(t("lnkBlocked"));
    return;
  }
  const { openExternalUrl } = await import("./backend");
  await openExternalUrl(url);
}

export function followLink(target: { url?: string | null; destSrc?: number | null }) {
  if (target.url) {
    void openLinkUrl(target.url);
    return;
  }
  const tab = getActiveTab();
  if (!tab || !target.destSrc) return;
  const out = tab.pages.findIndex((p) => p.src === target.destSrc);
  if (out >= 0) S().scrollTo(out + 1);
}
