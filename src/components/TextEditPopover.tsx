import { useEffect, useRef, useState } from "react";
import { getTextInRect, type DocEntry } from "../pdf";
import { replaceTextObject } from "../actions";

const COLORS = ["#111111", "#c0392b", "#1d4ed8"];

export default function TextEditPopover({
  entry, tabId, src, objId, bbox, left, top, onDone,
}: {
  entry: DocEntry;
  tabId: string;
  src: number;
  objId: number;
  bbox: [number, number, number, number];
  left: number;
  top: number;
  onDone: () => void;
}) {
  const [text, setText] = useState("");
  const [size, setSize] = useState(14);
  const [color, setColor] = useState(COLORS[0]);
  const [ready, setReady] = useState(false);
  const [unreadable, setUnreadable] = useState(false);
  const taRef = useRef<HTMLTextAreaElement>(null);

  useEffect(() => {
    let on = true;
    getTextInRect(entry, src, bbox)
      .then((r) => {
        if (!on) return;
        if (r.text) {
          setText(r.text);
          setSize(Math.max(6, Math.min(120, Math.round(r.size) || 14)));
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
    replaceTextObject(tabId, src, objId, bbox, text, size, color);
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
        placeholder={
          unreadable ? "Original text could not be read — type replacement" : "Loading…"
        }
        onChange={(e) => setText(e.target.value)}
      />
      <div className="te-tools">
        <button title="Smaller" onClick={() => setSize((v) => Math.max(6, v - 2))}>A−</button>
        <button title="Larger" onClick={() => setSize((v) => Math.min(120, v + 2))}>A+</button>
        <span className="te-size">{size}pt</span>
        {COLORS.map((c) => (
          <button
            key={c}
            className={"dot" + (color === c ? " on" : "")}
            style={{ background: c }}
            title="Color"
            onClick={() => setColor(c)}
          />
        ))}
      </div>
      <div className="row">
        <button onClick={onDone}>Cancel</button>
        <button className="btn-primary" onClick={replace}>Replace</button>
      </div>
    </div>
  );
}
