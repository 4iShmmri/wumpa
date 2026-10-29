import { useEffect, useRef, useState } from "react";
import type { PageViewport } from "pdfjs-dist";
import type { DocEntry } from "../../pdf";
import { useT } from "../../i18n";
import {
  KIND_LABEL,
  boxesFor,
  pageBannerFor,
  sideShows,
  type Box,
  type Change,
  type PageStatus,
  type Side,
} from "../../compare";
import ChangeNote from "./ChangeNote";
import { renderToCanvas, useVisible } from "./shared";

// One page of one side, with its change boxes and the open note.
export interface CellProps {
  entry: DocEntry | null;
  src: number;
  present: boolean;
  cellW: number;
  side: Side;
  changes: Change[];
  pageStatus: PageStatus | undefined;
  current: number;
  noteOpen: boolean;
  rows: boolean;
  total: number;
  onPick: (id: number) => void;
  onPrev: () => void;
  onNext: () => void;
  onCloseNote: () => void;
}

export default function TextCell({
  entry,
  src,
  present,
  cellW,
  side,
  changes,
  pageStatus,
  current,
  noteOpen,
  rows,
  onPick,
  onPrev,
  onNext,
  onCloseNote,
  total,
}: CellProps) {
  const t = useT();
  const wrapRef = useRef<HTMLDivElement>(null);
  const canvasRef = useRef<HTMLCanvasElement>(null);
  const visible = useVisible(wrapRef);
  const [vp, setVp] = useState<PageViewport | null>(null);
  const dims = present && entry ? entry.dims[src - 1] : null;
  const scale = dims && cellW ? cellW / dims.w : 1;
  const dispW = dims ? dims.w * scale : cellW;
  const dispH = dims ? dims.h * scale : cellW * 1.3;

  useEffect(() => {
    if (!visible || !present || !entry || !cellW) return;
    let cancelled = false;
    let task: { cancel: () => void } | null = null;
    (async () => {
      const canvas = canvasRef.current;
      if (!canvas) return;
      const v = await renderToCanvas(entry, src, scale, canvas, (tk) => (task = tk));
      if (!cancelled) setVp(v);
    })().catch(() => {});
    return () => {
      cancelled = true;
      task?.cancel();
    };
  }, [visible, cellW, entry]);

  const mine = changes.filter((c) => sideShows(c, side));
  const pageBanner = pageBannerFor(pageStatus, side);

  const toScreen = (b: Box) => {
    const [x1, y1] = vp!.convertToViewportPoint(b[0], b[1]) as [number, number];
    const [x2, y2] = vp!.convertToViewportPoint(b[2], b[3]) as [number, number];
    return {
      left: Math.min(x1, x2) - 2,
      top: Math.min(y1, y2) - 1,
      width: Math.abs(x2 - x1) + 4,
      height: Math.max(Math.abs(y2 - y1), 6) + 2,
    };
  };

  const cur = mine.find((c) => c.id === current);

  return (
    <div
      ref={wrapRef}
      className={
        "cmp-cell" +
        (pageBanner ? " flag-" + (pageBanner === "cmpPageInserted" ? "inserted" : "deleted") : "")
      }
      style={{ width: dispW }}
    >
      {present ? (
        <div className="cmp-page" style={{ width: dispW, height: dispH }}>
          <canvas ref={canvasRef} style={{ width: dispW, height: dispH }} />
          {vp &&
            rows &&
            mine.map((c) => {
              const boxes = boxesFor(c, side).map(toScreen);
              if (!boxes.length) return null;
              const top = Math.min(...boxes.map((b) => b.top)) - 1;
              const bottom = Math.max(...boxes.map((b) => b.top + b.height)) + 1;
              return (
                <div
                  key={"r" + c.id}
                  className={"cmp-rowhl" + (c.id === current ? " cur" : "")}
                  style={{ top, height: bottom - top }}
                />
              );
            })}
          {vp &&
            mine.map((c) =>
              boxesFor(c, side).map((b, k) => (
                <div
                  key={c.id + ":" + k}
                  className={"cmp-box " + c.kind + (c.id === current ? " cur" : "")}
                  data-chg={c.id}
                  style={toScreen(b)}
                  title={t(KIND_LABEL[c.kind])}
                  onClick={(e) => {
                    e.stopPropagation();
                    onPick(c.id);
                  }}
                />
              )),
            )}
          {vp &&
            noteOpen &&
            cur &&
            (side === "new") !== (cur.kind === "deleted") &&
            (() => {
              const boxes = boxesFor(cur, side).map(toScreen);
              if (!boxes.length) return null;
              const anchor = boxes[boxes.length - 1];
              return (
                <div
                  className="cmp-note-anchor"
                  style={{
                    top: anchor.top + anchor.height + 8,
                    left: Math.min(boxes[0].left, Math.max(0, dispW - 310)),
                  }}
                >
                  <ChangeNote
                    change={cur}
                    total={total}
                    onPrev={onPrev}
                    onNext={onNext}
                    onClose={onCloseNote}
                  />
                </div>
              );
            })()}
          {pageBanner && <div className="cmp-page-banner">{t(pageBanner)}</div>}
        </div>
      ) : (
        <div className="cmp-missing" style={{ width: dispW, height: dispH }}>
          {t("cmpNoPage")}
        </div>
      )}
      <span className="cmp-pageno">{src}</span>
    </div>
  );
}
