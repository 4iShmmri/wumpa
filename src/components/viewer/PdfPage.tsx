import { memo, useEffect, useMemo, useRef, useState } from "react";
import { TextLayer } from "pdfjs-dist";
import type { PageViewport } from "pdfjs-dist";
import { useStore, type Tool } from "../../store";
import { getPage, getAnnots, pageRotation, type DocEntry, type ExistingAnnot } from "../../pdf";
import type {
  PageSpec,
  Highlight,
  Note,
  ObjectEdit,
  StampItem,
  ShapeItem,
  LinkItem,
  LinkRemoval,
} from "../../types";
import StampLayer from "../StampLayer";
import ShapeLayer from "../ShapeLayer";
import LinkLayer from "../LinkLayer";
import ObjectLayer from "./ObjectLayer";
import NotePopover, { NoteIcon, NOTE_PT, vpTopLeft } from "./NotePopover";
import { blankViewport } from "./viewport";
import { useT } from "../../i18n";
import { report } from "../../errors";
import { scanQr } from "../../qr/scan";
import { sinceScroll } from "../../scrollState";

function esc(s: string) {
  return s.replace(/[&<>"]/g, (c) => ({ "&": "&amp;", "<": "&lt;", ">": "&gt;", '"': "&quot;" })[c]!);
}

function markText(orig: string, q: string): string {
  const lower = orig.toLowerCase();
  if (lower.length !== orig.length) return esc(orig);
  let out = "";
  let pos = 0;
  let i;
  while ((i = lower.indexOf(q, pos)) !== -1) {
    out += esc(orig.slice(pos, i)) + "<mark>" + esc(orig.slice(i, i + q.length)) + "</mark>";
    pos = i + q.length;
  }
  return out + esc(orig.slice(pos));
}

interface QrHit {
  data: string;
  box: [number, number, number, number]; // normalized to the rendered page
}
const qrCache = new Map<string, QrHit | null>();

export interface PdfPageProps {
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
  shapes: ShapeItem[];
  links: LinkItem[];
  removals: LinkRemoval[];
  tabId: string;
  tabPath: string;
  edits: ObjectEdit[];
  tool: Tool;
  openNote: string | null; // only set when the open note belongs to this page
  setOpenNote: (id: string | null) => void;
  requestClose: () => void;
}

// One page: canvas + text layer, QR detection and every overlay layer.
// Memoized so scrolling (which patches currentPage) does not re-render pages
// whose props are unchanged; the container keeps each prop referentially
// stable for that reason.
function PdfPage({
  entry,
  spec,
  index,
  zoom,
  w,
  h,
  searchQuery,
  highlights,
  notes,
  stamps,
  shapes,
  links,
  removals,
  tabId,
  tabPath,
  edits,
  tool,
  openNote,
  setOpenNote,
  requestClose,
}: PdfPageProps) {
  const t = useT();
  const wrapRef = useRef<HTMLDivElement>(null);
  const canvasRef = useRef<HTMLCanvasElement>(null);
  const textRef = useRef<HTMLDivElement>(null);
  const [visible, setVisible] = useState(false);
  const [rendered, setRendered] = useState<PageViewport | null>(null);
  const [annots, setAnnots] = useState<ExistingAnnot[]>([]);
  const qrKey = `${tabId}:${spec.src}:${entry.version}:${spec.rotation}`;
  const [qrHit, setQrHit] = useState<QrHit | null>(() => qrCache.get(qrKey) ?? null);
  const [textReady, setTextReady] = useState(0);
  const blank = spec.src <= 0;

  // Blank pages have nothing to render, so their viewport is built directly
  // from the spec; source pages get theirs once pdf.js has drawn them.
  const blankVp = useMemo(
    () => (blank ? blankViewport(spec, zoom) : null),
    [blank, spec.w, spec.h, spec.rotation, zoom],
  );
  const viewport = blank ? blankVp : rendered;

  useEffect(() => {
    const el = wrapRef.current!;
    const io = new IntersectionObserver(([e]) => setVisible((v) => v || e.isIntersecting), {
      rootMargin: "900px 0px",
    });
    io.observe(el);
    return () => io.disconnect();
  }, []);

  // Renders the page at the current zoom and rotation; a render still running
  // when either changes is cancelled so the worker is not flooded.
  useEffect(() => {
    if (!visible || blank) return;
    let cancelled = false;
    let task: { cancel: () => void } | null = null;
    let textLayer: TextLayer | null = null;
    (async () => {
      const page = await getPage(entry, spec.src);
      const vp = page.getViewport({ scale: zoom, rotation: pageRotation(page.rotate, spec.rotation) });
      const canvas = canvasRef.current;
      if (!canvas || cancelled) return;
      const dpr = Math.min(window.devicePixelRatio || 1, 2);
      canvas.width = Math.floor(vp.width * dpr);
      canvas.height = Math.floor(vp.height * dpr);
      const render = page.render({
        canvas,
        viewport: vp,
        transform: dpr !== 1 ? [dpr, 0, 0, dpr, 0, 0] : undefined,
      });
      task = render;
      await render.promise;
      if (cancelled) return;
      setRendered(vp);
      // The text layer is only needed for selection and search marks; it waits
      // for a pause in scrolling so fast scrolls stay smooth.
      while (sinceScroll() < 150) {
        await new Promise((r) => setTimeout(r, 150 - sinceScroll()));
        if (cancelled) return;
      }
      const textDiv = textRef.current;
      if (textDiv) {
        textDiv.innerHTML = "";
        textLayer = new TextLayer({
          textContentSource: page.streamTextContent(),
          container: textDiv,
          viewport: vp,
        });
        await textLayer.render();
        if (!cancelled) setTextReady((n) => n + 1);
      }
    })().catch((e) => {
      if (!cancelled) console.error("render page", spec.src, e);
    });
    return () => {
      cancelled = true;
      task?.cancel();
      textLayer?.cancel();
    };
  }, [visible, zoom, spec.rotation, entry]);

  useEffect(() => {
    if (!viewport || blank) return;
    let on = true;
    getAnnots(entry, spec.src)
      .then((a) => on && setAnnots(a))
      .catch((e) => report(t("stReadAnnots"), e));
    return () => {
      on = false;
    };
  }, [viewport]);

  useEffect(() => {
    if (!viewport || blank || qrCache.has(qrKey)) return;
    let cancelled = false;
    const timer = window.setTimeout(() => {
      const canvas = canvasRef.current;
      if (!canvas?.width) return;
      const scale = Math.min(1, 1200 / Math.max(canvas.width, canvas.height));
      const off = document.createElement("canvas");
      off.width = Math.max(1, Math.round(canvas.width * scale));
      off.height = Math.max(1, Math.round(canvas.height * scale));
      const ctx = off.getContext("2d")!;
      ctx.drawImage(canvas, 0, 0, off.width, off.height);
      void scanQr(ctx.getImageData(0, 0, off.width, off.height)).then((code) => {
        if (cancelled) return;
        const hit: QrHit | null = code
          ? {
              data: code.data,
              box: [
                Math.min(...code.corners.map((p) => p[0])) / off.width,
                Math.min(...code.corners.map((p) => p[1])) / off.height,
                Math.max(...code.corners.map((p) => p[0])) / off.width,
                Math.max(...code.corners.map((p) => p[1])) / off.height,
              ],
            }
          : null;
        if (qrCache.size > 300) qrCache.clear();
        qrCache.set(qrKey, hit);
        setQrHit(hit);
      });
    }, 100);
    return () => {
      cancelled = true;
      window.clearTimeout(timer);
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
  }, [searchQuery, viewport, textReady]);

  const iconSize = Math.max(14, NOTE_PT * zoom);

  const toggle = (id: string) => {
    if (openNote === id) requestClose();
    else setOpenNote(id);
  };

  return (
    <div
      className={"page-wrap" + (blank ? " blank" : "")}
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
      {viewport && (shapes.length > 0 || tool === "draw") && (
        <ShapeLayer
          tabId={tabId}
          src={spec.src}
          shapes={shapes}
          viewport={viewport}
          zoom={zoom}
          active={tool === "draw"}
        />
      )}
      {viewport && (
        <LinkLayer
          entry={entry}
          src={spec.src}
          viewport={viewport}
          w={w}
          h={h}
          active={tool === "link"}
          links={links}
          removals={removals}
        />
      )}
      <div
        className="textLayer"
        ref={textRef}
        style={{ "--scale-factor": zoom, "--total-scale-factor": zoom } as React.CSSProperties}
      />
      {viewport && qrHit && (
        <div
          className="qr-spot"
          data-tip={t("tipQrOnPage")}
          style={{
            left: qrHit.box[0] * w - 4,
            top: qrHit.box[1] * h - 4,
            width: (qrHit.box[2] - qrHit.box[0]) * w + 8,
            height: (qrHit.box[3] - qrHit.box[1]) * h + 8,
            pointerEvents: tool === "select" ? "auto" : "none",
          }}
          onClick={() => {
            const st = useStore.getState();
            st.setQrPreset(qrHit.data);
            st.setUtilModal("qr");
          }}
        >
          <span className="qr-badge">
            <svg
              width="10"
              height="10"
              viewBox="0 0 24 24"
              fill="none"
              stroke="currentColor"
              strokeWidth="2.6"
            >
              <path d="M3 3h6v6H3z M15 3h6v6h-6z M3 15h6v6H3z M15 15h3v3h-3z M21 21h-3 M21 15v3" />
            </svg>
          </span>
        </div>
      )}
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
        <StampLayer tabId={tabId} src={spec.src} stamps={stamps} viewport={viewport} zoom={zoom} />
      )}
      {viewport && !blank && (tool === "edit" || tool === "erase") && (
        <ObjectLayer
          tabId={tabId}
          tabPath={tabPath}
          spec={spec}
          viewport={viewport}
          edits={edits}
          tool={tool}
          entry={entry}
        />
      )}
      {viewport && (
        <NotePopover
          openNote={openNote}
          notes={notes}
          annots={annots}
          src={spec.src}
          viewport={viewport}
          iconSize={iconSize}
          w={w}
          h={h}
          tabId={tabId}
          onDone={() => setOpenNote(null)}
        />
      )}
    </div>
  );
}

export default memo(PdfPage);
