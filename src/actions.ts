import { useStore, getActiveTab } from "./store";
import type {
  Tab,
  Note,
  ObjectEdit,
  StampItem,
  ShapeItem,
  Bookmark,
  LinkItem,
  Rect,
  SecurityPolicy,
  PageSpec,
} from "./types";
import {
  pickPdfs,
  pickSavePath,
  pickSavePathWithExt,
  pickDirectory,
  saveDocument,
  mergeDocuments,
  splitDocument,
  baseName,
  dirName,
  confirmDialog,
  openExternalUrl,
  printDocument,
  savePng,
  inTauri,
  compressPdf,
  createBlankPdf,
  readImageB64,
  pickImagePath,
  type SaveExtras,
  encryptPdf,
  securityInfo,
  removeSecurity,
  type PdfPermissions,
  imageOnlyPages,
  rebuildScanned,
  listenPayload,
  type ScanProgress,
  exportPage,
  exportWrite,
  type ExportFormat,
  type ExportPage,
  type ExportProgress,
  autotestPlan,
  logLine,
  exitApp,
} from "./backend";
import { extractNativePage } from "./exportExtract";
import { getDoc, getPage, getPageText, docPath, specBase, pageRotation } from "./pdf";
import { loadBookmarks, outlineToWire, linksToWire, linkRemovalsToWire, newBookmark } from "./outline";
import { report } from "./errors";
import { measureText, rasterizeStamps, addTextStampAt, addImageStamp, loadImage } from "./stamps";
import { t } from "./i18n";

const ARABIC_RE = /[\u0600-\u06FF\u0750-\u077F\u08A0-\u08FF\uFB50-\uFDFF\uFE70-\uFEFF]/g;

async function detectTabLang(tabId: string, entry: Awaited<ReturnType<typeof getDoc>>) {
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
    const list = [{ path, name, time: Date.now() }, ...getRecentFiles().filter((r) => r.path !== path)].slice(
      0,
      10,
    );
    localStorage.setItem("recentFiles", JSON.stringify(list));
  } catch {
    /* best effort */
  }
}

export function dropRecent(path: string) {
  try {
    localStorage.setItem("recentFiles", JSON.stringify(getRecentFiles().filter((r) => r.path !== path)));
  } catch {
    /* best effort */
  }
}

export async function openPaths(paths: string[], opts: { untitled?: boolean } = {}) {
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
        id,
        path,
        name: opts.untitled ? t("untitled") : baseName(path),
        untitled: !!opts.untitled,
        pages: Array.from({ length: entry.numPages }, (_, i) => ({ src: i + 1, rotation: 0 })),
        highlights: [],
        notes: [],
        edits: [],
        stamps: [],
        shapes: [],
        bookmarks: null,
        links: [],
        linkRemovals: [],
        previewPath: null,
        past: [],
        future: [],
        zoom: 1,
        currentPage: 1,
        selected: [],
        dirty: false,
        docVersion: 0,
      };
      S().addTab(tab);
      if (inTauri && !opts.untitled) pushRecent(path, tab.name);
      void refreshSecurity(id);
      void detectTabLang(id, entry);
      void refreshBookmarks(id);
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
  if (!tab || tab.saving) return;
  if (tab.security?.encrypted) {
    report(t("stSaveProtected"), null);
    return;
  }
  let outPath = tab.path;
  if (saveAs || tab.untitled) {
    const picked = await pickSavePath(tab.name);
    if (!picked) return;
    outPath = ensurePdf(picked);
  }
  S().patchTab(tab.id, { saving: true });
  S().setStatus(t("stSaving"));
  try {
    const payload = await tabPayload(tab);
    await saveDocument(
      tab.path,
      outPath,
      tab.pages,
      payload.highlights,
      payload.notes,
      tab.edits,
      payload.stamps,
      payload.shapes,
      saveExtras(tab),
    );
    S().markSaved(tab.id, outPath, baseName(outPath));
    S().setStatus(t("stSaved", { name: baseName(outPath) }));
    void refreshBookmarks(tab.id);
    void refreshSecurity(tab.id);
  } catch (e) {
    S().patchTab(tab.id, { saving: false });
    report(t("stSaveFailed"), e);
  }
}

