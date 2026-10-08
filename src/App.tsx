import { useEffect, useState } from "react";
import "./App.css";
import TabBar from "./components/TabBar";
import Toolbar from "./components/Toolbar";
import Sidebar from "./components/Sidebar";
import Viewer from "./components/Viewer";
import SearchBar from "./components/SearchBar";
import StatusBar from "./components/StatusBar";
import SignatureModal from "./components/SignatureModal";
import { useStore } from "./store";
import {
  openPaths, openFileDialog, saveActive, closeTabSafe,
  deleteSelection, splitEvery, deleteObject, deleteShape, printActive,
} from "./actions";
import { deleteStamp } from "./stamps";
import { onFileDrop, takePendingFiles, exitApp, listenEvent, confirmDialog } from "./backend";
import { schedulePreview } from "./preview";

function SplitModal({ onClose }: { onClose: () => void }) {
  const [n, setN] = useState(1);
  return (
    <div className="modal-backdrop" onClick={onClose}>
      <div className="modal" onClick={(e) => e.stopPropagation()}>
        <h3>Split document</h3>
        <label>
          Split every
          <input
            type="number"
            min={1}
            value={n}
            autoFocus
            onChange={(e) => setN(Math.max(1, Number(e.target.value) || 1))}
            onKeyDown={(e) => {
              if (e.key === "Enter") {
                onClose();
                splitEvery(n);
              }
              if (e.key === "Escape") onClose();
            }}
          />
          page(s)
        </label>
        <div className="modal-actions">
          <button onClick={onClose}>Cancel</button>
          <button
            className="btn-primary"
            onClick={() => {
              onClose();
              splitEvery(n);
            }}
          >
            Choose folder &amp; split
          </button>
        </div>
      </div>
    </div>
  );
}

