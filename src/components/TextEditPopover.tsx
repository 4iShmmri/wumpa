import { useEffect, useRef, useState } from "react";
import { getTextInRect, type DocEntry } from "../pdf";
import { replaceTextObject } from "../actions";
import { STAMP_FONT, STAMP_LINE } from "../stamps";
import { useT } from "../i18n";

const COLORS = ["#111111", "#c0392b", "#1d4ed8"];

export default function TextEditPopover({
  entry, tabId, src, objId, bbox, left, top, zoom, onDone,
}: {
  entry: DocEntry;
  tabId: string;
  src: number;
  objId: number;
  bbox: [number, number, number, number];
  left: number;
  top: number;
  zoom: number;
  onDone: () => void;
}) {
  const t = useT();
  const [text, setText] = useState("");
  const [size, setSize] = useState(14);
  const [color, setColor] = useState(COLORS[0]);
  const [fontFamily, setFontFamily] = useState<string | undefined>(undefined);
  const [lineHeight, setLineHeight] = useState(STAMP_LINE);
  const [ready, setReady] = useState(false);
  const [unreadable, setUnreadable] = useState(false);
  const orig = useRef<{ text: string; size: number; color: string } | null>(null);
  const taRef = useRef<HTMLTextAreaElement>(null);

  useEffect(() => {
    let on = true;
    getTextInRect(entry, src, bbox)
      .then((r) => {
        if (!on) return;
        if (r.text) {
          const sz = Math.max(6, Math.min(120, r.size || 14));
          setText(r.text);
          setSize(sz);
          setLineHeight(r.lineHeight || STAMP_LINE);
          if (r.fontName) setFontFamily(`"${r.fontName}", ${r.fallbackFamily}`);
          orig.current = { text: r.text, size: sz, color: COLORS[0] };
        } else {
          setUnreadable(true);
        }
        setReady(true);
      })
      .catch(() => {
        if (on) {
          setUnreadable(true);
          setReady(true);
        }
      });
    return () => {
      on = false;
    };
  }, []);

  useEffect(() => {
    if (ready) taRef.current?.focus();
  }, [ready]);

  const replace = () => {
    const o = orig.current;
    if (o && text === o.text && size === o.size && color === o.color) {
      onDone();
      return;
    }
    replaceTextObject(tabId, src, objId, bbox, text, size, color, fontFamily, lineHeight);
    onDone();
  };

  return (
    <div
      className="note-popover text-edit-popover"
      style={{ left, top, pointerEvents: "auto" }}
      onPointerDown={(e) => e.stopPropagation()}
      onKeyDown={(e) => {
        if (e.key === "Escape") {
          e.stopPropagation();
          onDone();
        } else if (e.key === "Enter" && (e.metaKey || e.ctrlKey)) {
          e.preventDefault();
          replace();
        }
      }}
    >
      <textarea
        ref={taRef}
        value={text}
        disabled={!ready}
        dir="auto"
        style={{
          fontFamily: fontFamily ?? STAMP_FONT,
          fontSize: Math.max(11, size * zoom),
          lineHeight,
          color,
        }}
        placeholder={
          unreadable ? t("teUnreadable") : t("teLoading")
        }
        onChange={(e) => setText(e.target.value)}
      />
      <div className="te-tools">
        <button title={t("tipSmaller")} onClick={() => setSize((v) => Math.max(6, v - 2))}>A−</button>
        <button title={t("tipLarger")} onClick={() => setSize((v) => Math.min(120, v + 2))}>A+</button>
        <span className="te-size">{Math.round(size)}pt</span>
        {COLORS.map((c) => (
          <button
            key={c}
            className={"dot" + (color === c ? " on" : "")}
            style={{ background: c }}
            title={t("tipColor")}
            onClick={() => setColor(c)}
          />
        ))}
      </div>
      <div className="row">
        <button onClick={onDone}>{t("btnCancel")}</button>
        <button className="btn-primary" onClick={replace}>{t("btnReplace")}</button>
      </div>
    </div>
  );
}
