import { useEffect, useRef, useState } from "react";
import type { PageViewport } from "pdfjs-dist";
import { useStore } from "../store";
import { useT } from "../i18n";
import type { StampItem } from "../types";
import {
  moveStamp, resizeStamp, deleteStamp, updateTextStamp,
  STAMP_FONT, STAMP_PAD_X, STAMP_PAD_Y, STAMP_LINE,
} from "../stamps";

const COLORS = ["#111111", "#c0392b", "#1d4ed8"];
const COLOR_NAMES = ["colorBlack", "colorRed", "colorBlue"] as const;

interface Box {
  l: number;
  t: number;
  w: number;
  h: number;
}

function cssBox(viewport: PageViewport, rect: StampItem["rect"]): Box {
  const [ax, ay] = viewport.convertToViewportPoint(rect[0], rect[1]) as [number, number];
  const [bx, by] = viewport.convertToViewportPoint(rect[2], rect[3]) as [number, number];
  return {
    l: Math.min(ax, bx),
    t: Math.min(ay, by),
    w: Math.abs(bx - ax),
    h: Math.abs(by - ay),
  };
}

function track(onMove: (ev: PointerEvent) => void, onUp: (ev: PointerEvent) => void) {
  const up = (ev: PointerEvent) => {
    window.removeEventListener("pointermove", onMove);
    window.removeEventListener("pointerup", up);
    onUp(ev);
  };
  window.addEventListener("pointermove", onMove);
  window.addEventListener("pointerup", up);
}

function TextEditor({ stamp, tabId, box, zoom }: {
  stamp: StampItem;
  tabId: string;
  box: Box;
  zoom: number;
}) {
  const [text, setText] = useState(stamp.text ?? "");
  const ref = useRef<HTMLTextAreaElement>(null);
  const done = useRef(false);

  const commit = (cancel: boolean) => {
    if (done.current) return;
    done.current = true;
    const final = cancel ? (stamp.text ?? "") : text;
    if (final.trim() === "") deleteStamp(tabId, stamp.id);
    else if (final !== stamp.text) updateTextStamp(tabId, stamp.id, { text: final });
    useStore.getState().setStampEditing(null);
  };

  useEffect(() => {
    ref.current?.focus();
    ref.current?.select();
  }, []);

  const size = (stamp.size ?? 16) * zoom;
  return (
    <textarea
      ref={ref}
      className="stamp-edit"
      dir="auto"
      style={{
        left: box.l,
        top: box.t,
        minWidth: Math.max(box.w, 60),
        minHeight: box.h,
        fontSize: size,
        lineHeight: stamp.lineHeight ?? STAMP_LINE,
        color: stamp.color,
        fontFamily: stamp.fontFamily ?? STAMP_FONT,
        padding: `${STAMP_PAD_Y * zoom}px ${STAMP_PAD_X * zoom}px`,
      }}
      value={text}
      onChange={(e) => setText(e.target.value)}
      onBlur={() => commit(false)}
      onKeyDown={(e) => {
        e.stopPropagation();
        if (e.key === "Enter" && (e.metaKey || e.ctrlKey)) commit(false);
        if (e.key === "Escape") commit(true);
      }}
      onPointerDown={(e) => e.stopPropagation()}
    />
  );
}

