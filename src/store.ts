import { create } from "zustand";
import type { Tab, Snapshot, Highlight, Note, ObjectEdit, StampItem, ShapeItem, ShapeKind } from "./types";
import { evictDoc } from "./pdf";
import { evictObjects } from "./objects";

export type Tool = "select" | "note" | "edit" | "erase" | "text" | "sign" | "draw";

export interface ObjSel {
  src: number;
  id: number;
}

export interface SearchState {
  open: boolean;
  query: string;
  matches: number[]; // output page index (0-based), one entry per occurrence
  current: number; // index into matches
}

interface AppState {
  tabs: Tab[];
  activeId: string | null;
  theme: "light" | "dark";
  search: SearchState;
  status: string;
  scrollTarget: { page: number; nonce: number } | null;
  tool: Tool;
  objSel: ObjSel | null;
  stampSel: { src: number; id: string } | null;
  stampEditing: string | null;
  signatureModalOpen: boolean;
  shapeSel: { tabId: string; id: string } | null;
  drawKind: ShapeKind;
  drawColor: [number, number, number];
  drawWidth: number;

  addTab: (tab: Tab) => void;
  closeTab: (id: string) => void;
  setActive: (id: string) => void;
  patchTab: (id: string, patch: Partial<Tab>) => void;
  mutateDoc: (id: string, fn: (t: Tab) => Partial<Tab>) => void;
  undo: (id: string) => void;
  redo: (id: string) => void;
  markSaved: (id: string, newPath: string, newName: string) => void;
  setTheme: (t: "light" | "dark") => void;
  setSearch: (s: Partial<SearchState>) => void;
  setStatus: (s: string) => void;
  scrollTo: (page: number) => void;
  setTool: (tool: Tool) => void;
  setObjSel: (sel: ObjSel | null) => void;
  setStampSel: (sel: { src: number; id: string } | null) => void;
  setStampEditing: (id: string | null) => void;
  setSignatureModal: (open: boolean) => void;
  setShapeSel: (sel: { tabId: string; id: string } | null) => void;
  setDrawKind: (k: ShapeKind) => void;
  setDrawColor: (c: [number, number, number]) => void;
  setDrawWidth: (w: number) => void;
}

function snapshot(t: Tab): Snapshot {
  return structuredClone({
    pages: t.pages,
    highlights: t.highlights,
    notes: t.notes,
    edits: t.edits,
    stamps: t.stamps,
    shapes: t.shapes,
  });
}

const initialTheme: "light" | "dark" =
  (localStorage.getItem("theme") as "light" | "dark") ??
  (window.matchMedia("(prefers-color-scheme: dark)").matches ? "dark" : "light");
document.documentElement.dataset.theme = initialTheme;

