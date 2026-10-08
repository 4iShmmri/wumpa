import { useStore, getActiveTab } from "./store";
import type { Tab, Note } from "./types";
import {
  pickPdfs, pickSavePath, pickDirectory, saveDocument, mergeDocuments,
  splitDocument, baseName, dirName, confirmDialog,
} from "./backend";
import { getDoc, getPage, getPageText } from "./pdf";
import { report } from "./errors";

const S = useStore.getState;

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
        highlights: [], notes: [], past: [], future: [],
        zoom: 1, currentPage: 1, selected: [],
        dirty: false, docVersion: 0,
      };
      S().addTab(tab);
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
    await saveDocument(
      tab.path, outPath, tab.pages, tab.highlights,
      tab.notes.map(({ src, x, y, text }) => ({ src, x, y, text })),
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
    await saveDocument(tab.path, outPath, specs, [], []);
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

export const HIGHLIGHT_COLOR: [number, number, number] = [1.0, 0.82, 0.0];

export async function highlightSelection() {
  const tab = getActiveTab();
  if (!tab) return;
  const sel = window.getSelection();
  if (!sel || sel.isCollapsed || sel.rangeCount === 0) {
    S().setStatus("Select some text first, then click Highlight");
    return;
  }
  const entry = await getDoc(tab.id, tab.path, tab.docVersion);
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
    const entry = await getDoc(tab.id, tab.path, tab.docVersion);
    const matches: number[] = [];
    for (let i = 0; i < tab.pages.length; i++) {
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
