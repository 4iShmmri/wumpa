// Bridge to the Tauri backend, with browser fallbacks so the UI can be
// developed and tested in a plain browser (npm run dev).
import { invoke } from "@tauri-apps/api/core";
import type { PageSpec, Highlight, ObjDesc, ObjectEdit } from "./types";

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

export async function saveDocument(
  srcPath: string,
  outPath: string,
  pages: PageSpec[],
  highlights: Highlight[],
  notes: NotePayload[],
  edits: ObjectEdit[],
): Promise<void> {
  if (!inTauri) {
    console.log("saveDocument", { srcPath, outPath, pages, highlights, notes, edits });
    return;
  }
  await invoke("save_document", { srcPath, outPath, pages, highlights, notes, edits });
}

export async function listPageObjects(srcPath: string, page: number): Promise<ObjDesc[]> {
  if (!inTauri) return [];
  return await invoke<ObjDesc[]>("list_page_objects", { srcPath, page });
}

export async function applyEditsPreview(
  srcPath: string,
  edits: ObjectEdit[],
): Promise<string | null> {
  if (!inTauri) return null;
  return await invoke<string>("apply_edits_preview", { srcPath, edits });
}

export async function takePendingFiles(): Promise<string[]> {
  if (!inTauri) return [];
  return await invoke<string[]>("take_pending_files");
}

export async function exitApp(): Promise<void> {
  if (!inTauri) {
    window.close();
    return;
  }
  await invoke("exit_app");
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
  return i > 0 ? path.slice(0, i) : path;
}
