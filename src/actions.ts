import { useStore, getActiveTab } from "./store";
import type { Tab, Note, ObjectEdit, StampItem, ShapeItem } from "./types";
import {
  pickPdfs, pickSavePath, pickSavePathWithExt, pickDirectory, saveDocument,
  mergeDocuments, splitDocument, baseName, dirName, confirmDialog,
  printDocument, savePng, inTauri,
} from "./backend";
import { getDoc, getPage, getPageText, docPath, specBase } from "./pdf";
import { report } from "./errors";
import { measureText, rasterizeStamps } from "./stamps";

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
        highlights: [], notes: [], edits: [], stamps: [], shapes: [], previewPath: null, past: [], future: [],
        zoom: 1, currentPage: 1, selected: [],
        dirty: false, docVersion: 0,
      };
      S().addTab(tab);
      if (inTauri) pushRecent(path, tab.name);
    } catch (e) {
      report(`Could not open ${baseName(path)}`, e);
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
    );
    S().markSaved(tab.id, outPath, baseName(outPath));
    S().setStatus(`Saved ${baseName(outPath)}`);
  } catch (e) {
    report("Save failed", e);
  }
}

export async function closeTabSafe(id: string) {
  const tab = S().tabs.find((t) => t.id === id);
  if (!tab) return;
  if (tab.dirty) {
    const ok = await confirmDialog(`"${tab.name}" has unsaved changes. Close without saving?`);
    if (!ok) return;
  }
  S().closeTab(id);
}

export async function mergeFiles() {
  const paths = await pickPdfs(true);
  if (!paths || paths.length < 2) {
    if (paths) S().setStatus("Pick at least two PDF files to merge");
    return;
  }
  const out = await pickSavePath("merged.pdf");
  if (!out) return;
  const outPath = out.endsWith(".pdf") ? out : out + ".pdf";
  try {
    await mergeDocuments(paths, outPath);
    S().setStatus(`Merged ${paths.length} files`);
    await openPaths([outPath]);
  } catch (e) {
    report("Merge failed", e);
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
    S().setStatus(`Extracted ${specs.length} page(s)`);
    await openPaths([outPath]);
  } catch (e) {
    report("Extract failed", e);
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
    S().setStatus(`Split into ${parts.length} files in ${baseName(dirName(parts[0].out_path))}/`);
  } catch (e) {
    report("Split failed", e);
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
    S().setStatus("Cannot delete all pages");
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
    S().setStatus(`Exported page ${index + 1} as PNG`);
  } catch (e) {
    report("PNG export failed", e);
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
    S().setStatus("Opening in system viewer for printing…");
  } catch (e) {
    report("Print failed", e);
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
) {
  S().mutateDoc(tabId, (t) => {
    const edits: ObjectEdit[] = [...t.edits, { src, id, action: "delete" }];
    if (!text.trim()) return { edits };
    const { w, h } = measureText(text, size);
    const stamp: StampItem = {
      id: crypto.randomUUID(),
      src,
      rect: [bbox[0], bbox[3] - h, bbox[0] + Math.max(w, 80), bbox[3]],
      kind: "text",
      text,
      size,
      color,
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
    S().setStatus("Select some text first, then click Highlight");
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
    S().setStatus("Could not map the selection to a page");
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
  S().setStatus("Highlight added (saved into the PDF on save)");
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
    report("Search failed", e);
  }
}

export function searchStep(dir: 1 | -1) {
  const { search, scrollTo, setSearch } = S();
  if (!search.matches.length) return;
  const current = (search.current + dir + search.matches.length) % search.matches.length;
  setSearch({ current });
  scrollTo(search.matches[current] + 1);
}
