import { useEffect } from "react";
import "./styles/index.css";
import TabBar from "./components/TabBar";
import Toolbar from "./components/Toolbar";
import Sidebar from "./components/Sidebar";
import Viewer from "./components/Viewer";
import SearchBar from "./components/SearchBar";
import StatusBar from "./components/StatusBar";
import SignatureModal from "./components/SignatureModal";
import Modals from "./components/modals";
import CompareView from "./components/CompareView";
import { useStore } from "./store";
import {
  openPaths,
  openFileDialog,
  saveActive,
  closeTabSafe,
  deleteSelection,
  deleteObject,
  deleteShape,
  printActive,
  runAutotest,
} from "./actions";
import { deleteStamp } from "./stamps";
import { readLocal } from "./storage";
import { onFileDrop, takePendingFiles, exitApp, listenEvent, confirmDialog } from "./backend";
import { schedulePreview } from "./preview";
import { t } from "./i18n";

export default function App() {
  const activeEdits = useStore((s) => s.tabs.find((t) => t.id === s.activeId)?.edits);

  useEffect(() => {
    const id = useStore.getState().activeId;
    if (id && activeEdits) schedulePreview(id);
  }, [activeEdits]);

  useEffect(() => {
    const cleanups: (() => void)[] = [];
    let disposed = false;
    const keep = (u: () => void) => (disposed ? u() : cleanups.push(u));
    onFileDrop((paths) => openPaths(paths)).then(keep);

    const drainPending = () =>
      takePendingFiles().then((paths) => {
        if (paths.length) openPaths(paths);
      });
    drainPending().then(() => void runAutotest());
    listenEvent("open-files-pending", () => void drainPending()).then(keep);
    listenEvent("close-requested", async () => {
      const dirty = useStore.getState().tabs.filter((t) => t.dirty);
      if (!dirty.length) {
        exitApp();
        return;
      }
      const ok = await confirmDialog(t("cfQuitUnsaved", { n: dirty.length }));
      if (ok) exitApp();
    }).then(keep);

    return () => {
      disposed = true;
      cleanups.forEach((u) => u());
    };
  }, []);

  useEffect(() => {
    const onKey = (e: KeyboardEvent) => {
      const mod = e.metaKey || e.ctrlKey;
      const st = useStore.getState();
      // Dialogs and the compare view own the keyboard while they are open.
      if (st.utilModal || st.signatureModalOpen || st.passwordPrompt || st.compare || st.compareDialog)
        return;
      const tab = st.tabs.find((t) => t.id === st.activeId);
      const el = document.activeElement;
      const inInput =
        e.isComposing ||
        el instanceof HTMLInputElement ||
        el instanceof HTMLTextAreaElement ||
        el instanceof HTMLSelectElement ||
        (el instanceof HTMLElement && el.isContentEditable);

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
      } else if ((e.key === "Delete" || e.key === "Backspace") && tab && !inInput && st.stampSel) {
        e.preventDefault();
        deleteStamp(tab.id, st.stampSel.id);
      } else if ((e.key === "Delete" || e.key === "Backspace") && tab && !inInput && st.shapeSel) {
        e.preventDefault();
        deleteShape(st.shapeSel.tabId, st.shapeSel.id);
      } else if ((e.key === "Delete" || e.key === "Backspace") && tab && !inInput && st.objSel) {
        e.preventDefault();
        deleteObject(tab.id, st.objSel.src, st.objSel.id);
      } else if (
        (e.key === "Delete" || e.key === "Backspace") &&
        tab &&
        !inInput &&
        tab.selected.length > 0 &&
        (window.getSelection()?.isCollapsed ?? true)
      ) {
        e.preventDefault();
        deleteSelection();
      } else if (!mod && !e.altKey && e.key.toLowerCase() === "n" && tab && !inInput) {
        e.preventDefault();
        st.setTool(st.tool === "note" ? "select" : "note");
      } else if (!mod && !e.altKey && e.key.toLowerCase() === "e" && tab && !inInput) {
        e.preventDefault();
        st.setTool(st.tool === "edit" ? "select" : "edit");
      } else if (!mod && !e.altKey && e.key.toLowerCase() === "r" && tab && !inInput) {
        e.preventDefault();
        st.setTool(st.tool === "erase" ? "select" : "erase");
      } else if (!mod && !e.altKey && e.key.toLowerCase() === "t" && tab && !inInput) {
        e.preventDefault();
        st.setTool(st.tool === "text" ? "select" : "text");
      } else if (!mod && !e.altKey && e.key.toLowerCase() === "d" && tab && !inInput) {
        e.preventDefault();
        st.setTool(st.tool === "draw" ? "select" : "draw");
      } else if (!mod && !e.altKey && e.key.toLowerCase() === "l" && tab && !inInput) {
        e.preventDefault();
        st.setTool(st.tool === "link" ? "select" : "link");
      } else if (!mod && !e.altKey && e.key.toLowerCase() === "s" && tab && !inInput) {
        e.preventDefault();
        if (st.tool === "sign") {
          st.setTool("select");
        } else {
          const sig = readLocal("signature");
          if (sig) st.setTool("sign");
          else st.setSignatureModal(true);
        }
      } else if (tab && !inInput && (e.key === "PageDown" || (e.altKey && e.key === "ArrowDown"))) {
        e.preventDefault();
        st.scrollTo(Math.min(tab.pages.length, tab.currentPage + 1));
      } else if (tab && !inInput && (e.key === "PageUp" || (e.altKey && e.key === "ArrowUp"))) {
        e.preventDefault();
        st.scrollTo(Math.max(1, tab.currentPage - 1));
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
      <Toolbar />
      <div className="main">
        <Sidebar />
        <Viewer />
        <SearchBar />
      </div>
      <StatusBar />
      <SignatureModal />
      <Modals />
      <CompareView />
    </div>
  );
}
