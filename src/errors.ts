import { useStore } from "./store";
import { logLine } from "./backend";

// Errors raised for flow control (a superseded load, a file the app itself
// destroyed) already produced their own status line.
const SILENT = new Set(["LoadCancelled", "FileWiped"]);

export function report(context: string, e: unknown) {
  if (e instanceof Error && SILENT.has(e.name)) return;
  console.error(`[${context}]`, e);
  const detail = e instanceof Error ? e.message : e == null ? "" : String(e);
  logLine(`[${context}] ${detail}${e instanceof Error && e.stack ? "\n" + e.stack : ""}`);
  const same = detail.trim().toLowerCase() === context.trim().toLowerCase();
  useStore.getState().setStatus(detail && !same ? `${context}: ${detail}` : context);
}

export function flowError(name: "LoadCancelled" | "FileWiped", message = name): Error {
  const err = new Error(message);
  err.name = name;
  return err;
}
