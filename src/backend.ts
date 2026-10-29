// Bridge to the Tauri backend, with browser fallbacks so the UI can be
// developed and tested in a plain browser (npm run dev).
import { invoke } from "@tauri-apps/api/core";
import type {
  PageSpec,
  Highlight,
  ObjDesc,
  ObjectEdit,
  ShapeItem,
  SecurityInfo,
  SecurityPolicy,
  AttemptStatus,
} from "./types";

export const inTauri = "__TAURI_INTERNALS__" in window;

export async function loadPdfBytes(path: string): Promise<ArrayBuffer> {
  if (inTauri) return await invoke<ArrayBuffer>("load_pdf", { path });
  return await (await fetch(path)).arrayBuffer();
}

export async function pickPdfs(multiple = true): Promise<string[] | null> {
  if (inTauri) {
    const { open } = await import("@tauri-apps/plugin-dialog");
    const res = await open({
      multiple,
      filters: [{ name: "PDF", extensions: ["pdf"] }],
    });
    if (!res) return null;
    return Array.isArray(res) ? res : [res];
  }
  return ["/sample.pdf"];
}

export async function pickSavePath(defaultName: string): Promise<string | null> {
  if (inTauri) {
    const { save } = await import("@tauri-apps/plugin-dialog");
    return await save({
      defaultPath: defaultName,
      filters: [{ name: "PDF", extensions: ["pdf"] }],
    });
  }
  alert("Saving requires the desktop app");
  return null;
}

export async function pickSavePathWithExt(
  defaultName: string,
  ext: string,
  filterName: string,
): Promise<string | null> {
  if (inTauri) {
    const { save } = await import("@tauri-apps/plugin-dialog");
    return await save({
      defaultPath: defaultName,
      filters: [{ name: filterName, extensions: [ext] }],
    });
  }
  alert("Saving requires the desktop app");
  return null;
}

export async function pickDirectory(): Promise<string | null> {
  if (inTauri) {
    const { open } = await import("@tauri-apps/plugin-dialog");
    const res = await open({ directory: true });
    return typeof res === "string" ? res : null;
  }
  alert("This requires the desktop app");
  return null;
}

export async function confirmDialog(message: string): Promise<boolean> {
  if (inTauri) {
    const { confirm } = await import("@tauri-apps/plugin-dialog");
    return await confirm(message, { kind: "warning" });
  }
  return window.confirm(message);
}

export interface NotePayload {
  src: number;
  x: number;
  y: number;
  text: string;
}

export interface WireStamp {
  src: number;
  rect: [number, number, number, number];
  w: number;
  h: number;
  rgbaB64: string;
}

export type WireShape = Omit<ShapeItem, "id">;

// page fields below are 0-based indices into the OUTPUT page order (pages[]).
export interface WireOutline {
  title: string;
  page: number | null;
  top: number | null;
  url: string | null;
  open: boolean;
  children: WireOutline[];
}

export interface WireLink {
  page: number;
  rect: [number, number, number, number];
  url: string | null;
  destPage: number | null;
}

export interface WireLinkRemoval {
  page: number;
  rect: [number, number, number, number];
}

export interface SaveExtras {
  outline?: WireOutline[] | null;
  links?: WireLink[];
  linkRemovals?: WireLinkRemoval[];
}

export async function saveDocument(
  srcPath: string,
  outPath: string,
  pages: PageSpec[],
  highlights: Highlight[],
  notes: NotePayload[],
  edits: ObjectEdit[],
  stamps: WireStamp[],
  shapes: WireShape[],
  extras: SaveExtras = {},
): Promise<void> {
  const { outline = null, links = [], linkRemovals = [] } = extras;
  if (!inTauri) {
    console.log("saveDocument", {
      srcPath,
      outPath,
      pages,
      highlights,
      notes,
      edits,
      stamps,
      shapes,
      outline,
      links,
      linkRemovals,
    });
    return;
  }
  await invoke("save_document", {
    srcPath,
    outPath,
    pages,
    highlights,
    notes,
    edits,
    stamps,
    shapes,
    outline,
    links,
    linkRemovals,
  });
}

