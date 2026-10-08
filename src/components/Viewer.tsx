import { useEffect, useLayoutEffect, useRef, useState } from "react";
import { TextLayer } from "pdfjs-dist";
import type { PageViewport } from "pdfjs-dist";
import { useStore, useActiveTab } from "../store";
import { getDoc, getPage, getAnnots, type DocEntry, type ExistingAnnot } from "../pdf";
import type { PageSpec, Highlight, Note } from "../types";
import { openFileDialog, addNote, updateNote, deleteNote } from "../actions";
import { report } from "../errors";

function esc(s: string) {
  return s.replace(/[&<>"]/g, (c) => ({ "&": "&amp;", "<": "&lt;", ">": "&gt;", '"': "&quot;" })[c]!);
}

function markText(orig: string, q: string): string {
  const lower = orig.toLowerCase();
  let out = "";
  let pos = 0;
  let i;
  while ((i = lower.indexOf(q, pos)) !== -1) {
    out += esc(orig.slice(pos, i)) + "<mark>" + esc(orig.slice(i, i + q.length)) + "</mark>";
    pos = i + q.length;
  }
  return out + esc(orig.slice(pos));
}

function vpTopLeft(viewport: PageViewport, x1: number, y1: number, x2: number, y2: number) {
  const [ax, ay] = viewport.convertToViewportPoint(x1, y1) as [number, number];
  const [bx, by] = viewport.convertToViewportPoint(x2, y2) as [number, number];
  return { left: Math.min(ax, bx), top: Math.min(ay, by) };
}

function popoverPos(iconLeft: number, iconTop: number, size: number, w: number, h: number) {
  return {
    left: Math.max(4, Math.min(iconLeft + size + 8, w - 252)),
    top: Math.max(4, Math.min(iconTop, h - 140)),
  };
}

const NOTE_PT = 18; // icon footprint in PDF points

function NoteIcon(props: {
  left: number;
  top: number;
  size: number;
  draft?: boolean;
  onClick: (e: React.MouseEvent) => void;
}) {
  return (
    <div
      className={"note-icon" + (props.draft ? " draft" : "")}
      style={{ left: props.left, top: props.top, width: props.size, height: props.size }}
      onClick={props.onClick}
      title={props.draft ? "Note (unsaved)" : "Note"}
    >
      <svg width="68%" height="68%" viewBox="0 0 24 24" fill="none" stroke="currentColor"
        strokeWidth="2.4" strokeLinejoin="round">
        <path d="M21 15a2 2 0 0 1-2 2H7l-4 4V5a2 2 0 0 1 2-2h14a2 2 0 0 1 2 2z" />
      </svg>
    </div>
  );
}

function NoteEditor(props: {
  note: Note;
  tabId: string;
  left: number;
  top: number;
  onDone: () => void;
}) {
  const [text, setText] = useState(props.note.text);
  const save = () => {
    if (text.trim()) updateNote(props.tabId, props.note.id, text);
    else deleteNote(props.tabId, props.note.id);
    props.onDone();
  };
  return (
    <div className="note-popover" style={{ left: props.left, top: props.top }}>
      <textarea
        autoFocus
        value={text}
        placeholder="Write a note…"
        onChange={(e) => setText(e.target.value)}
        onKeyDown={(e) => {
          if (e.key === "Enter" && !e.shiftKey) {
            e.preventDefault();
            save();
          }
        }}
      />
      <div className="row">
        <button
          onClick={() => {
            deleteNote(props.tabId, props.note.id);
            props.onDone();
          }}
        >
          Delete
        </button>
        <button className="btn-primary" onClick={save}>
          Save
        </button>
      </div>
    </div>
  );
}

function NoteCard(props: { author: string; contents: string; left: number; top: number }) {
  return (
    <div className="note-popover" style={{ left: props.left, top: props.top }}>
      {props.author && <div className="note-author">{props.author}</div>}
      <div style={{ whiteSpace: "pre-wrap", userSelect: "text", WebkitUserSelect: "text" }}>
        {props.contents || "(no text)"}
      </div>
    </div>
  );
}

interface PdfPageProps {
  entry: DocEntry;
  spec: PageSpec;
  index: number;
  zoom: number;
  w: number;
  h: number;
  searchQuery: string;
  highlights: Highlight[];
  notes: Note[];
  tabId: string;
  openNote: string | null;
  setOpenNote: (id: string | null) => void;
  requestClose: () => void;
}

function PdfPage({
  entry, spec, index, zoom, w, h, searchQuery, highlights,
  notes, tabId, openNote, setOpenNote, requestClose,
}: PdfPageProps) {
  const wrapRef = useRef<HTMLDivElement>(null);
  const canvasRef = useRef<HTMLCanvasElement>(null);
  const textRef = useRef<HTMLDivElement>(null);
  const [visible, setVisible] = useState(false);
  const [viewport, setViewport] = useState<PageViewport | null>(null);
  const [annots, setAnnots] = useState<ExistingAnnot[]>([]);

  useEffect(() => {
    const el = wrapRef.current!;
    const io = new IntersectionObserver(
      ([e]) => setVisible((v) => v || e.isIntersecting),
      { rootMargin: "900px 0px" },
    );
    io.observe(el);
    return () => io.disconnect();
  }, []);

  useEffect(() => {
    if (!visible || viewport) return;
    let cancelled = false;
    (async () => {
      const page = await getPage(entry, spec.src);
      const rotation = (page.rotate + spec.rotation + 360) % 360;
      const vp = page.getViewport({ scale: zoom, rotation });
      const canvas = canvasRef.current;
      if (!canvas || cancelled) return;
      const dpr = Math.min(window.devicePixelRatio || 1, 2);
      canvas.width = Math.floor(vp.width * dpr);
      canvas.height = Math.floor(vp.height * dpr);
      await page.render({
        canvas,
        viewport: vp,
        transform: dpr !== 1 ? [dpr, 0, 0, dpr, 0, 0] : undefined,
      }).promise;
      if (cancelled) return;
      const textDiv = textRef.current;
      if (textDiv) {
        textDiv.innerHTML = "";
        const tl = new TextLayer({
          textContentSource: page.streamTextContent(),
          container: textDiv,
          viewport: vp,
        });
        await tl.render();
      }
      if (!cancelled) setViewport(vp);
    })().catch((e) => console.error("render page", spec.src, e));
    return () => {
      cancelled = true;
    };
  }, [visible]);

  useEffect(() => {
    if (!viewport) return;
    let on = true;
    getAnnots(entry, spec.src)
      .then((a) => on && setAnnots(a))
      .catch((e) => report("Could not read annotations", e));
    return () => {
      on = false;
    };
  }, [viewport]);

  useEffect(() => {
    const el = textRef.current;
    if (!el || !viewport) return;
    const q = searchQuery.trim().toLowerCase();
    el.querySelectorAll("span").forEach((sp) => {
      const s = sp as HTMLElement & { __orig?: string; __marked?: boolean };
      const orig = s.__orig ?? sp.textContent ?? "";
      s.__orig = orig;
      if (q && orig.toLowerCase().includes(q)) {
        sp.innerHTML = markText(orig, q);
        s.__marked = true;
      } else if (s.__marked) {
        sp.textContent = orig;
        s.__marked = false;
      }
    });
  }, [searchQuery, viewport]);

  const iconSize = Math.max(14, NOTE_PT * zoom);

  const renderPopover = () => {
    if (!viewport || !openNote) return null;
    const dn = notes.find((n) => n.id === openNote);
    if (dn) {
      const p = vpTopLeft(viewport, dn.x, dn.y, dn.x + NOTE_PT, dn.y - NOTE_PT);
      const pos = popoverPos(p.left, p.top, iconSize, w, h);
      return (
        <NoteEditor
          key={dn.id}
          note={dn}
          tabId={tabId}
          left={pos.left}
          top={pos.top}
          onDone={() => setOpenNote(null)}
        />
      );
    }
    if (openNote.startsWith(`ex:${spec.src}:`)) {
      const ai = Number(openNote.split(":")[2]);
      const a = annots[ai];
      if (a && a.subtype === "Text") {
        const p = vpTopLeft(viewport, a.rect[0], a.rect[1], a.rect[2], a.rect[3]);
        const pos = popoverPos(p.left, p.top, iconSize, w, h);
        return <NoteCard author={a.author} contents={a.contents} left={pos.left} top={pos.top} />;
      }
    }
    return null;
  };

  const toggle = (id: string) => {
    if (openNote === id) requestClose();
    else setOpenNote(id);
  };

  return (
    <div
      className="page-wrap"
      ref={wrapRef}
      data-pageindex={index}
      style={{ width: w, height: h }}
    >
      <canvas ref={canvasRef} style={{ width: w, height: h }} />
      {viewport &&
        highlights.flatMap((hl, hi) =>
          hl.rects.map((r, ri) => {
            const [x1, y1] = viewport.convertToViewportPoint(r[0], r[1]) as [number, number];
            const [x2, y2] = viewport.convertToViewportPoint(r[2], r[3]) as [number, number];
            return (
              <div
                key={`${hi}-${ri}`}
                className="hl"
                style={{
                  left: Math.min(x1, x2),
                  top: Math.min(y1, y2),
                  width: Math.abs(x2 - x1),
                  height: Math.abs(y2 - y1),
                  background: `rgb(${hl.color.map((c) => Math.round(c * 255)).join(",")})`,
                }}
              />
            );
          }),
        )}
      {viewport &&
        annots.flatMap((a, ai) =>
          a.subtype === "Highlight" && !a.hasAppearance
            ? a.quads.map((q, qi) => {
                const [x1, y1] = viewport.convertToViewportPoint(q[0], q[1]) as [number, number];
                const [x2, y2] = viewport.convertToViewportPoint(q[2], q[3]) as [number, number];
                return (
                  <div
                    key={`ex-${ai}-${qi}`}
                    className="hl"
                    style={{
                      left: Math.min(x1, x2),
                      top: Math.min(y1, y2),
                      width: Math.abs(x2 - x1),
                      height: Math.abs(y2 - y1),
                      background: a.cssColor,
                    }}
                  />
                );
              })
            : [],
        )}
      <div
        className="textLayer"
        ref={textRef}
        style={{ "--scale-factor": zoom, "--total-scale-factor": zoom } as React.CSSProperties}
      />
      {viewport &&
        annots.map((a, ai) => {
          if (a.subtype !== "Text") return null;
          const p = vpTopLeft(viewport, a.rect[0], a.rect[1], a.rect[2], a.rect[3]);
          return (
            <NoteIcon
              key={`exic-${ai}`}
              left={p.left}
              top={p.top}
              size={iconSize}
              onClick={() => toggle(`ex:${spec.src}:${ai}`)}
            />
          );
        })}
      {viewport &&
        notes.map((n) => {
          const p = vpTopLeft(viewport, n.x, n.y, n.x + NOTE_PT, n.y - NOTE_PT);
          return (
            <NoteIcon
              key={n.id}
              left={p.left}
              top={p.top}
              size={iconSize}
              draft
              onClick={() => toggle(n.id)}
            />
          );
        })}
      {renderPopover()}
    </div>
  );
}

export default function Viewer() {
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

  useEffect(() => {
    let on = true;
    setEntry(null);
    setOpenNote(null);
    if (!tab) return;
    getDoc(tab.id, tab.path, tab.docVersion)
      .then((e) => on && setEntry(e))
      .catch((e) => report("Load failed", e));
    return () => {
      on = false;
    };
  }, [tabId, docVersion]);

  useEffect(() => {
    if (!scrollTarget) return;
    slotRefs.current[scrollTarget.page - 1]?.scrollIntoView({ block: "start" });
  }, [scrollTarget?.nonce]);

  useEffect(() => {
    const onFit = () => {
      const t = useActiveTabNow();
      if (!t || !entry || !containerRef.current) return;
      const spec = t.pages[t.currentPage - 1] ?? t.pages[0];
      if (!spec) return;
      const base = entry.dims[spec.src - 1];
      const swap = spec.rotation % 180 !== 0;
      const bw = swap ? base.h : base.w;
      const zoom = Math.max(0.25, Math.min(6, (containerRef.current.clientWidth - 56) / bw));
      useStore.getState().patchTab(t.id, { zoom: Math.round(zoom * 100) / 100 });
    };
    window.addEventListener("fitwidth", onFit);
    return () => window.removeEventListener("fitwidth", onFit);
  }, [entry]);

  // Ctrl/Cmd + wheel zoom; must be non-passive to preventDefault.
  useEffect(() => {
    const c = containerRef.current;
    if (!c) return;
    const onWheel = (e: WheelEvent) => {
      if (!e.ctrlKey && !e.metaKey) return;
      e.preventDefault();
      const t = useActiveTabNow();
      if (!t) return;
      const factor = e.deltaY < 0 ? 1.1 : 1 / 1.1;
      const zoom = Math.max(0.25, Math.min(6, Math.round(t.zoom * factor * 100) / 100));
      if (zoom !== t.zoom) useStore.getState().patchTab(t.id, { zoom });
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

  const closePopover = () => {
    const id = openNoteRef.current;
    if (id && !id.startsWith("ex:")) {
      const t = useActiveTabNow();
      const n = t?.notes.find((x) => x.id === id);
      if (t && n && n.text.trim() === "") deleteNote(t.id, n.id);
    }
    setOpenNote(null);
  };

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
    if (st.tool !== "note") return;
    const target = e.target as Element;
    if (target.closest(".note-icon, .note-popover")) return;
    const wrap = target.closest<HTMLElement>("[data-pageindex]");
    const t = useActiveTabNow();
    if (!wrap || !t || !entry) return;
    const index = Number(wrap.dataset.pageindex);
    const spec = t.pages[index];
    if (!spec) return;
    try {
      const page = await getPage(entry, spec.src);
      const rotation = (page.rotate + spec.rotation + 360) % 360;
      const viewport = page.getViewport({ scale: t.zoom, rotation });
      const box = wrap.getBoundingClientRect();
      const [px, py] = viewport.convertToPdfPoint(
        e.clientX - box.left,
        e.clientY - box.top,
      ) as [number, number];
      const note: Note = { id: crypto.randomUUID(), src: spec.src, x: px, y: py, text: "" };
      addNote(t.id, note);
      setOpenNote(note.id);
    } catch (err) {
      report("Could not add note", err);
    }
    st.setTool("select");
  };

  const onScroll = () => {
    if (ticking.current) return;
    ticking.current = true;
    requestAnimationFrame(() => {
      ticking.current = false;
      const t = useActiveTabNow();
      const c = containerRef.current;
      if (!t || !c) return;
      scrollRatio.current = (c.scrollTop + c.clientHeight / 2) / Math.max(1, c.scrollHeight);
      const refTop = c.getBoundingClientRect().top + 10;
      let current = 1;
      slotRefs.current.forEach((el, i) => {
        if (el && el.getBoundingClientRect().top <= refTop + c.clientHeight / 3) current = i + 1;
      });
      if (current !== t.currentPage) useStore.getState().patchTab(t.id, { currentPage: current });
    });
  };

  if (!tab) {
    return (
      <div className="viewer welcome">
        <div className="welcome-box">
          <svg width="56" height="56" viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="1.2">
            <path d="M14 2H6a2 2 0 0 0-2 2v16a2 2 0 0 0 2 2h12a2 2 0 0 0 2-2V8z" />
            <path d="M14 2v6h6" />
          </svg>
          <h2>Open a PDF to get started</h2>
          <p>Drag &amp; drop files anywhere, or</p>
          <button className="btn-primary" onClick={() => openFileDialog()}>
            Open PDF…
          </button>
        </div>
      </div>
    );
  }

  slotRefs.current.length = tab.pages.length;

  return (
    <div
      className={"viewer" + (tool === "note" ? " note-mode" : "")}
      ref={containerRef}
      onScroll={onScroll}
      onClick={tool === "note" ? onViewerClick : undefined}
    >
      {!entry && <div className="loading">Loading…</div>}
      {entry && (
        <div className="pages">
          {tab.pages.map((spec, i) => {
            const base = entry.dims[spec.src - 1];
            if (!base) return null;
            const swap = spec.rotation % 180 !== 0;
            const w = (swap ? base.h : base.w) * tab.zoom;
            const h = (swap ? base.w : base.h) * tab.zoom;
            return (
              <div
                className="page-slot"
                key={`${spec.src}:${spec.rotation}:${tab.zoom}:${entry.version}`}
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
                  highlights={tab.highlights.filter((hl) => hl.src === spec.src)}
                  notes={tab.notes.filter((n) => n.src === spec.src)}
                  tabId={tab.id}
                  openNote={openNote}
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

function useActiveTabNow() {
  const s = useStore.getState();
  return s.tabs.find((t) => t.id === s.activeId) ?? null;
}
