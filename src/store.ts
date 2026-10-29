import { create } from "zustand";
import type {
  AttemptStatus,
  Tab,
  Snapshot,
  Highlight,
  Note,
  ObjectEdit,
  StampItem,
  ShapeItem,
  ShapeKind,
  LinkItem,
  LinkRemoval,
} from "./types";
import { evictDoc } from "./pdf";
import { clearPreview } from "./preview";
import type { LangPref } from "./i18n";
import { evictObjects } from "./objects";

export type Tool = "select" | "note" | "edit" | "erase" | "text" | "sign" | "draw" | "link";
export type UtilModal =
  | "compress"
  | "qr"
  | "newpdf"
  | "table"
  | "split"
  | "encrypt"
  | "decrypt"
  | "scan"
  | "exportDocx"
  | "exportXlsx";

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

export interface CompareState {
  leftKey: string;
  leftPath: string;
  leftVersion: number;
  leftName: string;
  rightKey: string;
  rightPath: string;
  rightVersion: number;
  rightName: string;
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
  fitNonce: number;
  passwordPrompt: {
    name: string;
    wrong: boolean;
    status: AttemptStatus | null;
    resolve: (pw: string | null) => void;
  } | null;
  utilModal: UtilModal | null;
  qrPreset: string | null;
  shapeSel: { tabId: string; id: string } | null;
  drawKind: ShapeKind;
  drawColor: [number, number, number];
  drawWidth: number;
  lang: LangPref;
  effectiveLang: "en" | "ar";
  compare: CompareState | null;
  compareDialog: { oldPath: string | null } | null;

  addTab: (tab: Tab) => void;
  setLang: (lang: LangPref) => void;
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
  askPassword: (name: string, wrong: boolean, status: AttemptStatus | null) => Promise<string | null>;
  fitWidth: () => void;
  setUtilModal: (m: UtilModal | null) => void;
  setQrPreset: (v: string | null) => void;
  setShapeSel: (sel: { tabId: string; id: string } | null) => void;
  setDrawKind: (k: ShapeKind) => void;
  setDrawColor: (c: [number, number, number]) => void;
  setDrawWidth: (w: number) => void;
  openCompare: (c: CompareState) => void;
  closeCompare: () => void;
  openCompareDialog: (oldPath: string | null) => void;
  closeCompareDialog: () => void;
}

function snapshot(t: Tab): Snapshot {
  return structuredClone({
    pages: t.pages,
    highlights: t.highlights,
    notes: t.notes,
    edits: t.edits,
    stamps: t.stamps,
    shapes: t.shapes,
    bookmarks: t.bookmarks,
    links: t.links,
    linkRemovals: t.linkRemovals,
  });
}

function restore(s: Snapshot) {
  return {
    pages: s.pages,
    highlights: s.highlights,
    notes: s.notes,
    edits: s.edits,
    stamps: s.stamps,
    shapes: s.shapes,
    bookmarks: s.bookmarks,
    links: s.links,
    linkRemovals: s.linkRemovals,
  };
}

const initialTheme: "light" | "dark" =
  (localStorage.getItem("theme") as "light" | "dark") ??
  (window.matchMedia("(prefers-color-scheme: dark)").matches ? "dark" : "light");
document.documentElement.dataset.theme = initialTheme;

function readLangPref(): LangPref {
  try {
    const v = localStorage.getItem("lang");
    return v === "en" || v === "ar" || v === "auto" ? v : "auto";
  } catch {
    return "auto";
  }
}

function computeEffective(lang: LangPref, tabs: Tab[], activeId: string | null): "en" | "ar" {
  if (lang !== "auto") return lang;
  return tabs.find((t) => t.id === activeId)?.detectedLang ?? "en";
}

const initialLang = readLangPref();
let passwordQueue: Promise<void> = Promise.resolve();

