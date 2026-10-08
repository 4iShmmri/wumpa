import { useEffect, useMemo, useRef, useState } from "react";
import type { PageViewport } from "pdfjs-dist";
import type { TextItem } from "pdfjs-dist/types/src/display/api";
import { useStore, type CompareState } from "../store";
import { getDoc, getPage, type DocEntry } from "../pdf";
import { report } from "../errors";
import { useT, t } from "../i18n";

type Box = [number, number, number, number];
type Mode = "text" | "overlay";

interface PageDiff {
  status: "same" | "changed" | "added" | "removed";
  leftBoxes: Box[];
  rightBoxes: Box[];
}

function itemBox(it: TextItem): Box {
  const tx = it.transform;
  const x = tx[4];
  const y = tx[5];
  const w = it.width || 0;
  const h = it.height || Math.abs(tx[3]) || 8;
  return [x, y, x + w, y + h];
}

async function pageTokens(entry: DocEntry, src: number) {
  const page = await getPage(entry, src);
  const content = await page.getTextContent();
  const items = content.items.filter((it): it is TextItem => "str" in it);
  const tokens = items.map((it) => it.str.replace(/\s+/g, " ").trim());
  const boxes = items.map(itemBox);
  return { tokens, boxes };
}

function diffFlags(a: string[], b: string[]) {
  const n = a.length;
  const m = b.length;
  const left = new Array<boolean>(n).fill(true);
  const right = new Array<boolean>(m).fill(true);
  if (n === 0 || m === 0) return { left, right };
  if (n * m > 600000) {
    const k = Math.min(n, m);
    for (let i = 0; i < k; i++) if (a[i] === b[i]) left[i] = right[i] = false;
    return { left, right };
  }
  const dp: Int32Array[] = Array.from({ length: n + 1 }, () => new Int32Array(m + 1));
  for (let i = n - 1; i >= 0; i--)
    for (let j = m - 1; j >= 0; j--)
      dp[i][j] = a[i] === b[j] ? dp[i + 1][j + 1] + 1 : Math.max(dp[i + 1][j], dp[i][j + 1]);
  let i = 0;
  let j = 0;
  while (i < n && j < m) {
    if (a[i] === b[j]) {
      left[i] = false;
      right[j] = false;
      i++;
      j++;
    } else if (dp[i + 1][j] >= dp[i][j + 1]) i++;
    else j++;
  }
  return { left, right };
}

async function renderToCanvas(entry: DocEntry, src: number, scale: number, canvas: HTMLCanvasElement) {
  const page = await getPage(entry, src);
  const vp = page.getViewport({ scale });
  const dpr = Math.min(window.devicePixelRatio || 1, 2);
  canvas.width = Math.floor(vp.width * dpr);
  canvas.height = Math.floor(vp.height * dpr);
  await page.render({
    canvas,
    viewport: vp,
    transform: dpr !== 1 ? [dpr, 0, 0, dpr, 0, 0] : undefined,
  }).promise;
  return vp;
}

