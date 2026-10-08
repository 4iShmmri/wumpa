import { useEffect, useLayoutEffect, useRef, useState } from "react";
import { TextLayer } from "pdfjs-dist";
import type { PageViewport } from "pdfjs-dist";
import { useStore, useActiveTab, type Tool } from "../store";
import { getDoc, getPage, getAnnots, docPath, type DocEntry, type ExistingAnnot } from "../pdf";
import type { PageSpec, Highlight, Note, ObjDesc, ObjectEdit, StampItem } from "../types";
import {
  openFileDialog, addNote, updateNote, deleteNote, addObjectEdits, deleteObject,
} from "../actions";
import { addTextStamp, addSignatureStamp, loadImage } from "../stamps";
import StampLayer from "./StampLayer";
import { getObjects, adjustedBox } from "../objects";
import { confirmDialog } from "../backend";
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

const objErrOnce = new Set<string>();

interface CssBox {
  l: number;
  t: number;
  w: number;
  h: number;
}

interface ObjBox extends CssBox {
  o: ObjDesc;
  pdf: [number, number, number, number];
}

function ObjectLayer({
  tabId, tabPath, spec, viewport, edits, tool,
}: {
  tabId: string;
  tabPath: string;
  spec: PageSpec;
  viewport: PageViewport;
  edits: ObjectEdit[];
  tool: "edit" | "erase";
}) {
  const [objs, setObjs] = useState<ObjDesc[] | null>(null);
  const objSel = useStore((s) => s.objSel);
  const layerRef = useRef<HTMLDivElement>(null);
  const [ghost, setGhost] = useState<CssBox | null>(null);
  const [eraseRect, setEraseRect] = useState<CssBox | null>(null);

  useEffect(() => {
    let on = true;
    getObjects(tabId, tabPath, spec.src)
      .then((o) => on && setObjs(o))
      .catch((e) => {
        const k = `${tabId}:${spec.src}`;
        if (!objErrOnce.has(k)) {
          objErrOnce.add(k);
          report("Object editing unavailable on this page", e);
        }
      });
    return () => {
      on = false;
    };
  }, [tabId, tabPath, spec.src]);

  if (!objs) return null;

  const boxes: ObjBox[] = objs
    .flatMap((o) => {
      const b = adjustedBox(o, edits, spec.src);
      if (!b) return [];
      const [ax, ay] = viewport.convertToViewportPoint(b[0], b[1]) as [number, number];
      const [bx, by] = viewport.convertToViewportPoint(b[2], b[3]) as [number, number];
      return [{
        o, pdf: b,
        l: Math.min(ax, bx), t: Math.min(ay, by),
        w: Math.abs(bx - ax), h: Math.abs(by - ay),
      }];
    })
    .sort((a, b) => b.w * b.h - a.w * a.h);

  const toPdf = (clientX: number, clientY: number) => {
    const r = layerRef.current!.getBoundingClientRect();
    return viewport.convertToPdfPoint(clientX - r.left, clientY - r.top) as [number, number];
  };

  const track = (
    onMove: (ev: PointerEvent) => void,
    onUp: (ev: PointerEvent) => void,
  ) => {
    const up = (ev: PointerEvent) => {
      window.removeEventListener("pointermove", onMove);
      window.removeEventListener("pointerup", up);
      onUp(ev);
    };
    window.addEventListener("pointermove", onMove);
    window.addEventListener("pointerup", up);
  };

  const startMove = (e: React.PointerEvent, box: ObjBox) => {
    if (tool !== "edit" || e.button !== 0) return;
    e.stopPropagation();
    useStore.getState().setObjSel({ src: spec.src, id: box.o.id });
    const sx = e.clientX;
    const sy = e.clientY;
    let moved = false;
    track(
      (ev) => {
        const dx = ev.clientX - sx;
        const dy = ev.clientY - sy;
        if (!moved && Math.hypot(dx, dy) > 2) moved = true;
        if (moved) setGhost({ l: box.l + dx, t: box.t + dy, w: box.w, h: box.h });
      },
      (ev) => {
        setGhost(null);
        if (!moved) return;
        const [px0, py0] = toPdf(sx, sy);
        const [px1, py1] = toPdf(ev.clientX, ev.clientY);
        addObjectEdits(tabId, [
          { src: spec.src, id: box.o.id, action: "translate", dx: px1 - px0, dy: py1 - py0 },
        ]);
      },
    );
  };

  const startScale = (e: React.PointerEvent, box: ObjBox, corner: number) => {
    if (e.button !== 0) return;
    e.stopPropagation();
    const cssCorners: [number, number][] = [
      [box.l, box.t], [box.l + box.w, box.t],
      [box.l, box.t + box.h], [box.l + box.w, box.t + box.h],
    ];
    const opp = cssCorners[3 - corner];
    const orig = cssCorners[corner];
    const r = layerRef.current!.getBoundingClientRect();
    const anchorPdf = viewport.convertToPdfPoint(opp[0], opp[1]) as [number, number];
    const origPdf = viewport.convertToPdfPoint(orig[0], orig[1]) as [number, number];
    track(
      (ev) => {
        const cl = ev.clientX - r.left;
        const ct = ev.clientY - r.top;
        setGhost({
          l: Math.min(cl, opp[0]), t: Math.min(ct, opp[1]),
          w: Math.abs(cl - opp[0]), h: Math.abs(ct - opp[1]),
        });
      },
      (ev) => {
        setGhost(null);
        const [nx, ny] = toPdf(ev.clientX, ev.clientY);
        const clampS = (v: number) => Math.max(0.05, Math.min(20, v));
        const sx = clampS((nx - anchorPdf[0]) / (origPdf[0] - anchorPdf[0] || 1e-6));
        const sy = clampS((ny - anchorPdf[1]) / (origPdf[1] - anchorPdf[1] || 1e-6));
        addObjectEdits(tabId, [
          { src: spec.src, id: box.o.id, action: "scale", sx, sy, ax: anchorPdf[0], ay: anchorPdf[1] },
        ]);
      },
    );
  };

  const startErase = (e: React.PointerEvent) => {
    if (tool !== "erase" || e.button !== 0) return;
    const r = layerRef.current!.getBoundingClientRect();
    const sx = e.clientX;
    const sy = e.clientY;
    track(
      (ev) => {
        setEraseRect({
          l: Math.min(ev.clientX, sx) - r.left, t: Math.min(ev.clientY, sy) - r.top,
          w: Math.abs(ev.clientX - sx), h: Math.abs(ev.clientY - sy),
        });
      },
      async (ev) => {
        setEraseRect(null);
        if (Math.abs(ev.clientX - sx) < 3 && Math.abs(ev.clientY - sy) < 3) return;
        const [px0, py0] = toPdf(sx, sy);
        const [px1, py1] = toPdf(ev.clientX, ev.clientY);
        const rx1 = Math.min(px0, px1);
        const rx2 = Math.max(px0, px1);
        const ry1 = Math.min(py0, py1);
        const ry2 = Math.max(py0, py1);
        const hits = boxes.filter(
          (b) => !(b.pdf[2] < rx1 || b.pdf[0] > rx2 || b.pdf[3] < ry1 || b.pdf[1] > ry2),
        );
        if (!hits.length) {
          useStore.getState().setStatus("No objects in that area");
          return;
        }
        if (await confirmDialog(`Delete ${hits.length} object(s)?`)) {
          addObjectEdits(
            tabId,
            hits.map((b) => ({ src: spec.src, id: b.o.id, action: "delete" as const })),
          );
        }
      },
    );
  };

  const sel =
    objSel && objSel.src === spec.src ? boxes.find((b) => b.o.id === objSel.id) ?? null : null;

  return (
    <div
      ref={layerRef}
      className="obj-layer"
      style={{ pointerEvents: tool === "erase" ? "auto" : "none" }}
      onPointerDown={tool === "erase" ? startErase : undefined}
    >
      {tool === "edit" &&
        boxes.map((b) => (
          <div
            key={b.o.id}
            className={"obj-box" + (sel?.o.id === b.o.id ? " selected" : "")}
            style={{ left: b.l, top: b.t, width: b.w, height: b.h, pointerEvents: "auto" }}
            onPointerDown={(e) => startMove(e, b)}
          />
        ))}
      {tool === "edit" && sel && (sel.o.kind === "image" || sel.o.kind === "form") &&
        [0, 1, 2, 3].map((c) => (
          <div
            key={c}
            className="obj-handle"
            style={{
              left: (c % 2 ? sel.l + sel.w : sel.l) - 4,
              top: (c > 1 ? sel.t + sel.h : sel.t) - 4,
              cursor: c === 0 || c === 3 ? "nwse-resize" : "nesw-resize",
              pointerEvents: "auto",
            }}
            onPointerDown={(e) => startScale(e, sel, c)}
          />
        ))}
      {tool === "edit" && sel && (
        <div
          className="obj-actions"
          style={{ left: sel.l, top: Math.max(0, sel.t - 30), pointerEvents: "auto" }}
        >
          <button onClick={() => deleteObject(tabId, spec.src, sel.o.id)}>Delete</button>
        </div>
      )}
      {ghost && (
        <div
          className="erase-rect"
          style={{ left: ghost.l, top: ghost.t, width: ghost.w, height: ghost.h }}
        />
      )}
      {eraseRect && (
        <div
          className="erase-rect"
          style={{ left: eraseRect.l, top: eraseRect.t, width: eraseRect.w, height: eraseRect.h }}
        />
      )}
    </div>
  );
}

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
  stamps: StampItem[];
  tabId: string;
  tabPath: string;
  edits: ObjectEdit[];
  tool: Tool;
  openNote: string | null;
  setOpenNote: (id: string | null) => void;
  requestClose: () => void;
}