export async function printDocument(
  srcPath: string,
  pages: PageSpec[],
  highlights: Highlight[],
  notes: NotePayload[],
  edits: ObjectEdit[],
  stamps: WireStamp[],
  shapes: WireShape[],
): Promise<void> {
  if (!inTauri) {
    window.print();
    return;
  }
  await invoke("print_document", { srcPath, pages, highlights, notes, edits, stamps, shapes });
}

export async function savePng(path: string, dataB64: string): Promise<void> {
  if (!inTauri) return;
  await invoke("save_png", { path, dataB64 });
}

export async function listPageObjects(srcPath: string, page: number): Promise<ObjDesc[]> {
  if (!inTauri) return [];
  return await invoke<ObjDesc[]>("list_page_objects", { srcPath, page });
}

export async function applyEditsPreview(srcPath: string, edits: ObjectEdit[]): Promise<string | null> {
  if (!inTauri) return null;
  return await invoke<string>("apply_edits_preview", { srcPath, edits });
}

export async function takePendingFiles(): Promise<string[]> {
  if (!inTauri) return [];
  return await invoke<string[]>("take_pending_files");
}

// Debug builds only: a file to open and export on startup, for automated testing.
export interface AutotestPlan {
  src: string;
  out: string;
}

export async function autotestPlan(): Promise<AutotestPlan | null> {
  if (!inTauri) return null;
  return await invoke<AutotestPlan | null>("autotest_plan").catch(() => null);
}

// Writes a line to the app log (Library/Logs/com.wumpa.app/wumpa.log on macOS).
export function logLine(line: string): void {
  if (!inTauri) return;
  void invoke("log_line", { line }).catch(() => {});
}

export async function exitApp(): Promise<void> {
  if (!inTauri) {
    window.close();
    return;
  }
  await invoke("exit_app");
}

export async function listenPayload<T>(name: string, cb: (payload: T) => void): Promise<() => void> {
  if (!inTauri) return () => {};
  const { listen } = await import("@tauri-apps/api/event");
  return await listen<T>(name, (e) => cb(e.payload));
}

export interface ScanProgress {
  page: number; // 1-based page being processed
  index: number; // 0-based position in the batch
  total: number;
  stage: string;
}

// Pages whose only content is one big picture: candidates for Image to PDF.
export async function imageOnlyPages(path: string): Promise<number[]> {
  if (!inTauri) return [];
  return await invoke<number[]>("image_only_pages", { path });
}

export async function rebuildScanned(
  srcPath: string,
  outPath: string,
  pages: number[],
  langs: string[],
): Promise<void> {
  if (!inTauri) throw new Error("Image to PDF requires the desktop app");
  await invoke("rebuild_scanned", { srcPath, outPath, pages, langs });
}

export async function listenEvent(name: string, cb: () => void): Promise<() => void> {
  if (!inTauri) return () => {};
  const { listen } = await import("@tauri-apps/api/event");
  return await listen(name, () => cb());
}

export async function mergeDocuments(paths: string[], outPath: string): Promise<void> {
  if (!inTauri) return;
  await invoke("merge_documents", { paths, outPath });
}

export async function splitDocument(
  srcPath: string,
  parts: { pages: number[]; out_path: string }[],
): Promise<void> {
  if (!inTauri) return;
  await invoke("split_document", { srcPath, parts });
}

export async function compressPdf(
  srcPath: string,
  outPath: string,
  quality: number,
  maxDim: number,
): Promise<[number, number]> {
  if (!inTauri) return [0, 0];
  return await invoke<[number, number]>("compress_pdf", { srcPath, outPath, quality, maxDim });
}

export async function createBlankPdf(w: number, h: number, pages: number): Promise<string | null> {
  if (!inTauri) return null;
  return await invoke<string>("create_blank_pdf", { w, h, pages });
}

export async function readImageB64(path: string): Promise<string | null> {
  if (!inTauri) return null;
  return await invoke<string>("read_image_b64", { path });
}

export async function discardPreview(path: string): Promise<void> {
  if (!inTauri) return;
  await invoke("discard_preview", { path }).catch(() => {});
}

