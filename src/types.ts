export interface PageSpec {
  src: number; // 1-based page in the source file; blank pages carry a unique negative value
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

export type Rect = [number, number, number, number];

export interface Bookmark {
  id: string; // UI-only identity
  title: string;
  src: number; // 1-based source page; 0 = no page target
  top?: number | null; // PDF user-space y for an /XYZ target; null = fit page
  url?: string | null; // URI action instead of a page target
  open?: boolean;
  children: Bookmark[];
}

export interface LinkItem {
  id: string; // UI-only identity
  src: number; // source page the link sits on
  rect: Rect; // PDF user space, x1 < x2, y1 < y2
  url?: string | null; // external target
  destSrc?: number | null; // internal target: source page number
}

export interface LinkRemoval {
  src: number;
  rect: Rect; // rect of an existing link annotation to drop on save
}

export interface SecurityPolicy {
  maxAttempts: number;
  wipe: boolean;
}

export interface SecurityInfo {
  encrypted: boolean;
  needsPassword: boolean;
  method: string;
  permissions: { print: boolean; copy: boolean; modify: boolean; annotate: boolean };
  policy?: SecurityPolicy | null;
}

export interface AttemptStatus {
  failures: number;
  remaining?: number | null;
  lockedSecs: number;
  wipe: boolean;
  wiped: boolean;
}

export interface Snapshot {
  pages: PageSpec[];
  highlights: Highlight[];
  notes: Note[];
  edits: ObjectEdit[];
  stamps: StampItem[];
  shapes: ShapeItem[];
  bookmarks: Bookmark[] | null;
  links: LinkItem[];
  linkRemovals: LinkRemoval[];
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
  bookmarks: Bookmark[] | null; // null = not loaded yet
  bookmarksTouched?: boolean; // the outline is rewritten on save only after an edit
  links: LinkItem[];
  linkRemovals: LinkRemoval[];
  security?: SecurityInfo | null; // password protection of the file on disk
  untitled?: boolean; // lives in the temp dir until the first Save As
  saving?: boolean; // edits are refused while the file is being written
  layoutNonce?: number; // bumped when late-measured page sizes arrive
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
