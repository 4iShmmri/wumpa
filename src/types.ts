export interface PageSpec {
  src: number; // 1-based page number in the source file on disk; 0 = blank page
  rotation: number; // extra rotation delta, multiple of 90
  w?: number; // blank-page size in PDF points
  h?: number;
}

export interface Highlight {
  src: number;
  rects: [number, number, number, number][]; // PDF user-space coords
  color: [number, number, number];
}

export interface Note {
  id: string; // UI-only identity
  src: number;
  x: number; // top-left of an 18x18 note icon, PDF user space
  y: number;
  text: string;
}

export interface ObjDesc {
  id: number;
  kind: "text" | "image" | "form" | "path";
  bbox: [number, number, number, number];
}

export interface ObjectEdit {
  src: number;
  id: number;
  action: "delete" | "translate" | "scale";
  dx?: number;
  dy?: number;
  sx?: number;
  sy?: number;
  ax?: number;
  ay?: number;
}

export interface StampItem {
  id: string;
  src: number;
  rect: [number, number, number, number];
  kind: "text" | "signature" | "image";
  text?: string;
  size?: number;
  color?: string;
  fontFamily?: string;
  lineHeight?: number;
  sigDataUrl?: string;
}

export type ShapeKind = "ink" | "rect" | "ellipse" | "line" | "arrow";

export interface ShapeItem {
  id: string;
  src: number;
  kind: ShapeKind;
  rect: [number, number, number, number];
  strokes: [number, number][][];
  color: [number, number, number]; // 0..1 floats
  width: number;
  opacity: number;
}

export interface Snapshot {
  pages: PageSpec[];
  highlights: Highlight[];
  notes: Note[];
  edits: ObjectEdit[];
  stamps: StampItem[];
  shapes: ShapeItem[];
}

export interface Tab {
  id: string;
  path: string;
  name: string;
  pages: PageSpec[];
  highlights: Highlight[];
  notes: Note[];
  edits: ObjectEdit[];
  stamps: StampItem[];
  shapes: ShapeItem[];
  previewPath: string | null;
  past: Snapshot[];
  future: Snapshot[];
  zoom: number;
  currentPage: number; // 1-based index into pages[]
  selected: number[]; // 0-based indices into pages[]
  dirty: boolean;
  docVersion: number; // bumped when the file on disk changes (after save)
  detectedLang?: "en" | "ar";
}