export async function pickImagePath(): Promise<string | null> {
  if (!inTauri) return null;
  const { open } = await import("@tauri-apps/plugin-dialog");
  const res = await open({
    multiple: false,
    filters: [{ name: "Image", extensions: ["png", "jpg", "jpeg"] }],
  });
  return typeof res === "string" ? res : null;
}

export async function openExternalUrl(url: string): Promise<void> {
  if (!inTauri) {
    window.open(url, "_blank");
    return;
  }
  const { openUrl } = await import("@tauri-apps/plugin-opener");
  await openUrl(url);
}

export async function onFileDrop(cb: (paths: string[]) => void): Promise<() => void> {
  if (!inTauri) return () => {};
  const { getCurrentWebview } = await import("@tauri-apps/api/webview");
  return await getCurrentWebview().onDragDropEvent((event) => {
    if (event.payload.type === "drop") {
      const pdfs = event.payload.paths.filter((p) => p.toLowerCase().endsWith(".pdf"));
      if (pdfs.length) cb(pdfs);
    }
  });
}

export function baseName(path: string): string {
  return path.split(/[\\/]/).pop() ?? path;
}

export function dirName(path: string): string {
  const i = Math.max(path.lastIndexOf("/"), path.lastIndexOf("\\"));
  if (i > 0) return path.slice(0, i);
  return i === 0 ? path[0] : path;
}

// ---- password protection ----

export type PdfPermissions = SecurityInfo["permissions"];

export async function encryptPdf(
  srcPath: string,
  outPath: string,
  userPassword: string,
  ownerPassword: string,
  permissions: PdfPermissions,
  policy: SecurityPolicy | null = null,
): Promise<void> {
  if (!inTauri) throw new Error("Protecting files requires the desktop app");
  await invoke("encrypt_pdf", { srcPath, outPath, userPassword, ownerPassword, permissions, policy });
}

export async function passwordStatus(path: string): Promise<AttemptStatus | null> {
  if (!inTauri) return null;
  return await invoke<AttemptStatus>("password_status", { path }).catch(() => null);
}

export async function passwordAttempt(path: string, ok: boolean): Promise<AttemptStatus | null> {
  if (!inTauri) return null;
  return await invoke<AttemptStatus>("password_attempt", { path, ok }).catch(() => null);
}

export async function securityInfo(path: string): Promise<SecurityInfo | null> {
  if (!inTauri) return null;
  return await invoke<SecurityInfo>("security_info", { path });
}

export async function removeSecurity(srcPath: string, outPath: string, password: string): Promise<void> {
  if (!inTauri) throw new Error("Removing protection requires the desktop app");
  await invoke("remove_security", { srcPath, outPath, password });
}

// ---- Export to Word / Excel ----

export interface ExportWord {
  text: string;
  bbox: [number, number, number, number];
}

export interface ExportLine {
  text: string;
  bbox: [number, number, number, number];
  size: number;
  bold: boolean;
  italic: boolean;
  words: ExportWord[];
}

export interface NativePageInput {
  kind: "native";
  page: number;
  width: number;
  height: number;
  rasterPngB64: string;
  lines: ExportLine[];
}

export interface ScannedPageInput {
  kind: "scanned";
  page: number;
}

export type ExportPageInput = NativePageInput | ScannedPageInput;

// The analysed page the core hands back; passed on unchanged to the writer.
export type ExportPage = Record<string, unknown> & { number: number };

export interface ExportProgress {
  page: number;
  stage: string;
}

export type ExportFormat = "docx" | "xlsx";

export async function exportPage(
  srcPath: string,
  input: ExportPageInput,
  langs: string[],
): Promise<ExportPage> {
  if (!inTauri) throw new Error("Export requires the desktop app");
  return await invoke<ExportPage>("export_page", { srcPath, input, langs });
}

export async function exportWrite(
  pages: ExportPage[],
  format: ExportFormat,
  outPath: string,
  lang: string,
): Promise<void> {
  if (!inTauri) throw new Error("Export requires the desktop app");
  await invoke("export_write", { pages, format, outPath, lang });
}
