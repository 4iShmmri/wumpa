import { useEffect, useRef, useState } from "react";
import { useStore } from "../store";

const W = 500;
const H = 200;
const DPR = 2;

type Stroke = [number, number][];

function redraw(canvas: HTMLCanvasElement, strokes: Stroke[]) {
  const ctx = canvas.getContext("2d")!;
  ctx.clearRect(0, 0, canvas.width, canvas.height);
  ctx.save();
  ctx.scale(DPR, DPR);
  ctx.lineWidth = 2.5;
  ctx.lineCap = "round";
  ctx.lineJoin = "round";
  ctx.strokeStyle = "#1b1b1b";
  for (const pts of strokes) {
    if (pts.length < 2) {
      if (pts.length === 1) {
        ctx.beginPath();
        ctx.arc(pts[0][0], pts[0][1], 1.2, 0, Math.PI * 2);
        ctx.fillStyle = "#1b1b1b";
        ctx.fill();
      }
      continue;
    }
    ctx.beginPath();
    ctx.moveTo(pts[0][0], pts[0][1]);
    for (let i = 1; i < pts.length - 1; i++) {
      const mx = (pts[i][0] + pts[i + 1][0]) / 2;
      const my = (pts[i][1] + pts[i + 1][1]) / 2;
      ctx.quadraticCurveTo(pts[i][0], pts[i][1], mx, my);
    }
    const last = pts[pts.length - 1];
    ctx.lineTo(last[0], last[1]);
    ctx.stroke();
  }
  ctx.restore();
}

export default function SignatureModal() {
  const open = useStore((s) => s.signatureModalOpen);
  const [saved, setSaved] = useState<string | null>(null);
  const [drawing, setDrawing] = useState(false);
  const strokes = useRef<Stroke[]>([]);
  const canvasRef = useRef<HTMLCanvasElement>(null);
  const [hasInk, setHasInk] = useState(false);

  useEffect(() => {
    if (!open) return;
    let s: string | null = null;
    try {
      s = localStorage.getItem("signature");
    } catch {
      s = null;
    }
    setSaved(s);
    setDrawing(!s);
    strokes.current = [];
    setHasInk(false);
  }, [open]);

  useEffect(() => {
    if (!open) return;
    const onKey = (e: KeyboardEvent) => {
      if (e.key === "Escape") {
        e.stopPropagation();
        close();
      }
    };
    window.addEventListener("keydown", onKey, true);
    return () => window.removeEventListener("keydown", onKey, true);
  }, [open]);

  if (!open) return null;

  const close = () => useStore.getState().setSignatureModal(false);
  const use = () => {
    useStore.getState().setTool("sign");
    close();
  };

  const onPointerDown = (e: React.PointerEvent<HTMLCanvasElement>) => {
    if (e.button !== 0) return;
    const canvas = canvasRef.current!;
    canvas.setPointerCapture(e.pointerId);
    const r = canvas.getBoundingClientRect();
    strokes.current.push([[e.clientX - r.left, e.clientY - r.top]]);
    setHasInk(true);
    const move = (ev: PointerEvent) => {
      const pts = strokes.current[strokes.current.length - 1];
      pts.push([ev.clientX - r.left, ev.clientY - r.top]);
      redraw(canvas, strokes.current);
    };
    const up = () => {
      canvas.removeEventListener("pointermove", move);
      canvas.removeEventListener("pointerup", up);
      canvas.removeEventListener("pointercancel", up);
    };
    canvas.addEventListener("pointermove", move);
    canvas.addEventListener("pointerup", up);
    canvas.addEventListener("pointercancel", up);
    redraw(canvas, strokes.current);
  };

  const clear = () => {
    strokes.current = [];
    setHasInk(false);
    if (canvasRef.current) redraw(canvasRef.current, []);
  };

  const save = () => {
    const all = strokes.current.flat();
    if (!all.length) return;
    const pad = 8;
    const minX = Math.max(0, Math.min(...all.map((p) => p[0])) - pad);
    const maxX = Math.min(W, Math.max(...all.map((p) => p[0])) + pad);
    const minY = Math.max(0, Math.min(...all.map((p) => p[1])) - pad);
    const maxY = Math.min(H, Math.max(...all.map((p) => p[1])) + pad);
    const out = document.createElement("canvas");
    out.width = Math.max(2, (maxX - minX) * DPR);
    out.height = Math.max(2, (maxY - minY) * DPR);
    const octx = out.getContext("2d")!;
    octx.drawImage(
      canvasRef.current!,
      minX * DPR, minY * DPR, out.width, out.height,
      0, 0, out.width, out.height,
    );
    const url = out.toDataURL("image/png");
    try {
      localStorage.setItem("signature", url);
    } catch {
      /* storage may be unavailable; signature still usable this session */
    }
    use();
  };

  return (
    <div className="modal-backdrop" onClick={close}>
      <div className="modal sig-modal" onClick={(e) => e.stopPropagation()}>
        <h3>Signature</h3>
        {!drawing && saved ? (
          <>
            <div className="sig-preview">
              <img src={saved} alt="Saved signature" />
            </div>
            <div className="modal-actions">
              <button onClick={close}>Cancel</button>
              <button onClick={() => setDrawing(true)}>Draw new</button>
              <button className="btn-primary" onClick={use}>Use</button>
            </div>
          </>
        ) : (
          <>
            <div className="sig-canvas-wrap" style={{ width: W, height: H }}>
              <canvas
                ref={canvasRef}
                className="sig-canvas"
                width={W * DPR}
                height={H * DPR}
                style={{ width: W, height: H }}
                onPointerDown={onPointerDown}
              />
              <div className="sig-baseline" />
            </div>
            <div className="modal-actions">
              <button onClick={clear}>Clear</button>
              <button onClick={close}>Cancel</button>
              <button className="btn-primary" disabled={!hasInk} onClick={save}>
                Save &amp; use
              </button>
            </div>
          </>
        )}
      </div>
    </div>
  );
}
