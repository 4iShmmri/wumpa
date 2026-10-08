import { useStore } from "./store";

export function report(context: string, e: unknown) {
  console.error(`[${context}]`, e);
  useStore
    .getState()
    .setStatus(`${context}: ${e instanceof Error ? e.message : String(e)}`);
}
