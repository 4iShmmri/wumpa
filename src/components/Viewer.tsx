import { useCallback, useEffect, useLayoutEffect, useMemo, useRef, useState } from "react";
import { useStore, useActiveTab, getActiveTab } from "../store";
import { getDoc, docPath, specBase, type DocEntry } from "../pdf";
import type { Note } from "../types";
import { addNote, deleteNote } from "../actions";
import { addTextStamp, addSignatureStamp, loadImage } from "../stamps";
import PdfPage from "./viewer/PdfPage";
import Welcome from "./viewer/Welcome";
import { specViewport } from "./viewer/viewport";
import { t, useT } from "../i18n";
import { report } from "../errors";
import { noteScroll } from "../scrollState";

const NONE: never[] = [];

// Buckets a tab array by source page once per array identity, so each page
// receives the same array instance until that kind of annotation changes.
function groupBySrc<T extends { src: number }>(items: readonly T[]): Map<number, T[]> {
  const m = new Map<number, T[]>();
  for (const it of items) {
    let arr = m.get(it.src);
    if (!arr) m.set(it.src, (arr = []));
    arr.push(it);
  }
  return m;
}

export default function Viewer() {
  useT();
  const tab = useActiveTab();
  const searchQuery = useStore((s) => (s.search.open ? s.search.query : ""));
  const scrollTarget = useStore((s) => s.scrollTarget);
  const tool = useStore((s) => s.tool);
  const [entry, setEntry] = useState<DocEntry | null>(null);
  const [openNote, setOpenNote] = useState<string | null>(null);
  const openNoteRef = useRef<string | null>(null);
  openNoteRef.current = openNote;
  const containerRef = useRef<HTMLDivElement>(null);
  const slotRefs = useRef<(HTMLDivElement | null)[]>([]);
  const ticking = useRef(false);
  const scrollRatio = useRef<number | null>(null);
  const ratioTabRef = useRef<string | undefined>(undefined);

  const tabId = tab?.id;
  const docVersion = tab?.docVersion ?? 0;

  const highlights = tab?.highlights ?? NONE;
  const notes = tab?.notes ?? NONE;
  const stamps = tab?.stamps ?? NONE;
  const shapes = tab?.shapes ?? NONE;
  const links = tab?.links ?? NONE;
  const linkRemovals = tab?.linkRemovals ?? NONE;
  const hlBy = useMemo(() => groupBySrc(highlights), [highlights]);
  const noteBy = useMemo(() => groupBySrc(notes), [notes]);
  const stampBy = useMemo(() => groupBySrc(stamps), [stamps]);
  const shapeBy = useMemo(() => groupBySrc(shapes), [shapes]);
  const linkBy = useMemo(() => groupBySrc(links), [links]);
  const removalBy = useMemo(() => groupBySrc(linkRemovals), [linkRemovals]);

  useEffect(() => {
    let on = true;
    setEntry(null);
    setOpenNote(null);
    if (!tab) return;
    getDoc(tab.id, docPath(tab), tab.docVersion)
      .then((e) => on && setEntry(e))
      .catch((e) => report(t("stLoadFailed"), e));
    return () => {
      on = false;
    };
  }, [tabId, docVersion]);

  useEffect(() => {
    if (!scrollTarget) return;
    slotRefs.current[scrollTarget.page - 1]?.scrollIntoView({ block: "start" });
  }, [scrollTarget?.nonce, entry]);

  const fitNonce = useStore((s) => s.fitNonce);
  useEffect(() => {
    if (!fitNonce) return;
    const cur = getActiveTab();
    if (!cur || !entry || !containerRef.current) return;
    const spec = cur.pages[cur.currentPage - 1] ?? cur.pages[0];
    const base = spec && specBase(entry, spec);
    if (!base) return;
    const bw = spec.rotation % 180 !== 0 ? base.h : base.w;
    const zoom = Math.max(0.25, Math.min(6, (containerRef.current.clientWidth - 56) / bw));
    useStore.getState().patchTab(cur.id, { zoom: Math.round(zoom * 100) / 100 });
  }, [fitNonce]);

  // Ctrl/Cmd + wheel zoom; must be non-passive to preventDefault.
  useEffect(() => {
    const c = containerRef.current;
    if (!c) return;
    const onWheel = (e: WheelEvent) => {
      if (!e.ctrlKey && !e.metaKey) return;
      e.preventDefault();
      const cur = getActiveTab();
      if (!cur) return;
      const factor = e.deltaY < 0 ? 1.1 : 1 / 1.1;
      const zoom = Math.max(0.25, Math.min(6, Math.round(cur.zoom * factor * 100) / 100));
      if (zoom !== cur.zoom) useStore.getState().patchTab(cur.id, { zoom });
    };
    c.addEventListener("wheel", onWheel, { passive: false });
    return () => c.removeEventListener("wheel", onWheel);
  }, [tabId]);

  // Keep the viewport centered on the same spot across zoom changes.
  useLayoutEffect(() => {
    const c = containerRef.current;
    if (!c || !tab) return;
    if (ratioTabRef.current !== tab.id) {
      ratioTabRef.current = tab.id;
      scrollRatio.current = null;
      return;
    }
    if (scrollRatio.current != null) {
      c.scrollTop = scrollRatio.current * c.scrollHeight - c.clientHeight / 2;
    }
  }, [tab?.zoom]);

  // Stable identity: it is a prop of every memoized page.
  const closePopover = useCallback(() => {
    const id = openNoteRef.current;
    if (id && !id.startsWith("ex:")) {
      const cur = getActiveTab();
      const n = cur?.notes.find((x) => x.id === id);
      if (cur && n && n.text.trim() === "") deleteNote(cur.id, n.id);
    }
    setOpenNote(null);
  }, []);

  useEffect(() => {
    if (!openNote) return;
    const onDown = (e: PointerEvent) => {
      const el = e.target as Element | null;
      if (!el?.closest?.(".note-popover, .note-icon")) closePopover();
    };
    const onKey = (e: KeyboardEvent) => {
      if (e.key === "Escape") {
        e.stopPropagation();
        closePopover();
      }
    };
    document.addEventListener("pointerdown", onDown, true);
    document.addEventListener("keydown", onKey, true);
    return () => {
      document.removeEventListener("pointerdown", onDown, true);
      document.removeEventListener("keydown", onKey, true);
    };
  }, [openNote]);

  const onViewerClick = async (e: React.MouseEvent) => {
    const st = useStore.getState();
    const tool = st.tool;
    if (tool !== "note" && tool !== "text" && tool !== "sign") return;
    const target = e.target as Element;
    if (target.closest(".note-icon, .note-popover, .stamp, .stamp-bar, .stamp-edit, .stamp-handle")) return;
    const wrap = target.closest<HTMLElement>("[data-pageindex]");
    const cur = getActiveTab();
    if (!wrap || !cur || !entry) return;
    const index = Number(wrap.dataset.pageindex);
    const spec = cur.pages[index];
    if (!spec) return;
    try {
      const viewport = await specViewport(entry, spec, cur.zoom);
      const box = wrap.getBoundingClientRect();
      const [px, py] = viewport.convertToPdfPoint(e.clientX - box.left, e.clientY - box.top) as [
        number,
        number,
      ];
      if (tool === "note") {
        const note: Note = { id: crypto.randomUUID(), src: spec.src, x: px, y: py, text: "" };
        addNote(cur.id, note);
        setOpenNote(note.id);
      } else if (tool === "text") {
        const stamp = addTextStamp(cur.id, spec.src, px, py);
        st.setStampSel({ src: spec.src, id: stamp.id });
        st.setStampEditing(stamp.id);
      } else {
        let sig: string | null = null;
        try {
          sig = localStorage.getItem("signature");
        } catch {
          sig = null;
        }
        if (!sig) {
          st.setSignatureModal(true);
        } else {
          const img = await loadImage(sig);
          const stamp = addSignatureStamp(cur.id, spec.src, px, py, sig, img.width / Math.max(1, img.height));
          st.setStampSel({ src: spec.src, id: stamp.id });
        }
      }
    } catch (err) {
      report(t("stCouldNotPlace"), err);
    }
    st.setTool("select");
  };

  const onScroll = () => {
    noteScroll();
    if (ticking.current) return;
    ticking.current = true;
    requestAnimationFrame(() => {
      ticking.current = false;
      const cur = getActiveTab();
      const c = containerRef.current;
      if (!cur || !c) return;
      scrollRatio.current = (c.scrollTop + c.clientHeight / 2) / Math.max(1, c.scrollHeight);
      const refTop = c.getBoundingClientRect().top + 10;
      let current = 1;
      slotRefs.current.forEach((el, i) => {
        if (el && el.getBoundingClientRect().top <= refTop + c.clientHeight / 3) current = i + 1;
      });
      if (current !== cur.currentPage) useStore.getState().patchTab(cur.id, { currentPage: current });
    });
  };

  if (!tab) return <Welcome />;

  return (
    <div
      className={
        "viewer" +
        (tool === "note" ? " note-mode" : "") +
        (tool === "edit" ? " edit-mode" : "") +
        (tool === "erase" ? " erase-mode" : "") +
        (tool === "text" ? " text-mode" : "") +
        (tool === "sign" ? " sign-mode" : "") +
        (tool === "draw" ? " draw-mode" : "") +
        (tool === "link" ? " link-mode" : "")
      }
      ref={containerRef}
      onScroll={onScroll}
      onClick={tool === "note" || tool === "text" || tool === "sign" ? onViewerClick : undefined}
    >
      {!entry && <div className="loading">{t("loading")}</div>}
      {entry && (
        <div className="pages">
          {tab.pages.map((spec, i) => {
            const base = specBase(entry, spec);
            if (!base) return null;
            const swap = spec.rotation % 180 !== 0;
            const w = (swap ? base.h : base.w) * tab.zoom;
            const h = (swap ? base.w : base.h) * tab.zoom;
            const pageNotes = noteBy.get(spec.src) ?? NONE;
            // Hand the open note only to the page it sits on, so opening one
            // re-renders that page alone.
            const pageOpenNote =
              openNote && (openNote.startsWith(`ex:${spec.src}:`) || pageNotes.some((n) => n.id === openNote))
                ? openNote
                : null;
            return (
              <div
                className="page-slot"
                key={`${i}:${spec.src}`}
                ref={(el) => {
                  slotRefs.current[i] = el;
                }}
              >
                <PdfPage
                  entry={entry}
                  spec={spec}
                  index={i}
                  zoom={tab.zoom}
                  w={w}
                  h={h}
                  searchQuery={searchQuery}
                  highlights={hlBy.get(spec.src) ?? NONE}
                  notes={pageNotes}
                  stamps={stampBy.get(spec.src) ?? NONE}
                  shapes={shapeBy.get(spec.src) ?? NONE}
                  links={linkBy.get(spec.src) ?? NONE}
                  removals={removalBy.get(spec.src) ?? NONE}
                  tabId={tab.id}
                  tabPath={tab.path}
                  edits={tab.edits}
                  tool={tool}
                  openNote={pageOpenNote}
                  setOpenNote={setOpenNote}
                  requestClose={closePopover}
                />
              </div>
            );
          })}
        </div>
      )}
    </div>
  );
}
