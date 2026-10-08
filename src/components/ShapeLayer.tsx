import { useState } from "react";
import type { PageViewport } from "pdfjs-dist";
import { useStore } from "../store";
import type { ShapeItem, ShapeKind } from "../types";
import { addShape, moveShape, deleteShape } from "../actions";

type Pt = [number, number];

const css = (c: [number, number, number]) => `rgb(${c.map((v) => Math.round(v * 255)).join(",")})`;

function track(onMove: (ev: PointerEvent) => void, onUp: (ev: PointerEvent) => void) {
  const up = (ev: PointerEvent) => {
    window.removeEventListener("pointermove", onMove);
    window.removeEventListener("pointerup", up);
    onUp(ev);
  };
  window.addEventListener("pointermove", onMove);
  window.addEventListener("pointerup", up);
}

function headPts(a: Pt, b: Pt, len: number): { base: Pt; tri: Pt[] } {
  const dx = b[0] - a[0];
  const dy = b[1] - a[1];
  const d = Math.hypot(dx, dy) || 1;
  const ux = dx / d;
  const uy = dy / d;
  const bx = b[0] - ux * len;
  const by = b[1] - uy * len;
  const hw = len * 0.45;
  return {
    base: [bx, by],
    tri: [b, [bx - uy * hw, by + ux * hw], [bx + uy * hw, by - ux * hw]],
  };
}

function ShapeSvg({ kind, strokes, color, w, headLen }: {
  kind: ShapeKind;
  strokes: Pt[][];
  color: string;
  w: number;
  headLen: number;
}) {
  const common = {
    stroke: color,
    strokeWidth: w,
    fill: "none",
    strokeLinecap: "round" as const,
    strokeLinejoin: "round" as const,
  };
  if (kind === "ink") {
    return (
      <>
        {strokes.map((pts, i) => (
          <path key={i} {...common} d={pts.map((p, j) => `${j ? "L" : "M"}${p[0]} ${p[1]}`).join(" ")} />
        ))}
      </>
    );
  }
  const a = strokes[0]?.[0];
  const b = strokes[0]?.[1];
  if (!a || !b) return null;
  if (kind === "line") return <line {...common} x1={a[0]} y1={a[1]} x2={b[0]} y2={b[1]} />;
  if (kind === "arrow") {
    const { base, tri } = headPts(a, b, headLen);
    return (
      <>
        <line {...common} x1={a[0]} y1={a[1]} x2={base[0]} y2={base[1]} />
        <polygon points={tri.map((p) => p.join(",")).join(" ")} fill={color} stroke="none" />
      </>
    );
  }
  const x = Math.min(a[0], b[0]);
  const y = Math.min(a[1], b[1]);
  const bw = Math.abs(b[0] - a[0]);
  const bh = Math.abs(b[1] - a[1]);
  if (kind === "rect") return <rect {...common} x={x} y={y} width={bw} height={bh} />;
  return <ellipse {...common} cx={x + bw / 2} cy={y + bh / 2} rx={bw / 2} ry={bh / 2} />;
}