export const useStore = create<AppState>((set, get) => ({
  tabs: [],
  activeId: null,
  theme: initialTheme,
  search: { open: false, query: "", matches: [], current: 0 },
  status: "",
  scrollTarget: null,
  tool: "select",
  objSel: null,
  stampSel: null,
  stampEditing: null,
  signatureModalOpen: false,
  shapeSel: null,
  drawKind: "ink",
  drawColor: [0.86, 0.21, 0.18],
  drawWidth: 2.5,

  addTab: (tab) =>
    set((s) => ({ tabs: [...s.tabs, tab], activeId: tab.id })),

  closeTab: (id) =>
    set((s) => {
      evictDoc(id);
      evictObjects(id);
      const tabs = s.tabs.filter((t) => t.id !== id);
      let activeId = s.activeId;
      if (activeId === id) {
        const idx = s.tabs.findIndex((t) => t.id === id);
        activeId = tabs[Math.min(idx, tabs.length - 1)]?.id ?? null;
      }
      return { tabs, activeId };
    }),

  setActive: (id) =>
    set({
      activeId: id,
      search: { open: false, query: "", matches: [], current: 0 },
      tool: "select",
      objSel: null,
      stampSel: null,
      stampEditing: null,
      shapeSel: null,
    }),

  patchTab: (id, patch) =>
    set((s) => ({ tabs: s.tabs.map((t) => (t.id === id ? { ...t, ...patch } : t)) })),

  mutateDoc: (id, fn) =>
    set((s) => ({
      tabs: s.tabs.map((t) => {
        if (t.id !== id) return t;
        const patch = fn(t);
        return {
          ...t,
          past: [...t.past, snapshot(t)].slice(-100),
          future: [],
          dirty: true,
          ...patch,
        };
      }),
    })),

  undo: (id) =>
    set((s) => ({
      tabs: s.tabs.map((t) => {
        if (t.id !== id || t.past.length === 0) return t;
        const prev = t.past[t.past.length - 1];
        return {
          ...t,
          pages: prev.pages,
          highlights: prev.highlights,
          notes: prev.notes,
          edits: prev.edits,
          stamps: prev.stamps,
          shapes: prev.shapes,
          past: t.past.slice(0, -1),
          future: [snapshot(t), ...t.future],
          dirty: true,
          currentPage: Math.min(t.currentPage, prev.pages.length),
          selected: [],
        };
      }),
    })),

  redo: (id) =>
    set((s) => ({
      tabs: s.tabs.map((t) => {
        if (t.id !== id || t.future.length === 0) return t;
        const next = t.future[0];
        return {
          ...t,
          pages: next.pages,
          highlights: next.highlights,
          notes: next.notes,
          edits: next.edits,
          stamps: next.stamps,
          shapes: next.shapes,
          past: [...t.past, snapshot(t)],
          future: t.future.slice(1),
          dirty: true,
          currentPage: Math.min(t.currentPage, next.pages.length),
          selected: [],
        };
      }),
    })),

  markSaved: (id, newPath, newName) =>
    set((s) => {
      evictObjects(id);
      return {
        objSel: null,
        stampSel: null,
        stampEditing: null,
        shapeSel: null,
        tabs: s.tabs.map((t) => {
          if (t.id !== id) return t;
          const n = t.pages.length;
          return {
            ...t,
            path: newPath,
            name: newName,
            pages: Array.from({ length: n }, (_, i) => ({ src: i + 1, rotation: 0 })),
            highlights: [] as Highlight[],
            notes: [] as Note[],
            edits: [] as ObjectEdit[],
            stamps: [] as StampItem[],
            shapes: [] as ShapeItem[],
            previewPath: null,
            past: [],
            future: [],
            dirty: false,
            docVersion: t.docVersion + 1,
            selected: [],
          };
        }),
      };
    }),

  setTheme: (theme) => {
    localStorage.setItem("theme", theme);
    document.documentElement.dataset.theme = theme;
    set({ theme });
  },

  setSearch: (patch) => set((s) => ({ search: { ...s.search, ...patch } })),
  setStatus: (status) => {
    set({ status });
    if (status) {
      const current = status;
      setTimeout(() => {
        if (get().status === current) set({ status: "" });
      }, 5000);
    }
  },
  scrollTo: (page) =>
    set((s) => ({ scrollTarget: { page, nonce: (s.scrollTarget?.nonce ?? 0) + 1 } })),
  setTool: (tool) =>
    set((s) => ({ tool, objSel: tool === "edit" ? s.objSel : null, stampSel: null, shapeSel: null })),
  setObjSel: (objSel) =>
    set((s) => ({
      objSel,
      stampSel: objSel ? null : s.stampSel,
      shapeSel: objSel ? null : s.shapeSel,
    })),
  setStampSel: (stampSel) =>
    set((s) => ({
      stampSel,
      objSel: stampSel ? null : s.objSel,
      shapeSel: stampSel ? null : s.shapeSel,
    })),
  setStampEditing: (stampEditing) => set({ stampEditing }),
  setSignatureModal: (signatureModalOpen) => set({ signatureModalOpen }),
  setShapeSel: (shapeSel) =>
    set((s) => ({
      shapeSel,
      objSel: shapeSel ? null : s.objSel,
      stampSel: shapeSel ? null : s.stampSel,
    })),
  setDrawKind: (drawKind) => set({ drawKind }),
  setDrawColor: (drawColor) => set({ drawColor }),
  setDrawWidth: (drawWidth) => set({ drawWidth }),
}));

export function useActiveTab(): Tab | null {
  return useStore((s) => s.tabs.find((t) => t.id === s.activeId) ?? null);
}

export function getActiveTab(): Tab | null {
  const s = useStore.getState();
  return s.tabs.find((t) => t.id === s.activeId) ?? null;
}
