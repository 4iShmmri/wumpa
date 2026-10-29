import { useState } from "react";
import type { PageViewport } from "pdfjs-dist";
import type { ExistingAnnot } from "../../pdf";
import type { Note } from "../../types";
import { updateNote, deleteNote } from "../../actions";
import { t } from "../../i18n";

export const NOTE_PT = 18; // icon footprint in PDF points

export function vpTopLeft(viewport: PageViewport, x1: number, y1: number, x2: number, y2: number) {
  const [ax, ay] = viewport.convertToViewportPoint(x1, y1) as [number, number];
  const [bx, by] = viewport.convertToViewportPoint(x2, y2) as [number, number];
  return { left: Math.min(ax, bx), top: Math.min(ay, by) };
}

function popoverPos(iconLeft: number, iconTop: number, size: number, w: number, h: number) {
  return {
    left: Math.max(4, Math.min(iconLeft + size + 8, w - 252)),
    top: Math.max(4, Math.min(iconTop, h - 140)),
  };
}

export function NoteIcon(props: {
  left: number;
  top: number;
  size: number;
  draft?: boolean;
  onClick: (e: React.MouseEvent) => void;
}) {
  return (
    <div
      className={"note-icon" + (props.draft ? " draft" : "")}
      style={{ left: props.left, top: props.top, width: props.size, height: props.size }}
      onClick={props.onClick}
      data-tip={props.draft ? t("noteUnsaved") : t("noteTitle")}
    >
      <svg
        width="68%"
        height="68%"
        viewBox="0 0 24 24"
        fill="none"
        stroke="currentColor"
        strokeWidth="2.4"
        strokeLinejoin="round"
      >
        <path d="M21 15a2 2 0 0 1-2 2H7l-4 4V5a2 2 0 0 1 2-2h14a2 2 0 0 1 2 2z" />
      </svg>
    </div>
  );
}

function NoteEditor(props: { note: Note; tabId: string; left: number; top: number; onDone: () => void }) {
  const [text, setText] = useState(props.note.text);
  const save = () => {
    if (text.trim()) updateNote(props.tabId, props.note.id, text);
    else deleteNote(props.tabId, props.note.id);
    props.onDone();
  };
  return (
    <div className="note-popover" style={{ left: props.left, top: props.top }}>
      <textarea
        autoFocus
        value={text}
        placeholder={t("notePlaceholder")}
        onChange={(e) => setText(e.target.value)}
        onKeyDown={(e) => {
          if (e.key === "Enter" && !e.shiftKey) {
            e.preventDefault();
            save();
          }
        }}
      />
      <div className="row">
        <button
          data-tip={t("tipDeleteNote")}
          onClick={() => {
            deleteNote(props.tabId, props.note.id);
            props.onDone();
          }}
        >
          {t("btnDelete")}
        </button>
        <button className="btn-primary" data-tip={t("tipSaveEnter")} onClick={save}>
          {t("btnSave")}
        </button>
      </div>
    </div>
  );
}

function NoteCard(props: { author: string; contents: string; left: number; top: number }) {
  return (
    <div className="note-popover" style={{ left: props.left, top: props.top }}>
      {props.author && <div className="note-author">{props.author}</div>}
      <div style={{ whiteSpace: "pre-wrap", userSelect: "text", WebkitUserSelect: "text" }}>
        {props.contents || t("noteNoText")}
      </div>
    </div>
  );
}

// The popover for the note that is open on this page: an editor for one of
// the tab's draft notes, a read-only card for an annotation already in the
// file (`ex:<src>:<index>`), or nothing when the open note lives elsewhere.
export default function NotePopover({
  openNote,
  notes,
  annots,
  src,
  viewport,
  iconSize,
  w,
  h,
  tabId,
  onDone,
}: {
  openNote: string | null;
  notes: Note[];
  annots: ExistingAnnot[];
  src: number;
  viewport: PageViewport;
  iconSize: number;
  w: number;
  h: number;
  tabId: string;
  onDone: () => void;
}) {
  if (!openNote) return null;
  const dn = notes.find((n) => n.id === openNote);
  if (dn) {
    const p = vpTopLeft(viewport, dn.x, dn.y, dn.x + NOTE_PT, dn.y - NOTE_PT);
    const pos = popoverPos(p.left, p.top, iconSize, w, h);
    return <NoteEditor key={dn.id} note={dn} tabId={tabId} left={pos.left} top={pos.top} onDone={onDone} />;
  }
  if (openNote.startsWith(`ex:${src}:`)) {
    const ai = Number(openNote.split(":")[2]);
    const a = annots[ai];
    if (a && a.subtype === "Text") {
      const p = vpTopLeft(viewport, a.rect[0], a.rect[1], a.rect[2], a.rect[3]);
      const pos = popoverPos(p.left, p.top, iconSize, w, h);
      return <NoteCard author={a.author} contents={a.contents} left={pos.left} top={pos.top} />;
    }
  }
  return null;
}
