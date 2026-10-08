import { useStore, useActiveTab } from "../store";
import {
  openFileDialog, saveActive, mergeFiles, extractSelected,
  rotateSelection, deleteSelection, highlightSelection,
} from "../actions";

function I({ d, size = 17 }: { d: string; size?: number }) {
  return (
    <svg width={size} height={size} viewBox="0 0 24 24" fill="none" stroke="currentColor"
      strokeWidth="1.8" strokeLinecap="round" strokeLinejoin="round">
      <path d={d} />
    </svg>
  );
}

const icons = {
  open: "M22 19a2 2 0 0 1-2 2H4a2 2 0 0 1-2-2V5a2 2 0 0 1 2-2h5l2 3h9a2 2 0 0 1 2 2z",
  save: "M19 21H5a2 2 0 0 1-2-2V5a2 2 0 0 1 2-2h11l5 5v11a2 2 0 0 1-2 2z M17 21v-8H7v8 M7 3v5h8",
  undo: "M3 7v6h6 M21 17a9 9 0 0 0-15-6.7L3 13",
  redo: "M21 7v6h-6 M3 17a9 9 0 0 1 15-6.7L21 13",
  zoomOut: "M11 19a8 8 0 1 0 0-16 8 8 0 0 0 0 16z M21 21l-4.35-4.35 M8 11h6",
  zoomIn: "M11 19a8 8 0 1 0 0-16 8 8 0 0 0 0 16z M21 21l-4.35-4.35 M11 8v6 M8 11h6",
  fit: "M8 3H5a2 2 0 0 0-2 2v3 M16 3h3a2 2 0 0 1 2 2v3 M8 21H5a2 2 0 0 1-2-2v-3 M16 21h3a2 2 0 0 0 2-2v-3",
  rotL: "M2.5 2v6h6 M2.66 15.57a10 10 0 1 0 .57-8.38",
  rotR: "M21.5 2v6h-6 M21.34 15.57a10 10 0 1 1-.57-8.38",
  del: "M3 6h18 M19 6v14a2 2 0 0 1-2 2H7a2 2 0 0 1-2-2V6 M8 6V4a2 2 0 0 1 2-2h4a2 2 0 0 1 2 2v2 M10 11v6 M14 11v6",
  extract: "M14 2H6a2 2 0 0 0-2 2v16a2 2 0 0 0 2 2h12a2 2 0 0 0 2-2V8z M14 2v6h6 M12 18v-6 M9 15l3 3 3-3",
  merge: "M8 3H5a2 2 0 0 0-2 2v14a2 2 0 0 0 2 2h3 M16 3h3a2 2 0 0 1 2 2v14a2 2 0 0 1-2 2h-3 M12 8v8 M9 11l3-3 3 3",
  split: "M16 3h3a2 2 0 0 1 2 2v14a2 2 0 0 1-2 2h-3 M8 3H5a2 2 0 0 0-2 2v14a2 2 0 0 0 2 2h3 M12 4v16",
  highlight: "M9 11l-6 6v3h3l6-6 M14 6l4 4 3.5-3.5a2.1 2.1 0 0 0 0-3L20 2a2.1 2.1 0 0 0-3 0z M9 11l4-4 4 4-4 4z",
  note: "M21 15a2 2 0 0 1-2 2H7l-4 4V5a2 2 0 0 1 2-2h14a2 2 0 0 1 2 2z",
  search: "M11 19a8 8 0 1 0 0-16 8 8 0 0 0 0 16z M21 21l-4.35-4.35",
  sun: "M12 17a5 5 0 1 0 0-10 5 5 0 0 0 0 10z M12 1v2 M12 21v2 M4.22 4.22l1.42 1.42 M18.36 18.36l1.42 1.42 M1 12h2 M21 12h2 M4.22 19.78l1.42-1.42 M18.36 5.64l1.42-1.42",
  moon: "M21 12.79A9 9 0 1 1 11.21 3 7 7 0 0 0 21 12.79z",
};

export default function Toolbar({ onSplit }: { onSplit: () => void }) {
  const tab = useActiveTab();
  const theme = useStore((s) => s.theme);
  const searchOpen = useStore((s) => s.search.open);
  const tool = useStore((s) => s.tool);
  const st = useStore.getState;
  const has = !!tab;

  const setZoom = (z: number) => {
    if (!tab) return;
    st().patchTab(tab.id, { zoom: Math.max(0.25, Math.min(6, Math.round(z * 100) / 100)) });
  };

  return (
    <div className="toolbar">
      <button title="Open (⌘O)" onClick={() => openFileDialog()}><I d={icons.open} /></button>
      <button title="Save (⌘S)" disabled={!has} onClick={() => saveActive(false)}><I d={icons.save} /></button>
      <button className="txt" title="Save As (⇧⌘S)" disabled={!has} onClick={() => saveActive(true)}>
        Save&nbsp;As
      </button>
      <div className="sep" />
      <button title="Undo (⌘Z)" disabled={!has || !tab!.past.length} onClick={() => tab && st().undo(tab.id)}><I d={icons.undo} /></button>
      <button title="Redo (⇧⌘Z)" disabled={!has || !tab!.future.length} onClick={() => tab && st().redo(tab.id)}><I d={icons.redo} /></button>
      <div className="sep" />
      <button title="Zoom out (⌘-)" disabled={!has} onClick={() => tab && setZoom(tab.zoom / 1.2)}><I d={icons.zoomOut} /></button>
      <span className="zoom-label">{tab ? Math.round(tab.zoom * 100) + "%" : "—"}</span>
      <button title="Zoom in (⌘+)" disabled={!has} onClick={() => tab && setZoom(tab.zoom * 1.2)}><I d={icons.zoomIn} /></button>
      <button title="Fit width" disabled={!has} onClick={() => window.dispatchEvent(new CustomEvent("fitwidth"))}><I d={icons.fit} /></button>
      <div className="sep" />
      <button title="Rotate left" disabled={!has} onClick={() => rotateSelection(-90)}><I d={icons.rotL} /></button>
      <button title="Rotate right" disabled={!has} onClick={() => rotateSelection(90)}><I d={icons.rotR} /></button>
      <button title="Delete page(s)" disabled={!has} onClick={() => deleteSelection()}><I d={icons.del} /></button>
      <button title="Extract page(s) to a new PDF" disabled={!has} onClick={() => extractSelected()}><I d={icons.extract} /></button>
      <div className="sep" />
      <button className="txt" title="Merge PDF files" onClick={() => mergeFiles()}>
        <I d={icons.merge} />&nbsp;Merge
      </button>
      <button className="txt" title="Split into multiple files" disabled={!has} onClick={onSplit}>
        <I d={icons.split} />&nbsp;Split
      </button>
      <div className="sep" />
      <button title="Highlight selected text" disabled={!has} onClick={() => highlightSelection()}><I d={icons.highlight} /></button>
      <button
        title="Add note (N)"
        disabled={!has}
        className={tool === "note" ? "active" : ""}
        onClick={() => st().setTool(tool === "note" ? "select" : "note")}
      >
        <I d={icons.note} />
      </button>
      <div className="spacer" />
      <button
        title="Search (⌘F)"
        disabled={!has}
        className={searchOpen ? "active" : ""}
        onClick={() => st().setSearch({ open: !searchOpen })}
      >
        <I d={icons.search} />
      </button>
      <button title="Toggle theme" onClick={() => st().setTheme(theme === "dark" ? "light" : "dark")}>
        <I d={theme === "dark" ? icons.sun : icons.moon} />
      </button>
    </div>
  );
}
