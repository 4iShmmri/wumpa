import { useEffect, useRef, useState } from "react";
import { useStore, useActiveTab } from "../store";
import { getDoc, getPage, type DocEntry } from "../pdf";
import type { PageSpec } from "../types";
import { movePage } from "../actions";
import { report } from "../errors";

const THUMB_W = 132;

function Thumb({ entry, spec }: { entry: DocEntry; spec: PageSpec }) {
  const canvasRef = useRef<HTMLCanvasElement>(null);
  const [visible, setVisible] = useState(false);
  const rendered = useRef(false);

  const base = entry.dims[spec.src - 1];
  const swap = spec.rotation % 180 !== 0;
  const bw = swap ? base.h : base.w;
  const bh = swap ? base.w : base.h;
  const scale = THUMB_W / bw;
  const h = Math.round(bh * scale);

  useEffect(() => {
    const el = canvasRef.current!;
    const io = new IntersectionObserver(([e]) => setVisible((v) => v || e.isIntersecting), {
      rootMargin: "400px 0px",
    });
    io.observe(el);
    return () => io.disconnect();
  }, []);

  useEffect(() => {
    if (!visible || rendered.current) return;
    rendered.current = true;
    let cancelled = false;
    (async () => {
      const page = await getPage(entry, spec.src);
      const rotation = (page.rotate + spec.rotation + 360) % 360;
      const vp = page.getViewport({ scale, rotation });
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
    })().catch(console.error);
    return () => {
      cancelled = true;
    };
  }, [visible]);

  return <canvas ref={canvasRef} style={{ width: THUMB_W, height: h }} />;
}

export default function Sidebar() {
  const tab = useActiveTab();
  const [entry, setEntry] = useState<DocEntry | null>(null);
  const listRef = useRef<HTMLDivElement>(null);
  const drag = useRef<{ from: number; startY: number; active: boolean } | null>(null);
  const [insertAt, setInsertAt] = useState<number | null>(null);
  const suppressClick = useRef(false);

  const tabId = tab?.id;
  const docVersion = tab?.docVersion ?? 0;

  useEffect(() => {
    let on = true;
    setEntry(null);
    if (!tab) return;
    getDoc(tab.id, tab.path, tab.docVersion)
      .then((e) => on && setEntry(e))
      .catch((e) => report("Load failed", e));
    return () => {
      on = false;
    };
  }, [tabId, docVersion]);

  if (!tab) return <div className="sidebar empty" />;

  const select = (i: number, e: React.MouseEvent) => {
    if (suppressClick.current) {
      suppressClick.current = false;
      return;
    }
    const st = useStore.getState();
    if (e.metaKey || e.ctrlKey) {
      const sel = tab.selected.includes(i)
        ? tab.selected.filter((x) => x !== i)
        : [...tab.selected, i];
      st.patchTab(tab.id, { selected: sel });
    } else if (e.shiftKey && tab.selected.length) {
      const anchor = tab.selected[0];
      const [a, b] = anchor < i ? [anchor, i] : [i, anchor];
      st.patchTab(tab.id, {
        selected: Array.from({ length: b - a + 1 }, (_, k) => a + k),
      });
    } else {
      st.patchTab(tab.id, { selected: [i] });
      st.scrollTo(i + 1);
    }
  };

  const computeInsert = (clientY: number): number => {
    const items = Array.from(listRef.current!.querySelectorAll<HTMLElement>(".thumb"));
    for (let i = 0; i < items.length; i++) {
      const r = items[i].getBoundingClientRect();
      if (clientY < r.top + r.height / 2) return i;
    }
    return items.length;
  };

  const onPointerDown = (i: number, e: React.PointerEvent) => {
    if (e.button !== 0) return;
    drag.current = { from: i, startY: e.clientY, active: false };
    const onMove = (ev: PointerEvent) => {
      const d = drag.current;
      if (!d) return;
      if (!d.active && Math.abs(ev.clientY - d.startY) > 6) d.active = true;
      if (d.active) setInsertAt(computeInsert(ev.clientY));
    };
    const onUp = (ev: PointerEvent) => {
      window.removeEventListener("pointermove", onMove);
      window.removeEventListener("pointerup", onUp);
      const d = drag.current;
      drag.current = null;
      setInsertAt(null);
      if (d?.active) {
        suppressClick.current = true;
        let to = computeInsert(ev.clientY);
        if (to > d.from) to -= 1;
        movePage(d.from, to);
      }
    };
    window.addEventListener("pointermove", onMove);
    window.addEventListener("pointerup", onUp);
  };

  return (
    <div className="sidebar" ref={listRef}>
      {entry &&
        tab.pages.map((spec, i) => (
          <div key={`${spec.src}:${spec.rotation}:${entry.version}`}>
            {insertAt === i && <div className="insert-line" />}
            <div
              className={
                "thumb" +
                (tab.selected.includes(i) ? " selected" : "") +
                (tab.currentPage === i + 1 ? " current" : "")
              }
              onClick={(e) => select(i, e)}
              onDoubleClick={() => useStore.getState().scrollTo(i + 1)}
              onPointerDown={(e) => onPointerDown(i, e)}
            >
              <Thumb entry={entry} spec={spec} />
              <span className="thumb-num">{i + 1}</span>
            </div>
          </div>
        ))}
      {insertAt === tab.pages.length && <div className="insert-line" />}
    </div>
  );
}