export default function StampLayer({ tabId, src, stamps, viewport, zoom }: {
  tabId: string;
  src: number;
  stamps: StampItem[];
  viewport: PageViewport;
  zoom: number;
}) {
  const t = useT();
  const stampSel = useStore((s) => s.stampSel);
  const editing = useStore((s) => s.stampEditing);
  const [ghost, setGhost] = useState<{ id: string; dx: number; dy: number; scale: number } | null>(null);
  const layerRef = useRef<HTMLDivElement>(null);

  const toPdf = (clientX: number, clientY: number) => {
    const r = layerRef.current!.getBoundingClientRect();
    return viewport.convertToPdfPoint(clientX - r.left, clientY - r.top) as [number, number];
  };

  const startMove = (e: React.PointerEvent, s: StampItem) => {
    if (e.button !== 0 || editing === s.id) return;
    e.stopPropagation();
    e.preventDefault();
    useStore.getState().setStampSel({ src, id: s.id });
    useStore.getState().setObjSel(null);
    const sx = e.clientX;
    const sy = e.clientY;
    let moved = false;
    track(
      (ev) => {
        const dx = ev.clientX - sx;
        const dy = ev.clientY - sy;
        if (!moved && Math.hypot(dx, dy) > 2) moved = true;
        if (moved) setGhost({ id: s.id, dx, dy, scale: 1 });
      },
      (ev) => {
        setGhost(null);
        if (!moved) return;
        const [px0, py0] = toPdf(sx, sy);
        const [px1, py1] = toPdf(ev.clientX, ev.clientY);
        moveStamp(tabId, s.id, px1 - px0, py1 - py0);
      },
    );
  };

  const startResize = (e: React.PointerEvent, s: StampItem, box: Box) => {
    if (e.button !== 0) return;
    e.stopPropagation();
    e.preventDefault();
    const r = layerRef.current!.getBoundingClientRect();
    track(
      (ev) => {
        const w = Math.max(10, ev.clientX - r.left - box.l);
        setGhost({ id: s.id, dx: 0, dy: 0, scale: w / box.w });
      },
      (ev) => {
        setGhost(null);
        const w = Math.max(10, ev.clientX - r.left - box.l);
        const factor = w / box.w;
        if (Math.abs(factor - 1) > 0.02) resizeStamp(tabId, s.id, factor);
      },
    );
  };

  return (
    <div ref={layerRef} className="stamp-layer">
      {stamps.map((s) => {
        const box = cssBox(viewport, s.rect);
        const selected = stampSel?.id === s.id;
        const g = ghost?.id === s.id ? ghost : null;
        const transform = g
          ? `translate(${g.dx}px, ${g.dy}px)` + (g.scale !== 1 ? ` scale(${g.scale})` : "")
          : undefined;
        if (editing === s.id && s.kind === "text") {
          return <TextEditor key={s.id} stamp={s} tabId={tabId} box={box} zoom={zoom} />;
        }
        return (
          <div key={s.id}>
            <div
              className={"stamp" + (selected ? " selected" : "")}
              style={{
                left: box.l, top: box.t, width: box.w, height: box.h,
                transform, transformOrigin: "top left",
              }}
              onPointerDown={(e) => startMove(e, s)}
              onDoubleClick={(e) => {
                if (s.kind !== "text") return;
                e.stopPropagation();
                useStore.getState().setStampEditing(s.id);
              }}
            >
              {s.kind === "text" ? (
                <div
                  className="stamp-text"
                  dir="auto"
                  style={{
                    fontSize: (s.size ?? 16) * zoom,
                    lineHeight: s.lineHeight ?? STAMP_LINE,
                    color: s.color,
                    fontFamily: s.fontFamily ?? STAMP_FONT,
                    padding: `${STAMP_PAD_Y * zoom}px ${STAMP_PAD_X * zoom}px`,
                  }}
                >
                  {s.text || " "}
                </div>
              ) : (
                <img src={s.sigDataUrl} draggable={false} alt="" />
              )}
            </div>
            {selected && !g && (
              <>
                <div
                  className="stamp-handle"
                  style={{ left: box.l + box.w - 5, top: box.t + box.h - 5 }}
                  onPointerDown={(e) => startResize(e, s, box)}
                />
                <div className="stamp-bar" style={{ left: box.l, top: Math.max(0, box.t - 34) }}>
                  {s.kind === "text" && (
                    <>
                      <button
                        data-tip={t("tipSmaller")}
                        onClick={() =>
                          updateTextStamp(tabId, s.id, {
                            size: Math.max(6, (s.size ?? 16) - 2),
                          })
                        }
                      >
                        A−
                      </button>
                      <button
                        data-tip={t("tipLarger")}
                        onClick={() =>
                          updateTextStamp(tabId, s.id, {
                            size: Math.min(120, (s.size ?? 16) + 2),
                          })
                        }
                      >
                        A+
                      </button>
                      {COLORS.map((c, ci) => (
                        <button
                          key={c}
                          className={"dot" + (s.color === c ? " on" : "")}
                          style={{ background: c }}
                          data-tip={t("tipTextColor", { c: t(COLOR_NAMES[ci]) })}
                          onClick={() => updateTextStamp(tabId, s.id, { color: c })}
                        />
                      ))}
                      <button
                        data-tip={t("tipEditText")}
                        onClick={() => useStore.getState().setStampEditing(s.id)}
                      >
                        {t("btnEdit")}
                      </button>
                    </>
                  )}
                  <button data-tip={t("tipDelete")} onClick={() => deleteStamp(tabId, s.id)}>
                    {t("btnDelete")}
                  </button>
                </div>
              </>
            )}
          </div>
        );
      })}
    </div>
  );
}
