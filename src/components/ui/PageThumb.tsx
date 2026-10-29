import { useEffect, useRef, useState } from "react";
import { getPage, pageRotation, type DocEntry } from "../../pdf";

// One page rendered small into a canvas. Sized from `base` (the page's size at
// scale 1) so the canvas has its final footprint before anything is drawn;
// either a fixed `width` or a `fit` box picks the scale. With `lazy` the page
// is drawn only once the canvas scrolls near the viewport.
export interface PageThumbProps {
  entry: DocEntry;
  src: number; // 1-based source page; 0 is a blank page and draws nothing
  base: { w: number; h: number }; // page size at scale 1, before `rotation`
  rotation?: number; // extra rotation on top of the page's own
  width?: number; // CSS width; the height follows the page's aspect
  fit?: { w: number; h: number }; // or fit inside this box
  lazy?: boolean; // default true
  rootMargin?: string;
  onRendered?: () => void;
  style?: React.CSSProperties;
  className?: string;
}

export default function PageThumb({
  entry,
  src,
  base,
  rotation = 0,
  width,
  fit,
  lazy = true,
  rootMargin = "400px 0px",
  onRendered,
  style,
  className,
}: PageThumbProps) {
  const canvasRef = useRef<HTMLCanvasElement>(null);
  const [visible, setVisible] = useState(!lazy);

  const swap = rotation % 180 !== 0;
  const bw = swap ? base.h : base.w;
  const bh = swap ? base.w : base.h;
  const scale = width ? width / bw : fit ? Math.min(fit.w / bw, fit.h / bh) : 1;
  const w = Math.round(bw * scale);
  const h = Math.round(bh * scale);

  useEffect(() => {
    if (!lazy) return;
    const el = canvasRef.current!;
    const io = new IntersectionObserver(([e]) => setVisible((v) => v || e.isIntersecting), { rootMargin });
    io.observe(el);
    return () => io.disconnect();
  }, [lazy]);

  useEffect(() => {
    if (!visible || src <= 0) return;
    let cancelled = false;
    let task: { cancel: () => void } | null = null;
    (async () => {
      const page = await getPage(entry, src);
      const vp = page.getViewport({ scale, rotation: pageRotation(page.rotate, rotation) });
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
      if (!cancelled) onRendered?.();
    })().catch((e) => {
      if (!cancelled) console.error(e);
    });
    return () => {
      cancelled = true;
      task?.cancel();
    };
  }, [visible, entry, src, rotation, scale]);

  return <canvas ref={canvasRef} className={className} style={{ width: w, height: h, ...style }} />;
}