export default function ShapeLayer({ tabId, src, shapes, viewport, zoom, active }: {
  tabId: string;
  src: number;
  shapes: ShapeItem[];
  viewport: PageViewport;
  zoom: number;
  active: boolean;
}) {
  const shapeSel = useStore((s) => s.shapeSel);
  const [draft, setDraft] = useState<{ kind: ShapeKind; pts: Pt[] } | null>(null);
  const [ghost, setGhost] = useState<{ id: string; dx: number; dy: number } | null>(null);

  const toVp = (p: Pt) => viewport.convertToViewportPoint(p[0], p[1]) as Pt;
  const cssStrokes = (sh: ShapeItem): Pt[][] => {
    if (sh.kind === "rect" || sh.kind === "ellipse") {
      return [[toVp([sh.rect[0], sh.rect[1]]), toVp([sh.rect[2], sh.rect[3]])]];
    }
    return sh.strokes.map((st) => st.map(toVp));
  };
  const selBox = (sh: ShapeItem) => {
    const [ax, ay] = toVp([sh.rect[0], sh.rect[1]]);
    const [bx, by] = toVp([sh.rect[2], sh.rect[3]]);
    const pad = (sh.width * zoom) / 2 + 3;
    return {
      l: Math.min(ax, bx) - pad,
      t: Math.min(ay, by) - pad,
      w: Math.abs(bx - ax) + pad * 2,
      h: Math.abs(by - ay) + pad * 2,
    };
  };

  const onDown = (e: React.PointerEvent<SVGSVGElement>) => {
    if (!active || e.button !== 0) return;
    e.preventDefault();
    e.stopPropagation();
    const st = useStore.getState();
    const box = e.currentTarget.getBoundingClientRect();
    const start: Pt = [e.clientX - box.left, e.clientY - box.top];
    const toPdf = (p: Pt) => viewport.convertToPdfPoint(p[0], p[1]) as Pt;
    const pStart = toPdf(start);

    const hit = shapes
      .filter((sh) => {
        const g = Math.max(6 / Math.max(zoom, 0.01), sh.width);
        return (
          pStart[0] >= sh.rect[0] - g && pStart[0] <= sh.rect[2] + g &&
          pStart[1] >= sh.rect[1] - g && pStart[1] <= sh.rect[3] + g
        );
      })
      .sort(
        (a, b) =>
          (a.rect[2] - a.rect[0]) * (a.rect[3] - a.rect[1]) -
          (b.rect[2] - b.rect[0]) * (b.rect[3] - b.rect[1]),
      )[0];

    if (hit) {
      let moved = false;
      track(
        (ev) => {
          const dx = ev.clientX - box.left - start[0];
          const dy = ev.clientY - box.top - start[1];
          if (!moved && Math.hypot(dx, dy) > 3) moved = true;
          if (moved) setGhost({ id: hit.id, dx, dy });
        },
        (ev) => {
          setGhost(null);
          useStore.getState().setShapeSel({ tabId, id: hit.id });
          if (!moved) return;
          const pEnd = toPdf([ev.clientX - box.left, ev.clientY - box.top]);
          moveShape(tabId, hit.id, pEnd[0] - pStart[0], pEnd[1] - pStart[1]);
        },
      );
      return;
    }

    if (st.shapeSel) st.setShapeSel(null);
    const kind = st.drawKind;
    const pts: Pt[] = [start];
    setDraft({ kind, pts: [start] });
    track(
      (ev) => {
        const p: Pt = [ev.clientX - box.left, ev.clientY - box.top];
        if (kind === "ink") {
          const last = pts[pts.length - 1];
          if (Math.hypot(p[0] - last[0], p[1] - last[1]) >= 2) {
            pts.push(p);
            setDraft({ kind, pts: [...pts] });
          }
        } else {
          setDraft({ kind, pts: [start, p] });
        }
      },
      (ev) => {
        setDraft(null);
        const end: Pt = [ev.clientX - box.left, ev.clientY - box.top];
        const cssPts = kind === "ink" ? pts : [start, end];
        const pdfPts = cssPts.map(toPdf);
        const xs = pdfPts.map((p) => p[0]);
        const ys = pdfPts.map((p) => p[1]);
        const rect: [number, number, number, number] = [
          Math.min(...xs), Math.min(...ys), Math.max(...xs), Math.max(...ys),
        ];
        if (Math.max(rect[2] - rect[0], rect[3] - rect[1]) < 3) return;
        const strokes: Pt[][] =
          kind === "ink" ? [pdfPts] : kind === "line" || kind === "arrow" ? [[pdfPts[0], pdfPts[1]]] : [];
        addShape(tabId, {
          id: crypto.randomUUID(),
          src, kind, rect, strokes,
          color: st.drawColor, width: st.drawWidth, opacity: 1,
        });
      },
    );
  };

  const sel = shapeSel?.tabId === tabId ? shapes.find((sh) => sh.id === shapeSel.id) : undefined;

  return (
    <div className="shape-layer">
      <svg
        width={viewport.width}
        height={viewport.height}
        style={{ pointerEvents: active ? "auto" : "none" }}
        onPointerDown={onDown}
      >
        {shapes.map((sh) => {
          const g = ghost?.id === sh.id ? ghost : null;
          return (
            <g key={sh.id} transform={g ? `translate(${g.dx} ${g.dy})` : undefined}>
              <ShapeSvg
                kind={sh.kind}
                strokes={cssStrokes(sh)}
                color={css(sh.color)}
                w={sh.width * zoom}
                headLen={Math.max(9, 4.5 * sh.width) * zoom}
              />
            </g>
          );
        })}
        {draft && (
          <ShapeSvg
            kind={draft.kind}
            strokes={draft.kind === "ink" ? [draft.pts] : [[draft.pts[0], draft.pts[1] ?? draft.pts[0]]]}
            color={css(useStore.getState().drawColor)}
            w={useStore.getState().drawWidth * zoom}
            headLen={Math.max(9, 4.5 * useStore.getState().drawWidth) * zoom}
          />
        )}
      </svg>
      {sel && !ghost && (() => {
        const b = selBox(sel);
        return (
          <>
            <div
              className="shape-sel-outline"
              style={{ left: b.l, top: b.t, width: b.w, height: b.h }}
            />
            <div
              className="obj-actions"
              style={{ left: b.l, top: Math.max(0, b.t - 30), pointerEvents: "auto" }}
              onPointerDown={(e) => e.stopPropagation()}
            >
              <button onClick={() => deleteShape(tabId, sel.id)}>Delete</button>
            </div>
          </>
        );
      })()}
    </div>
  );
}
