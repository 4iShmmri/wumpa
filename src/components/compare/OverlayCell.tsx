import { useEffect, useRef, useState } from "react";
import { getPage, type DocEntry } from "../../pdf";
import { useVisible } from "./shared";

const LUM = (d: Uint8ClampedArray, i: number) => 0.299 * d[i] + 0.587 * d[i + 1] + 0.114 * d[i + 2];

// Both versions of a page blended into one canvas: ink only in the old file
// shows in one tint, ink only in the new file in the other.
export default function OverlayCell({
  left,
  right,
  src,
  cellW,
}: {
  left: DocEntry;
  right: DocEntry;
  src: number;
  cellW: number;
}) {
  const wrapRef = useRef<HTMLDivElement>(null);
  const canvasRef = useRef<HTMLCanvasElement>(null);
  const visible = useVisible(wrapRef);
  const [done, setDone] = useState(false);
  const lp = src <= left.numPages ? left.dims[src - 1] : null;
  const rp = src <= right.numPages ? right.dims[src - 1] : null;
  const maxW = Math.max(lp?.w ?? 0, rp?.w ?? 0) || 600;
  const maxH = Math.max(lp?.h ?? 0, rp?.h ?? 0) || 800;
  const scale = cellW ? cellW / maxW : 1;
  const dispW = maxW * scale;
  const dispH = maxH * scale;

  useEffect(() => {
    setDone(false);
  }, [cellW]);

  useEffect(() => {
    if (!visible || !cellW || done) return;
    let cancelled = false;
    (async () => {
      const out = canvasRef.current;
      if (!out) return;
      const w = Math.max(1, Math.round(dispW));
      const h = Math.max(1, Math.round(dispH));
      const side = async (dims: typeof lp, entry: DocEntry) => {
        const c = document.createElement("canvas");
        c.width = w;
        c.height = h;
        const cx = c.getContext("2d")!;
        cx.fillStyle = "#fff";
        cx.fillRect(0, 0, w, h);
        if (dims) {
          const page = await getPage(entry, src);
          await page.render({ canvas: c, viewport: page.getViewport({ scale }) }).promise;
        }
        return c;
      };
      const [lc, rc] = await Promise.all([side(lp, left), side(rp, right)]);
      if (cancelled) return;
      const lctx = lc.getContext("2d")!;
      const rctx = rc.getContext("2d")!;
      const ld = lctx.getImageData(0, 0, w, h).data;
      const rd = rctx.getImageData(0, 0, w, h).data;
      const outImg = lctx.createImageData(w, h);
      const od = outImg.data;
      for (let i = 0; i < od.length; i += 4) {
        const lInk = 255 - LUM(ld, i);
        const rInk = 255 - LUM(rd, i);
        od[i] = 255 - rInk;
        od[i + 1] = 255 - Math.max(lInk, rInk);
        od[i + 2] = 255 - lInk;
        od[i + 3] = 255;
      }
      out.width = w;
      out.height = h;
      out.getContext("2d")!.putImageData(outImg, 0, 0);
      if (!cancelled) setDone(true);
    })().catch(() => {});
    return () => {
      cancelled = true;
    };
  }, [visible, cellW, done]);

  return (
    <div ref={wrapRef} className="cmp-cell overlay" style={{ width: dispW }}>
      <div className="cmp-page" style={{ width: dispW, height: dispH }}>
        <canvas ref={canvasRef} style={{ width: dispW, height: dispH }} />
      </div>
      <span className="cmp-pageno">{src}</span>
    </div>
  );
}