const ensurePdf = (path: string) => (/\.pdf$/i.test(path) ? path : path + ".pdf");

// The per-page items the core understands, optionally limited to some source pages.
async function tabPayload(tab: Tab, srcs?: Set<number>) {
  const keep = <T extends { src: number }>(items: T[]) =>
    srcs ? items.filter((i) => srcs.has(i.src)) : items;
  return {
    highlights: keep(tab.highlights),
    notes: keep(tab.notes).map(({ src, x, y, text }) => ({ src, x, y, text })),
    stamps: await rasterizeStamps(keep(tab.stamps)),
    shapes: keep(tab.shapes).map((sh) => ({
      src: sh.src,
      kind: sh.kind,
      rect: sh.rect,
      strokes: sh.strokes,
      color: sh.color,
      width: sh.width,
      opacity: sh.opacity,
    })),
  };
}

function saveExtras(tab: Tab): SaveExtras {
  return {
    outline: tab.bookmarksTouched && tab.bookmarks ? outlineToWire(tab.bookmarks, tab.pages) : null,
    links: linksToWire(tab.links, tab.pages),
    linkRemovals: linkRemovalsToWire(tab.linkRemovals, tab.pages),
  };
}

// Loads the file's outline into the tab unless the user already edited bookmarks.
async function refreshBookmarks(tabId: string) {
  const tab = S().tabs.find((t) => t.id === tabId);
  if (!tab) return;
  try {
    const entry = await getDoc(tabId, docPath(tab), tab.docVersion);
    const bms = await loadBookmarks(entry.doc);
    const cur = S().tabs.find((t) => t.id === tabId);
    if (cur && cur.bookmarks === null && cur.docVersion === tab.docVersion) {
      S().patchTab(tabId, { bookmarks: bms });
    }
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
  const outPath = ensurePdf(out);
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
  const outPath = ensurePdf(out);
  try {
    const srcs = new Set(specs.map((p) => p.src));
    const payload = await tabPayload(tab, srcs);
    await saveDocument(
      tab.path,
      outPath,
      specs,
      payload.highlights,
      payload.notes,
      tab.edits,
      payload.stamps,
      payload.shapes,
    );
    S().setStatus(t("stExtracted", { n: specs.length }));
    await openPaths([outPath]);
  } catch (e) {
    report(t("stExtractFailed"), e);
  }
}

export async function splitEvery(n: number) {
  const tab = getActiveTab();
  if (!tab || n < 1) return;
  if (tab.dirty && !(await confirmDialog(t("compressDirtyWarn")))) return;
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
      indices.includes(i) ? { ...p, rotation: pageRotation(p.rotation, delta) } : p,
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

// Blank pages are identified by a unique negative src that survives reordering.
const nextBlankSrc = (pages: PageSpec[]) => -(Math.max(0, ...pages.map((p) => -p.src)) + 1);

export async function insertBlankPage(tabId: string, atIndex: number) {
  const tab = S().tabs.find((t) => t.id === tabId);
  if (!tab) return;
  let w = 612;
  let h = 792;
  const ref = tab.pages[atIndex - 1] ?? tab.pages[atIndex] ?? tab.pages[0];
  if (ref) {
    try {
      const entry = ref.src <= 0 ? null : await getDoc(tab.id, docPath(tab), tab.docVersion);
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
    pages.splice(atIndex, 0, { src: nextBlankSrc(t.pages), rotation: 0, w, h });
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
    if (spec.src <= 0) {
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
      const rotation = pageRotation(page.rotate, spec.rotation);
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
    const payload = await tabPayload(tab);
    await printDocument(
      tab.path,
      tab.pages,
      payload.highlights,
      payload.notes,
      tab.edits,
      payload.stamps,
      payload.shapes,
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
        e.action === "translate" &&
        last &&
        last.action === "translate" &&
        last.id === e.id &&
        last.src === e.src
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
              number,
              number,
              number,
              number,
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

const HIGHLIGHT_COLOR: [number, number, number] = [1.0, 0.82, 0.0];

export async function highlightSelection() {
  const active = getActiveTab();
  if (!active) return;
  const sel = window.getSelection();
  if (!sel || sel.isCollapsed || sel.rangeCount === 0) {
    S().setStatus(t("stSelectTextFirst"));
    return;
  }
  const entry = await getDoc(active.id, docPath(active), active.docVersion);
  // Zoom may have changed during the await; the DOM boxes follow the current tab.
  const tab = getActiveTab();
  if (!tab || tab.id !== active.id) return;
  const pageEls = Array.from(document.querySelectorAll<HTMLElement>("[data-pageindex]")).map((el) => ({
    el,
    index: Number(el.dataset.pageindex),
    box: el.getBoundingClientRect(),
  }));

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

  const added: Tab["highlights"] = [];
  for (const [index, rects] of byIndex) {
    const spec = tab.pages[index];
    if (!spec) continue;
    const pageEl = pageEls.find((p) => p.index === index)!;
    const page = await getPage(entry, spec.src);
    const viewport = page.getViewport({
      scale: tab.zoom,
      rotation: pageRotation(page.rotate, spec.rotation),
    });
    const pdfRects = rects.map((r) => {
      const [x1, y1] = viewport.convertToPdfPoint(r.left - pageEl.box.left, r.top - pageEl.box.top);
      const [x2, y2] = viewport.convertToPdfPoint(r.right - pageEl.box.left, r.bottom - pageEl.box.top);
      return [x1, y1, x2, y2] as [number, number, number, number];
    });
    added.push({ src: spec.src, rects: pdfRects, color: HIGHLIGHT_COLOR });
  }
  if (!added.length) return;
  S().mutateDoc(tab.id, (tt) => ({ highlights: [...tt.highlights, ...added] }));
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
      if (tab.pages[i].src <= 0) continue;
      const text = (await getPageText(entry, tab.pages[i].src)).toLowerCase();
      let pos = 0;
      while ((pos = text.indexOf(q, pos)) !== -1) {
        matches.push(i);
        pos += q.length;
      }
    }
    if (S().search.query === query) {
      store.setSearch({ matches: [...new Set(matches)], current: 0 });
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
  const out = await pickSavePathWithExt(tab.name.replace(/\.pdf$/i, "") + "-compressed.pdf", "pdf", "PDF");
  if (!out) return;
  const outPath = ensurePdf(out);
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
    await openPaths([path], { untitled: true });
  } catch (e) {
    report(t("stNewFailed"), e);
  }
}

async function currentPageSize(tab: Tab): Promise<[number, number]> {
  const spec = tab.pages[tab.currentPage - 1] ?? tab.pages[0];
  const entry = spec.src <= 0 ? null : await getDoc(tab.id, docPath(tab), tab.docVersion);
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
  if (spec.src <= 0) {
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
  if (spec.src <= 0) {
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
    ctx.strokeRect(
      ctx.lineWidth / 2,
      ctx.lineWidth / 2,
      canvas.width - ctx.lineWidth,
      canvas.height - ctx.lineWidth,
    );
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
  if (!spec || spec.src <= 0) return;
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

export function compareActive() {
  const tab = getActiveTab();
  S().openCompareDialog(tab ? docPath(tab) || null : null);
}

export function startCompare(oldPath: string, newPath: string) {
  const seq = ++cmpSeq;
  S().openCompare({
    leftKey: `__cmpL__${seq}`,
    leftPath: oldPath,
    leftVersion: 0,
    leftName: baseName(oldPath),
    rightKey: `__cmpR__${seq}`,
    rightPath: newPath,
    rightVersion: 0,
    rightName: baseName(newPath),
  });
}

// ---- Password protection ----

// Describes the file on disk, so it is refreshed whenever the tab's path or
// saved version changes.
export async function refreshSecurity(tabId: string) {
  const tab = S().tabs.find((tb) => tb.id === tabId);
  if (!tab || !inTauri) return;
  const security = await securityInfo(tab.path).catch(() => null);
  if (S().tabs.some((tb) => tb.id === tabId && tb.path === tab.path)) {
    S().patchTab(tabId, { security });
  }
}

function outName(tab: Tab, suffix: string) {
  return tab.name.replace(/\.pdf$/i, "") + suffix + ".pdf";
}

async function pickOut(tab: Tab, suffix: string): Promise<string | null> {
  const out = await pickSavePathWithExt(outName(tab, suffix), "pdf", "PDF");
  return out ? ensurePdf(out) : null;
}

export async function protectActive(
  userPassword: string,
  ownerPassword: string,
  perms: PdfPermissions,
  policy: SecurityPolicy | null = null,
) {
  const tab = getActiveTab();
  if (!tab) return;
  const outPath = await pickOut(tab, "-protected");
  if (!outPath) return;
  try {
    await encryptPdf(tab.path, outPath, userPassword, ownerPassword, perms, policy);
    S().setStatus(t("stProtected", { name: baseName(outPath) }));
    await openPaths([outPath]);
  } catch (e) {
    report(t("stProtectFailed"), e);
  }
}

export async function unprotectActive(password: string): Promise<boolean> {
  const tab = getActiveTab();
  if (!tab) return false;
  const outPath = await pickOut(tab, "-unlocked");
  if (!outPath) return false;
  try {
    await removeSecurity(tab.path, outPath, password);
    S().setStatus(t("stUnprotected", { name: baseName(outPath) }));
    await openPaths([outPath]);
    return true;
  } catch (e) {
    const msg = String(e);
    if (/wrong password/i.test(msg)) {
      report(t("stWrongPassword"), e);
    } else if (/permissions password/i.test(msg)) {
      report(t("stNeedOwnerPassword"), e);
    } else {
      report(t("stUnprotectFailed"), e);
    }
    return false;
  }
}

// ---- Image to PDF (scanned pages) ----

export async function scannedPagesOfActive(): Promise<number[]> {
  const tab = getActiveTab();
  if (!tab) return [];
  try {
    return await imageOnlyPages(tab.path);
  } catch {
    return [];
  }
}

// Rebuilds the chosen scanned pages as real text and opens the result.
export async function rebuildScannedActive(
  pages: number[],
  langs: string[],
  onProgress: (p: ScanProgress) => void,
): Promise<boolean> {
  const tab = getActiveTab();
  if (!tab || !pages.length) return false;
  const outPath = await pickOut(tab, "-text");
  if (!outPath) return false;
  const stop = await listenPayload<ScanProgress>("scan-progress", onProgress);
  try {
    await rebuildScanned(tab.path, outPath, pages, langs);
    S().setStatus(t("stScanDone", { name: baseName(outPath), n: pages.length }));
    await openPaths([outPath]);
    return true;
  } catch (e) {
    report(t("stScanFailed"), e);
    return false;
  } finally {
    stop();
  }
}

// ---- Export to Word / Excel ----

export interface ExportStatus {
  page: number; // 1-based source page being processed
  index: number; // 0-based position in the batch
  total: number;
  stage: string; // extract | recognize | layout | write
}

// Exports the chosen source pages of a tab to `outPath` as a Word or Excel
// file; scanned pages are recognised with `langs`.
export async function exportTabTo(
  tab: Tab,
  format: ExportFormat,
  srcs: number[],
  langs: string[],
  outPath: string,
  onProgress: (p: ExportStatus) => void,
): Promise<void> {
  const total = srcs.length;
  let index = 0;
  const stop = await listenPayload<ExportProgress>("export-progress", (p) => {
    const stage = p.stage === "ocr" ? "recognize" : p.stage === "done" ? "layout" : null;
    if (stage) onProgress({ page: p.page, index, total, stage });
  });
  try {
    const scanned = new Set(await imageOnlyPages(tab.path));
    const entry = await getDoc(tab.id, docPath(tab), tab.docVersion);
    const pages: ExportPage[] = [];
    for (const src of srcs) {
      onProgress({ page: src, index, total, stage: "extract" });
      const input = scanned.has(src)
        ? { kind: "scanned" as const, page: src }
        : await extractNativePage(entry, src);
      onProgress({ page: src, index, total, stage: "layout" });
      pages.push(await exportPage(tab.path, input, langs));
      index++;
    }
    onProgress({ page: srcs[total - 1], index: total - 1, total, stage: "write" });
    await exportWrite(pages, format, outPath, S().effectiveLang);
  } finally {
    stop();
  }
}

// Asks where to save, then exports the chosen pages of the active tab.
export async function exportActive(
  format: ExportFormat,
  srcs: number[],
  langs: string[],
  onProgress: (p: ExportStatus) => void,
): Promise<{ ok: boolean; error?: string }> {
  const tab = getActiveTab();
  if (!tab || !srcs.length) return { ok: false };
  const ext = format;
  const picked = await pickSavePathWithExt(
    tab.name.replace(/\.pdf$/i, "") + "." + ext,
    ext,
    format === "docx" ? "Word" : "Excel",
  );
  if (!picked) return { ok: false };
  const outPath = picked.toLowerCase().endsWith("." + ext) ? picked : picked + "." + ext;
  try {
    await exportTabTo(tab, format, srcs, langs, outPath, onProgress);
    S().setStatus(t("stExportDone", { name: baseName(outPath) }));
    return { ok: true };
  } catch (e) {
    report(t("stExportFailed"), e);
    return { ok: false, error: e instanceof Error ? e.message : String(e) };
  }
}

// ---- Automated in-app test (debug builds) ----

// Opens the planned file, reads its text, exports it to Word and Excel,
// writes the verdict to the app log and quits. Runs the real viewer and
// the real IPC, which unit tests cannot.
export async function runAutotest(): Promise<void> {
  const plan = await autotestPlan();
  if (!plan) return;
  const verdict = async (): Promise<string> => {
    await openPaths([plan.src]);
    const tab = S().tabs.find((t) => t.path === plan.src);
    if (!tab) return "failed: file did not open";
    const entry = await getDoc(tab.id, docPath(tab), tab.docVersion);
    const text = await getPageText(entry, 1);
    if (!text.trim()) return "failed: no text read from page 1";
    const srcs = tab.pages.filter((p) => p.src > 0).map((p) => p.src);
    const quiet = () => {};
    await exportTabTo(tab, "docx", srcs, ["ar", "en"], plan.out + ".docx", quiet);
    await exportTabTo(tab, "xlsx", srcs, ["ar", "en"], plan.out + ".xlsx", quiet);
    return `ok: ${srcs.length} page(s), ${text.length} chars on page 1`;
  };
  let result: string;
  try {
    result = await verdict();
  } catch (e) {
    result = `failed: ${e instanceof Error ? e.message : String(e)}`;
  }
  logLine(`autotest ${result}`);
  await exitApp();
}

// ---- Bookmarks (outline) ----

function mapTree(nodes: Bookmark[], fn: (b: Bookmark) => Bookmark): Bookmark[] {
  return nodes.map((n) => fn({ ...n, children: mapTree(n.children, fn) }));
}

function removeNode(nodes: Bookmark[], id: string): Bookmark[] {
  return nodes.filter((n) => n.id !== id).map((n) => ({ ...n, children: removeNode(n.children, id) }));
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
    bookmarksTouched: true,
    bookmarks: [...currentBookmarks(tt), newBookmark(title, src, null)],
  }));
}

export function renameBookmark(id: string, title: string) {
  const tab = getActiveTab();
  if (!tab) return;
  S().mutateDoc(tab.id, (tt) => ({
    bookmarksTouched: true,
    bookmarks: mapTree(currentBookmarks(tt), (b) => (b.id === id ? { ...b, title } : b)),
  }));
}

export function retargetBookmark(id: string) {
  const tab = getActiveTab();
  if (!tab) return;
  const spec = tab.pages[tab.currentPage - 1];
  const src = spec && spec.src > 0 ? spec.src : 0;
  S().mutateDoc(tab.id, (tt) => ({
    bookmarksTouched: true,
    bookmarks: mapTree(currentBookmarks(tt), (b) => (b.id === id ? { ...b, src, url: null, top: null } : b)),
  }));
}

export function deleteBookmark(id: string) {
  const tab = getActiveTab();
  if (!tab) return;
  S().mutateDoc(tab.id, (tt) => ({
    bookmarksTouched: true,
    bookmarks: removeNode(currentBookmarks(tt), id),
  }));
}

export function toggleBookmarkOpen(id: string) {
  const tab = getActiveTab();
  if (!tab) return;
  S().mutateDoc(tab.id, (tt) => ({
    bookmarksTouched: true,
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
    bookmarksTouched: true,
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
    bookmarksTouched: true,
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
  S().mutateDoc(tab.id, (tt) => ({ bookmarksTouched: true, bookmarks: liftOut(currentBookmarks(tt), id) }));
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