function CmpTextCell({
  entry, src, present, cellW, boxes, kind, flagged,
}: {
  entry: DocEntry | null;
  src: number;
  present: boolean;
  cellW: number;
  boxes: Box[] | undefined;
  kind: "removed" | "added";
  flagged: boolean;
}) {
  const wrapRef = useRef<HTMLDivElement>(null);
  const canvasRef = useRef<HTMLCanvasElement>(null);
  const [visible, setVisible] = useState(false);
  const [vp, setVp] = useState<PageViewport | null>(null);
  const dims = present && entry ? entry.dims[src - 1] : null;
  const scale = dims && cellW ? cellW / dims.w : 1;
  const dispW = dims ? dims.w * scale : cellW;
  const dispH = dims ? dims.h * scale : cellW * 1.3;

  useEffect(() => {
    const el = wrapRef.current!;
    const io = new IntersectionObserver(
      ([e]) => setVisible((v) => v || e.isIntersecting),
      { rootMargin: "800px 0px" },
    );
    io.observe(el);
    return () => io.disconnect();
  }, []);

  useEffect(() => {
    setVp(null);
  }, [cellW]);

  useEffect(() => {
    if (!visible || !present || !entry || !cellW || vp) return;
    let cancelled = false;
    (async () => {
      const canvas = canvasRef.current;
      if (!canvas) return;
      const v = await renderToCanvas(entry, src, scale, canvas);
      if (!cancelled) setVp(v);
    })().catch(() => {});
    return () => {
      cancelled = true;
    };
  }, [visible, cellW, vp]);

  return (
    <div
      ref={wrapRef}
      className={"cmp-cell" + (flagged ? " flag-" + kind : "")}
      style={{ width: dispW }}
    >
      {present ? (
        <div className="cmp-page" style={{ width: dispW, height: dispH }}>
          <canvas ref={canvasRef} style={{ width: dispW, height: dispH }} />
          {vp &&
            boxes?.map((b, k) => {
              const [x1, y1] = vp.convertToViewportPoint(b[0], b[1]) as [number, number];
              const [x2, y2] = vp.convertToViewportPoint(b[2], b[3]) as [number, number];
              return (
                <div
                  key={k}
                  className={"cmp-box " + kind}
                  style={{
                    left: Math.min(x1, x2),
                    top: Math.min(y1, y2),
                    width: Math.abs(x2 - x1),
                    height: Math.max(Math.abs(y2 - y1), 6),
                  }}
                />
              );
            })}
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

const LUM = (d: Uint8ClampedArray, i: number) =>
  0.299 * d[i] + 0.587 * d[i + 1] + 0.114 * d[i + 2];

function CmpOverlayCell({
  left, right, src, cellW,
}: {
  left: DocEntry;
  right: DocEntry;
  src: number;
  cellW: number;
}) {
  const wrapRef = useRef<HTMLDivElement>(null);
  const canvasRef = useRef<HTMLCanvasElement>(null);
  const [visible, setVisible] = useState(false);
  const [done, setDone] = useState(false);
  const lp = src <= left.numPages ? left.dims[src - 1] : null;
  const rp = src <= right.numPages ? right.dims[src - 1] : null;
  const maxW = Math.max(lp?.w ?? 0, rp?.w ?? 0) || 600;
  const maxH = Math.max(lp?.h ?? 0, rp?.h ?? 0) || 800;
  const scale = cellW ? cellW / maxW : 1;
  const dispW = maxW * scale;
  const dispH = maxH * scale;

  useEffect(() => {
    const el = wrapRef.current!;
    const io = new IntersectionObserver(
      ([e]) => setVisible((v) => v || e.isIntersecting),
      { rootMargin: "800px 0px" },
    );
    io.observe(el);
    return () => io.disconnect();
  }, []);

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

function CompareInner({ cmp }: { cmp: CompareState }) {
  const tr = useT();
  const close = useStore((s) => s.closeCompare);
  const [left, setLeft] = useState<DocEntry | null>(null);
  const [right, setRight] = useState<DocEntry | null>(null);
  const [diffs, setDiffs] = useState<PageDiff[] | null>(null);
  const [mode, setMode] = useState<Mode>("text");
  const [cellW, setCellW] = useState(0);
  const [curDiff, setCurDiff] = useState(-1);
  const bodyRef = useRef<HTMLDivElement>(null);
  const rowRefs = useRef<(HTMLDivElement | null)[]>([]);

  useEffect(() => {
    const onKey = (e: KeyboardEvent) => {
      if (e.key === "Escape") {
        e.stopPropagation();
        e.preventDefault();
        close();
      }
    };
    window.addEventListener("keydown", onKey, true);
    return () => window.removeEventListener("keydown", onKey, true);
  }, [close]);

  useEffect(() => {
    let on = true;
    Promise.all([
      getDoc(cmp.leftKey, cmp.leftPath, cmp.leftVersion),
      getDoc(cmp.rightKey, cmp.rightPath, cmp.rightVersion),
    ])
      .then(([l, r]) => {
        if (!on) return;
        setLeft(l);
        setRight(r);
      })
      .catch((e) => on && report(t("stCmpFailed"), e));
    return () => {
      on = false;
    };
  }, [cmp.leftKey, cmp.rightKey]);

  useEffect(() => {
    const el = bodyRef.current;
    if (!el) return;
    const measure = () => {
      const pad = 72;
      const w = Math.max(240, Math.floor((el.clientWidth - pad) / 2));
      setCellW((prev) => (Math.abs(prev - w) > 8 ? w : prev));
    };
    measure();
    const ro = new ResizeObserver(measure);
    ro.observe(el);
    return () => ro.disconnect();
  }, [left]);

  useEffect(() => {
    if (!left || !right) return;
    let on = true;
    setDiffs(null);
    setCurDiff(-1);
    (async () => {
      const pages = Math.max(left.numPages, right.numPages);
      const out: PageDiff[] = [];
      for (let i = 0; i < pages; i++) {
        if (!on) return;
        if (i >= left.numPages) {
          out.push({ status: "added", leftBoxes: [], rightBoxes: [] });
          continue;
        }
        if (i >= right.numPages) {
          out.push({ status: "removed", leftBoxes: [], rightBoxes: [] });
          continue;
        }
        const [L, R] = await Promise.all([pageTokens(left, i + 1), pageTokens(right, i + 1)]);
        const f = diffFlags(L.tokens, R.tokens);
        const leftBoxes = L.boxes.filter((_, k) => f.left[k] && L.tokens[k].length > 0);
        const rightBoxes = R.boxes.filter((_, k) => f.right[k] && R.tokens[k].length > 0);
        out.push({
          status: leftBoxes.length || rightBoxes.length ? "changed" : "same",
          leftBoxes,
          rightBoxes,
        });
      }
      if (on) setDiffs(out);
    })().catch((e) => on && report(t("stCmpFailed"), e));
    return () => {
      on = false;
    };
  }, [left, right]);

  const diffPages = useMemo(
    () => (diffs ? diffs.flatMap((d, i) => (d.status !== "same" ? [i] : [])) : []),
    [diffs],
  );

  const jump = (dir: 1 | -1) => {
    if (!diffPages.length) return;
    let next = curDiff + dir;
    if (next < 0) next = diffPages.length - 1;
    if (next >= diffPages.length) next = 0;
    setCurDiff(next);
    rowRefs.current[diffPages[next]]?.scrollIntoView({ block: "center", behavior: "smooth" });
  };

  const pages = Math.max(left?.numPages ?? 0, right?.numPages ?? 0);
  const ready = !!left && !!right;

  return (
    <div className="cmp-overlay">
      <div className="cmp-head">
        <div className="cmp-files">
          <span className="cmp-fname left" title={cmp.leftName}>{cmp.leftName}</span>
          <span className="cmp-vs">⟷</span>
          <span className="cmp-fname right" title={cmp.rightName}>{cmp.rightName}</span>
        </div>

        <div className="cmp-tools">
          <div className="cmp-seg">
            <button
              className={mode === "text" ? "on" : ""}
              data-tip={tr("tipCmpModeText")}
              onClick={() => setMode("text")}
            >
              {tr("cmpModeText")}
            </button>
            <button
              className={mode === "overlay" ? "on" : ""}
              data-tip={tr("tipCmpModeOverlay")}
              onClick={() => setMode("overlay")}
            >
              {tr("cmpModeOverlay")}
            </button>
          </div>

          {mode === "overlay" && (
            <div className="cmp-legend">
              <span><i className="lg removed" />{tr("cmpLegendRemoved")}</span>
              <span><i className="lg added" />{tr("cmpLegendAdded")}</span>
            </div>
          )}

          <span className="cmp-count">
            {!ready || !diffs
              ? tr("cmpComparing")
              : diffPages.length
                ? tr("cmpDiffs", { n: diffPages.length })
                : tr("cmpNoDiff")}
          </span>

          <button
            className="cmp-nav"
            disabled={!diffPages.length}
            data-tip={tr("tipCmpPrev")}
            onClick={() => jump(-1)}
          >
            ‹
          </button>
          <button
            className="cmp-nav"
            disabled={!diffPages.length}
            data-tip={tr("tipCmpNext")}
            onClick={() => jump(1)}
          >
            ›
          </button>

          <button className="cmp-close" data-tip={tr("tipCmpClose")} onClick={() => close()}>
            ✕
          </button>
        </div>
      </div>

      <div className="cmp-body" ref={bodyRef}>
        {ready && cellW > 0 &&
          Array.from({ length: pages }).map((_, i) => {
            const d = diffs?.[i];
            return (
              <div
                className={"cmp-row" + (d && d.status !== "same" ? " diff" : "")}
                key={i}
                ref={(el) => {
                  rowRefs.current[i] = el;
                }}
              >
                {mode === "text" ? (
                  <>
                    <CmpTextCell
                      entry={left}
                      src={i + 1}
                      present={i < (left?.numPages ?? 0)}
                      cellW={cellW}
                      boxes={d?.leftBoxes}
                      kind="removed"
                      flagged={d?.status === "removed"}
                    />
                    <CmpTextCell
                      entry={right}
                      src={i + 1}
                      present={i < (right?.numPages ?? 0)}
                      cellW={cellW}
                      boxes={d?.rightBoxes}
                      kind="added"
                      flagged={d?.status === "added"}
                    />
                  </>
                ) : (
                  <CmpOverlayCell left={left!} right={right!} src={i + 1} cellW={cellW * 2} />
                )}
              </div>
            );
          })}
      </div>
    </div>
  );
}

export default function CompareView() {
  const cmp = useStore((s) => s.compare);
  if (!cmp) return null;
  return <CompareInner cmp={cmp} key={cmp.leftKey} />;
}