// Per-document selections never survive a change of active tab.
const selectionReset = {
  search: { open: false, query: "", matches: [] as number[], current: 0 },
  tool: "select" as Tool,
  objSel: null,
  stampSel: null,
  stampEditing: null,
  shapeSel: null,
};

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
  fitNonce: 0,
  passwordPrompt: null,
  utilModal: null,
  qrPreset: null,
  shapeSel: null,
  drawKind: "ink",
  drawColor: [0.86, 0.21, 0.18],
  drawWidth: 2.5,
  lang: initialLang,
  effectiveLang: computeEffective(initialLang, [], null),
  compare: null,
  compareDialog: null,

  setLang: (lang) => {
    try {
      localStorage.setItem("lang", lang);
    } catch {
      /* best effort */
    }
    set({ lang });
  },

  addTab: (tab) => set((s) => ({ tabs: [...s.tabs, tab], activeId: tab.id })),

  closeTab: (id) => {
    evictDoc(id);
    evictObjects(id);
    clearPreview(id);
    set((s) => {
      const tabs = s.tabs.filter((t) => t.id !== id);
      if (s.activeId !== id) return { tabs };
      const idx = s.tabs.findIndex((t) => t.id === id);
      const activeId = tabs[Math.min(idx, tabs.length - 1)]?.id ?? null;
      return { tabs, activeId, ...selectionReset };
    });
  },

  setActive: (id) => set({ activeId: id, ...selectionReset }),

  patchTab: (id, patch) => set((s) => ({ tabs: s.tabs.map((t) => (t.id === id ? { ...t, ...patch } : t)) })),

  mutateDoc: (id, fn) =>
    set((s) => ({
      tabs: s.tabs.map((t) => {
        if (t.id !== id || t.saving) return t;
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
          ...restore(prev),
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
          ...restore(next),
          past: [...t.past, snapshot(t)],
          future: t.future.slice(1),
          dirty: true,
          currentPage: Math.min(t.currentPage, next.pages.length),
          selected: [],
        };
      }),
    })),

  markSaved: (id, newPath, newName) => {
    evictObjects(id);
    set((s) => {
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
            bookmarks: null,
            bookmarksTouched: false,
            links: [] as LinkItem[],
            linkRemovals: [] as LinkRemoval[],
            previewPath: null,
            past: [],
            future: [],
            dirty: false,
            untitled: false,
            saving: false,
            docVersion: t.docVersion + 1,
            selected: [],
          };
        }),
      };
    });
  },

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
  scrollTo: (page) => set((s) => ({ scrollTarget: { page, nonce: (s.scrollTarget?.nonce ?? 0) + 1 } })),
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
  // WebViews have no window.prompt, so password requests go through an in-app
  // modal, one at a time (two protected files opened together both ask).
  askPassword: (name, wrong, status) => {
    const show = () =>
      new Promise<string | null>((resolve) => {
        set({
          passwordPrompt: {
            name,
            wrong,
            status,
            resolve: (pw) => {
              set({ passwordPrompt: null });
              resolve(pw);
            },
          },
        });
      });
    const next = passwordQueue.then(show, show);
    passwordQueue = next.then(
      () => undefined,
      () => undefined,
    );
    return next;
  },
  fitWidth: () => set((s) => ({ fitNonce: s.fitNonce + 1 })),
  setUtilModal: (utilModal) => set({ utilModal }),
  setQrPreset: (qrPreset) => set({ qrPreset }),
  setShapeSel: (shapeSel) =>
    set((s) => ({
      shapeSel,
      objSel: shapeSel ? null : s.objSel,
      stampSel: shapeSel ? null : s.stampSel,
    })),
  setDrawKind: (drawKind) => set({ drawKind }),
  setDrawColor: (drawColor) => set({ drawColor }),
  setDrawWidth: (drawWidth) => set({ drawWidth }),
  openCompare: (compare) => set({ compare, compareDialog: null }),
  openCompareDialog: (oldPath) => set({ compareDialog: { oldPath } }),
  closeCompareDialog: () => set({ compareDialog: null }),
  closeCompare: () => {
    const c = get().compare;
    if (c) {
      evictDoc(c.leftKey);
      evictDoc(c.rightKey);
    }
    set({ compare: null });
  },
}));

function applyLangAttrs(l: "en" | "ar") {
  document.documentElement.dir = l === "ar" ? "rtl" : "ltr";
  document.documentElement.lang = l;
}

applyLangAttrs(useStore.getState().effectiveLang);

useStore.subscribe((s) => {
  const next = computeEffective(s.lang, s.tabs, s.activeId);
  if (next !== s.effectiveLang) {
    useStore.setState({ effectiveLang: next });
    applyLangAttrs(next);
  }
});

export function useActiveTab(): Tab | null {
  return useStore((s) => s.tabs.find((t) => t.id === s.activeId) ?? null);
}

export function getActiveTab(): Tab | null {
  const s = useStore.getState();
  return s.tabs.find((t) => t.id === s.activeId) ?? null;
}
