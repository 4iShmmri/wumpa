import { useEffect, useRef, useState } from "react";
import { trackPointer } from "../pointer";
import type { PageViewport } from "pdfjs-dist";
import { useStore } from "../store";
import { useT } from "../i18n";
import type { StampItem } from "../types";
import TextToolbar from "./ui/TextToolbar";
import {
  moveStamp,
  resizeStamp,
  deleteStamp,
  updateTextStamp,
  STAMP_FONT,
  STAMP_PAD_X,
  STAMP_PAD_Y,
  STAMP_LINE,
} from "../stamps";

// The stamp's place on screen. A stamp is always drawn upright in PDF user
// space (stamps.rs: `w 0 0 h x1 y1 cm /Img Do`, so the top row of the bitmap
// sits on y2), so on a rotated page its content turns with the page while the
// outline `l/t/w/h` stays the axis-aligned box of the rotated rect.
interface Box {
  l: number;
  t: number;
  w: number;
  h: number;
  // Screen position of the user-space top-left corner (x1, y2). This is the
  // corner resizeStamp/updateTextStamp keep fixed, and the content is rotated
  // about it.
  ax: number;
  ay: number;
  // Unrotated content size in CSS px and the page rotation to turn it by.
  cw: number;
  ch: number;
  rot: number;
}

function cssBox(viewport: PageViewport, rect: StampItem["rect"]): Box {
  const x1 = Math.min(rect[0], rect[2]);
  const x2 = Math.max(rect[0], rect[2]);
  const y1 = Math.min(rect[1], rect[3]);
  const y2 = Math.max(rect[1], rect[3]);
  const [ax, ay] = viewport.convertToViewportPoint(x1, y2) as [number, number];
  const [bx, by] = viewport.convertToViewportPoint(x2, y1) as [number, number];
  const [rx, ry] = viewport.convertToViewportPoint(x2, y2) as [number, number];
  return {
    l: Math.min(ax, bx),
    t: Math.min(ay, by),
    w: Math.abs(bx - ax),
    h: Math.abs(by - ay),
    ax,
    ay,
    cw: Math.hypot(rx - ax, ry - ay),
    ch: Math.hypot(bx - rx, by - ry),
    rot: ((viewport.rotation % 360) + 360) % 360,
  };
}

// CSS placing the unrotated content (cw × ch) with its top-left on the anchor
// corner, turned clockwise by the page rotation. Relative to `box.l/box.t`.
function contentStyle(box: Box): React.CSSProperties {
  return {
    position: "absolute",
    left: box.ax - box.l,
    top: box.ay - box.t,
    width: box.cw,
    height: box.ch,
    transformOrigin: "0 0",
    transform: box.rot ? `rotate(${box.rot}deg)` : undefined,
  };
}

const anchorRight = (box: Box) => box.ax > box.l + box.w / 2;
const anchorBottom = (box: Box) => box.ay > box.t + box.h / 2;

function TextEditor({
  stamp,
  tabId,
  box,
  zoom,
}: {
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
        // The textarea's own top-left is the user-space top-left of the text,
        // so turning it about that corner keeps it on the stamp's anchor.
        left: box.ax,
        top: box.ay,
        transformOrigin: "0 0",
        transform: box.rot ? `rotate(${box.rot}deg)` : undefined,
        minWidth: Math.max(box.cw, 60),
        minHeight: box.ch,
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

export default function StampLayer({
  tabId,
  src,
  stamps,
  viewport,
  zoom,
}: {
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
    trackPointer(
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

  // The handle sits on the corner opposite the anchor; the scale factor is the
  // pointer's horizontal distance from the anchor over the box width, so it
  // reads the same whichever way the page is turned.
  const startResize = (e: React.PointerEvent, s: StampItem, box: Box) => {
    if (e.button !== 0) return;
    e.stopPropagation();
    e.preventDefault();
    const r = layerRef.current!.getBoundingClientRect();
    const right = anchorRight(box);
    const factorAt = (ev: PointerEvent) => {
      const px = ev.clientX - r.left;
      const w = Math.max(10, right ? box.ax - px : px - box.ax);
      return w / box.w;
    };
    trackPointer(
      (ev) => setGhost({ id: s.id, dx: 0, dy: 0, scale: factorAt(ev) }),
      (ev) => {
        setGhost(null);
        const factor = factorAt(ev);
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
        const right = anchorRight(box);
        const bottom = anchorBottom(box);
        return (
          <div key={s.id}>
            <div
              className={"stamp" + (selected ? " selected" : "")}
              style={{
                left: box.l,
                top: box.t,
                width: box.w,
                height: box.h,
                transform,
                transformOrigin: `${right ? "right" : "left"} ${bottom ? "bottom" : "top"}`,
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
                    ...contentStyle(box),
                    fontSize: (s.size ?? 16) * zoom,
                    lineHeight: s.lineHeight ?? STAMP_LINE,
                    color: s.color,
                    fontFamily: s.fontFamily ?? STAMP_FONT,
                    padding: `${STAMP_PAD_Y * zoom}px ${STAMP_PAD_X * zoom}px`,
                    boxSizing: "border-box",
                  }}
                >
                  {s.text || " "}
                </div>
              ) : (
                <img src={s.sigDataUrl} draggable={false} alt="" style={contentStyle(box)} />
              )}
            </div>
            {selected && !g && (
              <>
                <div
                  className="stamp-handle"
                  style={{
                    left: right ? box.l - 5 : box.l + box.w - 5,
                    top: bottom ? box.t - 5 : box.t + box.h - 5,
                    cursor: right === bottom ? "nwse-resize" : "nesw-resize",
                  }}
                  onPointerDown={(e) => startResize(e, s, box)}
                />
                <div className="stamp-bar" style={{ left: box.l, top: Math.max(0, box.t - 34) }}>
                  {s.kind === "text" && (
                    <TextToolbar
                      size={s.size ?? 16}
                      color={s.color}
                      onSize={(size) => updateTextStamp(tabId, s.id, { size })}
                      onColor={(color) => updateTextStamp(tabId, s.id, { color })}
                    >
                      <button
                        data-tip={t("tipEditText")}
                        onClick={() => useStore.getState().setStampEditing(s.id)}
                      >
                        {t("btnEdit")}
                      </button>
                    </TextToolbar>
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
