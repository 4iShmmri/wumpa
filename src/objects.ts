import { listPageObjects } from "./backend";
import type { ObjDesc, ObjectEdit } from "./types";

const cache = new Map<string, Promise<ObjDesc[]>>();

export function getObjects(tabId: string, path: string, src: number): Promise<ObjDesc[]> {
  const key = `${tabId}:${src}`;
  let p = cache.get(key);
  if (!p) {
    p = listPageObjects(path, src);
    cache.set(key, p);
    p.catch(() => cache.delete(key));
  }
  return p;
}

export function evictObjects(tabId: string) {
  for (const k of [...cache.keys()]) {
    if (k.startsWith(`${tabId}:`)) cache.delete(k);
  }
}

export function adjustedBox(
  o: ObjDesc,
  edits: ObjectEdit[],
  src: number,
): [number, number, number, number] | null {
  let [x1, y1, x2, y2] = o.bbox;
  for (const e of edits) {
    if (e.src !== src || e.id !== o.id) continue;
    if (e.action === "delete") return null;
    if (e.action === "translate") {
      x1 += e.dx ?? 0;
      x2 += e.dx ?? 0;
      y1 += e.dy ?? 0;
      y2 += e.dy ?? 0;
    } else if (e.action === "scale") {
      const ax = e.ax ?? 0;
      const ay = e.ay ?? 0;
      const sx = e.sx ?? 1;
      const sy = e.sy ?? 1;
      x1 = ax + (x1 - ax) * sx;
      x2 = ax + (x2 - ax) * sx;
      y1 = ay + (y1 - ay) * sy;
      y2 = ay + (y2 - ay) * sy;
    }
  }
  return [Math.min(x1, x2), Math.min(y1, y2), Math.max(x1, x2), Math.max(y1, y2)];
}
