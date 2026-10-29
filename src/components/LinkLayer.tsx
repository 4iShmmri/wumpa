import { useEffect, useState } from "react";
import { trackPointer } from "../pointer";
import Modal from "./ui/Modal";
import type { PageViewport } from "pdfjs-dist";
import { getLinks, type DocEntry, type ExistingLink } from "../pdf";
import type { LinkItem, LinkRemoval, Rect } from "../types";
import { addLink, deleteLink, removeExistingLink, followLink } from "../actions";
import { useT } from "../i18n";

type Pt = [number, number];

interface Shown {
  key: string;
  rect: Rect; // PDF coords
  url: string | null;
  destSrc: number | null;
  kind: "existing" | "added";
  id?: string;
}

function rectNear(a: Rect, b: Rect) {
  return a.every((v, i) => Math.abs(v - b[i]) < 1.5);
}

function LinkModal({
  onClose,
  onSave,
}: {
  onClose: () => void;
  onSave: (t: { url?: string; destSrc?: number }) => void;
}) {
  const t = useT();
  const [mode, setMode] = useState<"url" | "page">("url");
  const [url, setUrl] = useState("");
  const [page, setPage] = useState("");

  const save = () => {
    if (mode === "url") {
      const v = url.trim();
      if (v) onSave({ url: /^https?:\/\//i.test(v) ? v : `https://${v}` });
    } else {
      const n = parseInt(page, 10);
      if (n > 0) onSave({ destSrc: n });
    }
  };

  return (
    <Modal onClose={onClose} className="lnk-modal">
      <h3>{t("lnkTitle")}</h3>
      <div className="seg">
        <button className={mode === "url" ? "on" : ""} onClick={() => setMode("url")}>
          {t("lnkToUrl")}
        </button>
        <button className={mode === "page" ? "on" : ""} onClick={() => setMode("page")}>
          {t("lnkToPage")}
        </button>
      </div>
      {mode === "url" ? (
        <input
          autoFocus
          placeholder={t("lnkUrlPlaceholder")}
          value={url}
          onChange={(e) => setUrl(e.target.value)}
          onKeyDown={(e) => {
            if (e.key === "Enter") save();
          }}
        />
      ) : (
        <input
          autoFocus
          type="number"
          min={1}
          placeholder={t("lnkPagePlaceholder")}
          value={page}
          onChange={(e) => setPage(e.target.value)}
          onKeyDown={(e) => {
            if (e.key === "Enter") save();
          }}
        />
      )}
      <div className="modal-actions">
        <button data-tip={t("tipCancelEsc")} onClick={onClose}>
          {t("btnCancel")}
        </button>
        <button className="btn-primary" onClick={save}>
          {t("btnInsert")}
        </button>
      </div>
    </Modal>
  );
}

export default function LinkLayer({
  entry,
  src,
  viewport,
  w,
  h,
  active,
  links,
  removals,
}: {
  entry: DocEntry;
  src: number;
  viewport: PageViewport;
  w: number;
  h: number;
  active: boolean;
  links: LinkItem[];
  removals: LinkRemoval[];
}) {
  const t = useT();
  const [existing, setExisting] = useState<ExistingLink[]>([]);
  const [draw, setDraw] = useState<{ a: Pt; b: Pt } | null>(null);
  const [pending, setPending] = useState<Rect | null>(null);
  const [sel, setSel] = useState<string | null>(null);

  useEffect(() => {
    if (src <= 0) return; // blank pages carry no links of their own
    let on = true;
    getLinks(entry, src)
      .then((l) => on && setExisting(l))
      .catch(() => {});
    return () => {
      on = false;
    };
  }, [entry, src, entry.version]);

  useEffect(() => {
    if (!active) {
      setSel(null);
      setPending(null);
      setDraw(null);
    }
  }, [active]);

  const toVp = (r: Rect) => {
    const [x1, y1] = viewport.convertToViewportPoint(r[0], r[1]) as [number, number];
    const [x2, y2] = viewport.convertToViewportPoint(r[2], r[3]) as [number, number];
    return {
      left: Math.min(x1, x2),
      top: Math.min(y1, y2),
      width: Math.abs(x2 - x1),
      height: Math.abs(y2 - y1),
    };
  };

  const shown: Shown[] = [
    ...existing
      .filter((e) => !removals.some((r) => rectNear(r.rect, e.rect)))
      .map((e, i) => ({
        key: `ex${i}`,
        rect: e.rect,
        url: e.url,
        destSrc: e.destSrc,
        kind: "existing" as const,
      })),
    ...links.map((l) => ({
      key: l.id,
      rect: l.rect,
      url: l.url ?? null,
      destSrc: l.destSrc ?? null,
      kind: "added" as const,
      id: l.id,
    })),
  ];

  const startDraw = (e: React.PointerEvent) => {
    if (!active || e.button !== 0) return;
    setSel(null);
    const host = (e.currentTarget as HTMLElement).getBoundingClientRect();
    const a: Pt = [e.clientX - host.left, e.clientY - host.top];
    setDraw({ a, b: a });
    trackPointer(
      (ev) => setDraw({ a, b: [ev.clientX - host.left, ev.clientY - host.top] }),
      (ev) => {
        const b: Pt = [ev.clientX - host.left, ev.clientY - host.top];
        setDraw(null);
        if (Math.abs(b[0] - a[0]) < 6 || Math.abs(b[1] - a[1]) < 6) return;
        const [px1, py1] = viewport.convertToPdfPoint(a[0], a[1]) as [number, number];
        const [px2, py2] = viewport.convertToPdfPoint(b[0], b[1]) as [number, number];
        setPending([Math.min(px1, px2), Math.min(py1, py2), Math.max(px1, px2), Math.max(py1, py2)]);
      },
    );
  };

  return (
    <div
      className={"link-layer" + (active ? " active" : "")}
      style={{ width: w, height: h }}
      onPointerDown={startDraw}
    >
      {shown.map((s) => {
        const box = toVp(s.rect);
        const label = s.url
          ? t("tipLnkOpen")
          : s.destSrc
            ? t("lnkGoPage", { n: s.destSrc })
            : t("lnkExisting");
        return (
          <div
            key={s.key}
            className={"link-box " + s.kind + (sel === s.key ? " sel" : "")}
            data-tip={active ? undefined : label}
            style={box}
            onPointerDown={(e) => {
              if (active) {
                e.stopPropagation();
                setSel(s.key);
              }
            }}
            onClick={() => {
              if (!active) followLink(s);
            }}
          >
            {active && sel === s.key && (
              <span className="link-pop" onPointerDown={(e) => e.stopPropagation()}>
                <span className="link-what" title={s.url ?? undefined}>
                  {s.url ?? label}
                </span>
                <button
                  data-tip={t("tipLnkDelete")}
                  onClick={() => {
                    if (s.kind === "added" && s.id) deleteLink(s.id);
                    else removeExistingLink(src, s.rect);
                    setSel(null);
                  }}
                >
                  ✕
                </button>
              </span>
            )}
          </div>
        );
      })}

      {draw && (
        <div
          className="link-draw"
          style={{
            left: Math.min(draw.a[0], draw.b[0]),
            top: Math.min(draw.a[1], draw.b[1]),
            width: Math.abs(draw.b[0] - draw.a[0]),
            height: Math.abs(draw.b[1] - draw.a[1]),
          }}
        />
      )}

      {active && !draw && shown.length === 0 && <span className="link-hint">{t("lnkDrawHint")}</span>}

      {pending && (
        <LinkModal
          onClose={() => setPending(null)}
          onSave={(target) => {
            addLink(src, pending, target);
            setPending(null);
          }}
        />
      )}
    </div>
  );
}
