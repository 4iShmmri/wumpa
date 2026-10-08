import { create } from "zustand";
import type { Tab, Snapshot, Highlight, Note } from "./types";
import { evictDoc } from "./pdf";

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
  tool: "select" | "note";

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
  setTool: (tool: "select" | "note") => void;
}

function snapshot(t: Tab): Snapshot {
  return structuredClone({ pages: t.pages, highlights: t.highlights, notes: t.notes });
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

  addTab: (tab) =>
    set((s) => ({ tabs: [...s.tabs, tab], activeId: tab.id })),

  closeTab: (id) =>
    set((s) => {
      evictDoc(id);
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
          past: [...t.past, snapshot(t)],
          future: t.future.slice(1),
          dirty: true,
          currentPage: Math.min(t.currentPage, next.pages.length),
          selected: [],
        };
      }),
    })),

  markSaved: (id, newPath, newName) =>
    set((s) => ({
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
          past: [],
          future: [],
          dirty: false,
          docVersion: t.docVersion + 1,
          selected: [],
        };
      }),
    })),

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
  setTool: (tool) => set({ tool }),
}));

export function useActiveTab(): Tab | null {
  return useStore((s) => s.tabs.find((t) => t.id === s.activeId) ?? null);
}

export function getActiveTab(): Tab | null {
  const s = useStore.getState();
  return s.tabs.find((t) => t.id === s.activeId) ?? null;
}
