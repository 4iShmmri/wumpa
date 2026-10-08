import { useEffect, useRef, useState } from "react";
import { useStore, useActiveTab } from "../store";
import { getDoc, getPage, docPath, specBase, type DocEntry } from "../pdf";
import type { PageSpec } from "../types";
import {
  movePage, insertBlankPage, duplicatePage, exportPageAsPng,
  rotateSelection, deleteSelection, extractSelected,
} from "../actions";
import { report } from "../errors";
import { t, useT } from "../i18n";
import BookmarkPanel from "./BookmarkPanel";

const THUMB_W = 132;
type SideView = "pages" | "outline";

function Thumb({ entry, spec }: { entry: DocEntry; spec: PageSpec }) {
  const canvasRef = useRef<HTMLCanvasElement>(null);
  const [visible, setVisible] = useState(false);
  const rendered = useRef(false);

  const base = specBase(entry, spec) ?? { w: 612, h: 792, rotate: 0 };
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
    if (!visible || rendered.current || spec.src === 0) return;
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
  const tr = useT();
  const tab = useActiveTab();
  const [entry, setEntry] = useState<DocEntry | null>(null);
  const listRef = useRef<HTMLDivElement>(null);
  const drag = useRef<{ from: number; startY: number; active: boolean } | null>(null);
  const [insertAt, setInsertAt] = useState<number | null>(null);
  const suppressClick = useRef(false);
  const [menu, setMenu] = useState<{ x: number; y: number; index: number } | null>(null);
  const [view, setView] = useState<SideView>("pages");
  const bmCount = tab?.bookmarks?.length ?? 0;

  const tabId = tab?.id;
  const docVersion = tab?.docVersion ?? 0;

  useEffect(() => {
    let on = true;
    setEntry(null);
    if (!tab) return;
    getDoc(tab.id, docPath(tab), tab.docVersion)
      .then((e) => on && setEntry(e))
      .catch((e) => report(t("stLoadFailed"), e));
    return () => {
      on = false;
    };
  }, [tabId, docVersion]);

  useEffect(() => {
    setMenu(null);
  }, [tabId]);

  useEffect(() => {
    if (!menu) return;
    const onDown = (e: PointerEvent) => {
      if (!(e.target as Element | null)?.closest?.(".ctx-menu")) setMenu(null);
    };
    const onKey = (e: KeyboardEvent) => {
      if (e.key === "Escape") {
        e.stopPropagation();
        setMenu(null);
      }
    };
    document.addEventListener("pointerdown", onDown, true);
    document.addEventListener("keydown", onKey, true);
    return () => {
      document.removeEventListener("pointerdown", onDown, true);
      document.removeEventListener("keydown", onKey, true);
    };
  }, [menu]);

  if (!tab) return <div className="side-col empty" />;

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

  const ensureSel = (i: number) => {
    if (!tab.selected.includes(i)) useStore.getState().patchTab(tab.id, { selected: [i] });
  };

  const item = (label: string, fn: () => void) => (
    <button
      onClick={() => {
        setMenu(null);
        fn();
      }}
    >
      {label}
    </button>
  );

  return (
    <div className="side-col">
      <div className="side-tabs">
        <button className={view === "pages" ? "on" : ""} onClick={() => setView("pages")}>
          {tr("sbPages")}
        </button>
        <button className={view === "outline" ? "on" : ""} onClick={() => setView("outline")}>
          {tr("sbOutline")}
          {bmCount > 0 && <span className="side-badge">{bmCount}</span>}
        </button>
      </div>

      {view === "outline" ? (
        <BookmarkPanel tab={tab} />
      ) : (
        <div className="sidebar" ref={listRef}>
          {entry &&
            tab.pages.map((spec, i) => (
          <div key={`${i}:${spec.src}:${spec.rotation}:${entry.version}`}>
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
              onContextMenu={(e) => {
                e.preventDefault();
                setMenu({ x: e.clientX, y: e.clientY, index: i });
              }}
            >
              <Thumb entry={entry} spec={spec} />
              <span className="thumb-num">{i + 1}</span>
            </div>
          </div>
        ))}
          {insertAt === tab.pages.length && <div className="insert-line" />}
        </div>
      )}

      {menu && (
        <div
          className="ctx-menu"
          style={{
            left: Math.min(menu.x, window.innerWidth - 200),
            top: Math.min(menu.y, window.innerHeight - 290),
          }}
        >
          {item(tr("ctxDuplicate"), () => duplicatePage(tab.id, menu.index))}
          {item(tr("ctxInsertBefore"), () => insertBlankPage(tab.id, menu.index))}
          {item(tr("ctxInsertAfter"), () => insertBlankPage(tab.id, menu.index + 1))}
          {item(tr("ctxExportPng"), () => exportPageAsPng(tab.id, menu.index))}
          <div className="ctx-sep" />
          {item(tr("ctxRotL"), () => {
            ensureSel(menu.index);
            rotateSelection(-90);
          })}
          {item(tr("ctxRotR"), () => {
            ensureSel(menu.index);
            rotateSelection(90);
          })}
          <div className="ctx-sep" />
          {item(tr("ctxExtract"), () => {
            ensureSel(menu.index);
            extractSelected();
          })}
          {item(tr("ctxDelete"), () => {
            ensureSel(menu.index);
            deleteSelection();
          })}
        </div>
      )}
    </div>
  );
}