export default function App() {
  const [splitOpen, setSplitOpen] = useState(false);
  const activeEdits = useStore((s) => s.tabs.find((t) => t.id === s.activeId)?.edits);

  useEffect(() => {
    const id = useStore.getState().activeId;
    if (id && activeEdits) schedulePreview(id);
  }, [activeEdits]);

  useEffect(() => {
    const cleanups: (() => void)[] = [];
    onFileDrop((paths) => openPaths(paths)).then((u) => cleanups.push(u));

    const drainPending = () =>
      takePendingFiles().then((paths) => {
        if (paths.length) openPaths(paths);
      });
    drainPending();
    listenEvent("open-files-pending", () => void drainPending()).then((u) => cleanups.push(u));
    listenEvent("close-requested", async () => {
      const dirty = useStore.getState().tabs.filter((t) => t.dirty);
      if (!dirty.length) {
        exitApp();
        return;
      }
      const ok = await confirmDialog(
        `${dirty.length} file(s) have unsaved changes. Quit anyway?`,
      );
      if (ok) exitApp();
    }).then((u) => cleanups.push(u));

    return () => cleanups.forEach((u) => u());
  }, []);

  useEffect(() => {
    const onKey = (e: KeyboardEvent) => {
      const mod = e.metaKey || e.ctrlKey;
      const st = useStore.getState();
      const tab = st.tabs.find((t) => t.id === st.activeId);
      const inInput =
        document.activeElement instanceof HTMLInputElement ||
        document.activeElement instanceof HTMLTextAreaElement;

      if (mod && e.key.toLowerCase() === "o") {
        e.preventDefault();
        openFileDialog();
      } else if (mod && e.key.toLowerCase() === "s") {
        e.preventDefault();
        saveActive(e.shiftKey);
      } else if (mod && e.key.toLowerCase() === "p" && tab) {
        e.preventDefault();
        printActive();
      } else if (mod && e.key.toLowerCase() === "f" && tab) {
        e.preventDefault();
        st.setSearch({ open: true });
      } else if (mod && e.key.toLowerCase() === "w" && tab) {
        e.preventDefault();
        closeTabSafe(tab.id);
      } else if (mod && e.key.toLowerCase() === "z" && tab && !inInput) {
        e.preventDefault();
        if (e.shiftKey) st.redo(tab.id);
        else st.undo(tab.id);
      } else if (mod && e.key.toLowerCase() === "y" && tab && !inInput) {
        e.preventDefault();
        st.redo(tab.id);
      } else if (mod && (e.key === "=" || e.key === "+") && tab) {
        e.preventDefault();
        st.patchTab(tab.id, { zoom: Math.min(6, Math.round(tab.zoom * 1.2 * 100) / 100) });
      } else if (mod && e.key === "-" && tab) {
        e.preventDefault();
        st.patchTab(tab.id, { zoom: Math.max(0.25, Math.round((tab.zoom / 1.2) * 100) / 100) });
      } else if (mod && e.key === "0" && tab) {
        e.preventDefault();
        st.patchTab(tab.id, { zoom: 1 });
      } else if (
        (e.key === "Delete" || e.key === "Backspace") &&
        tab && !inInput && st.stampSel
      ) {
        e.preventDefault();
        deleteStamp(tab.id, st.stampSel.id);
      } else if (
        (e.key === "Delete" || e.key === "Backspace") &&
        tab && !inInput && st.shapeSel
      ) {
        e.preventDefault();
        deleteShape(st.shapeSel.tabId, st.shapeSel.id);
      } else if (
        (e.key === "Delete" || e.key === "Backspace") &&
        tab && !inInput && st.objSel
      ) {
        e.preventDefault();
        deleteObject(tab.id, st.objSel.src, st.objSel.id);
      } else if (
        (e.key === "Delete" || e.key === "Backspace") &&
        tab && !inInput && tab.selected.length > 0 &&
        (window.getSelection()?.isCollapsed ?? true)
      ) {
        e.preventDefault();
        deleteSelection();
      } else if (
        !mod && !e.altKey && e.key.toLowerCase() === "n" && tab && !inInput
      ) {
        e.preventDefault();
        st.setTool(st.tool === "note" ? "select" : "note");
      } else if (
        !mod && !e.altKey && e.key.toLowerCase() === "e" && tab && !inInput
      ) {
        e.preventDefault();
        st.setTool(st.tool === "edit" ? "select" : "edit");
      } else if (
        !mod && !e.altKey && e.key.toLowerCase() === "r" && tab && !inInput
      ) {
        e.preventDefault();
        st.setTool(st.tool === "erase" ? "select" : "erase");
      } else if (
        !mod && !e.altKey && e.key.toLowerCase() === "t" && tab && !inInput
      ) {
        e.preventDefault();
        st.setTool(st.tool === "text" ? "select" : "text");
      } else if (
        !mod && !e.altKey && e.key.toLowerCase() === "d" && tab && !inInput
      ) {
        e.preventDefault();
        st.setTool(st.tool === "draw" ? "select" : "draw");
      } else if (
        !mod && !e.altKey && e.key.toLowerCase() === "s" && tab && !inInput
      ) {
        e.preventDefault();
        if (st.tool === "sign") {
          st.setTool("select");
        } else {
          let sig: string | null = null;
          try {
            sig = localStorage.getItem("signature");
          } catch {
            sig = null;
          }
          if (sig) st.setTool("sign");
          else st.setSignatureModal(true);
        }
      } else if (e.key === "Escape" && tab && !inInput) {
        if (st.stampSel) st.setStampSel(null);
        else if (st.shapeSel) st.setShapeSel(null);
        else if (st.objSel) st.setObjSel(null);
        else if (st.tool !== "select") st.setTool("select");
        else if (st.search.open) st.setSearch({ open: false });
        else if (tab.selected.length) st.patchTab(tab.id, { selected: [] });
      }
    };
    window.addEventListener("keydown", onKey);
    return () => window.removeEventListener("keydown", onKey);
  }, []);

  return (
    <div className="app">
      <TabBar />
      <Toolbar onSplit={() => setSplitOpen(true)} />
      <div className="main">
        <Sidebar />
        <Viewer />
        <SearchBar />
      </div>
      <StatusBar />
      <SignatureModal />
      {splitOpen && <SplitModal onClose={() => setSplitOpen(false)} />}
    </div>
  );
}