function PdfPage({
  entry, spec, index, zoom, w, h, searchQuery, highlights,
  notes, stamps, tabId, tabPath, edits, tool, openNote, setOpenNote, requestClose,
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
      {viewport && stamps.length > 0 && (
        <StampLayer
          tabId={tabId}
          src={spec.src}
          stamps={stamps}
          viewport={viewport}
          zoom={zoom}
        />
      )}
      {viewport && (tool === "edit" || tool === "erase") && (
        <ObjectLayer
          tabId={tabId}
          tabPath={tabPath}
          spec={spec}
          viewport={viewport}
          edits={edits}
          tool={tool}
        />
      )}
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
    getDoc(tab.id, docPath(tab), tab.docVersion)
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
    const tool = st.tool;
    if (tool !== "note" && tool !== "text" && tool !== "sign") return;
    const target = e.target as Element;
    if (target.closest(".note-icon, .note-popover, .stamp, .stamp-bar, .stamp-edit, .stamp-handle")) return;
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
      if (tool === "note") {
        const note: Note = { id: crypto.randomUUID(), src: spec.src, x: px, y: py, text: "" };
        addNote(t.id, note);
        setOpenNote(note.id);
      } else if (tool === "text") {
        const stamp = addTextStamp(t.id, spec.src, px, py);
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
          const stamp = addSignatureStamp(
            t.id, spec.src, px, py, sig, img.width / Math.max(1, img.height),
          );
          st.setStampSel({ src: spec.src, id: stamp.id });
        }
      }
    } catch (err) {
      report("Could not place item", err);
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
      className={
        "viewer" +
        (tool === "note" ? " note-mode" : "") +
        (tool === "edit" ? " edit-mode" : "") +
        (tool === "erase" ? " erase-mode" : "") +
        (tool === "text" ? " text-mode" : "") +
        (tool === "sign" ? " sign-mode" : "")
      }
      ref={containerRef}
      onScroll={onScroll}
      onClick={tool === "note" || tool === "text" || tool === "sign" ? onViewerClick : undefined}
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
                  stamps={tab.stamps.filter((st) => st.src === spec.src)}
                  tabId={tab.id}
                  tabPath={tab.path}
                  edits={tab.edits}
                  tool={tool}
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
