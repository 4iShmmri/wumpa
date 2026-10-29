import { useStore } from "./store";
import type { StampItem } from "./types";
import type { WireStamp } from "./backend";
import { DEFAULT_TEXT_COLOR, clampTextSize } from "./components/ui/textPalette";

const S = useStore.getState;
const SCALE = 4;

export const STAMP_FONT = '-apple-system, "Segoe UI", "Helvetica Neue", sans-serif';
export const STAMP_PAD_X = 4;
export const STAMP_PAD_Y = 3;
export const STAMP_LINE = 1.35;

let mctx: CanvasRenderingContext2D | null = null;
function measureCtx() {
  if (!mctx) mctx = document.createElement("canvas").getContext("2d")!;
  return mctx;
}

export function measureText(text: string, size: number, fontFamily?: string, lineHeight?: number) {
  const ctx = measureCtx();
  ctx.font = `${size}px ${fontFamily ?? STAMP_FONT}`;
  const lines = text.split("\n");
  const natural = Math.max(0, ...lines.map((l) => ctx.measureText(l).width));
  return {
    w: Math.max(natural + STAMP_PAD_X * 2, 24),
    h: lines.length * size * (lineHeight ?? STAMP_LINE) + STAMP_PAD_Y * 2,
  };
}

export function addTextStamp(tabId: string, src: number, x: number, y: number): StampItem {
  const size = 16;
  const { h } = measureText("", size);
  const stamp: StampItem = {
    id: crypto.randomUUID(),
    src,
    rect: [x, y - h, x + 80, y],
    kind: "text",
    text: "",
    size,
    color: DEFAULT_TEXT_COLOR,
  };
  S().mutateDoc(tabId, (t) => ({ stamps: [...t.stamps, stamp] }));
  return stamp;
}

export function updateTextStamp(
  tabId: string,
  id: string,
  patch: { text?: string; size?: number; color?: string },
) {
  S().mutateDoc(tabId, (t) => ({
    stamps: t.stamps.map((s) => {
      if (s.id !== id || s.kind !== "text") return s;
      const next = { ...s, ...patch };
      const { w, h } = measureText(next.text ?? "", next.size ?? 16, next.fontFamily, next.lineHeight);
      const [x1, , , y2] = s.rect;
      return { ...next, rect: [x1, y2 - h, x1 + Math.max(w, 80), y2] as StampItem["rect"] };
    }),
  }));
}

export function addSignatureStamp(
  tabId: string,
  src: number,
  x: number,
  y: number,
  dataUrl: string,
  aspect: number,
) {
  const w = 160;
  const h = w / Math.max(0.1, aspect);
  const stamp: StampItem = {
    id: crypto.randomUUID(),
    src,
    rect: [x, y - h, x + w, y],
    kind: "signature",
    sigDataUrl: dataUrl,
  };
  S().mutateDoc(tabId, (t) => ({ stamps: [...t.stamps, stamp] }));
  return stamp;
}

export function addTextStampAt(
  tabId: string,
  src: number,
  rect: [number, number, number, number],
  text: string,
  size: number,
) {
  const stamp: StampItem = {
    id: crypto.randomUUID(),
    src,
    rect,
    kind: "text",
    text,
    size,
    color: DEFAULT_TEXT_COLOR,
  };
  S().mutateDoc(tabId, (t) => ({ stamps: [...t.stamps, stamp] }));
  return stamp;
}

export function addImageStamp(
  tabId: string,
  src: number,
  rect: [number, number, number, number],
  dataUrl: string,
) {
  const stamp: StampItem = {
    id: crypto.randomUUID(),
    src,
    rect,
    kind: "image",
    sigDataUrl: dataUrl,
  };
  S().mutateDoc(tabId, (t) => ({ stamps: [...t.stamps, stamp] }));
  return stamp;
}

export function moveStamp(tabId: string, id: string, dx: number, dy: number) {
  S().mutateDoc(tabId, (t) => ({
    stamps: t.stamps.map((s) =>
      s.id === id
        ? {
            ...s,
            rect: [s.rect[0] + dx, s.rect[1] + dy, s.rect[2] + dx, s.rect[3] + dy] as StampItem["rect"],
          }
        : s,
    ),
  }));
}

export function resizeStamp(tabId: string, id: string, factor: number) {
  const tab = S().tabs.find((t) => t.id === tabId);
  const s = tab?.stamps.find((x) => x.id === id);
  if (!s) return;
  if (s.kind === "text") {
    const size = clampTextSize(Math.round((s.size ?? 16) * factor));
    updateTextStamp(tabId, id, { size });
    return;
  }
  const w = Math.max(20, Math.min(2000, (s.rect[2] - s.rect[0]) * factor));
  const h = (w / (s.rect[2] - s.rect[0])) * (s.rect[3] - s.rect[1]);
  S().mutateDoc(tabId, (t) => ({
    stamps: t.stamps.map((x) =>
      x.id === id
        ? { ...x, rect: [x.rect[0], x.rect[3] - h, x.rect[0] + w, x.rect[3]] as StampItem["rect"] }
        : x,
    ),
  }));
}

export function deleteStamp(tabId: string, id: string) {
  S().mutateDoc(tabId, (t) => ({ stamps: t.stamps.filter((s) => s.id !== id) }));
  const st = S();
  if (st.stampSel?.id === id) st.setStampSel(null);
  if (st.stampEditing === id) st.setStampEditing(null);
}

// Decoded images keyed by a hash of their URL rather than the URL itself: a
// data URL is the whole image again, and the map must not pin every one ever
// placed. Insertion order doubles as LRU order; a hit is re-inserted.
const IMG_CACHE_MAX = 24;
const imgCache = new Map<string, HTMLImageElement>();

// cyrb53: cheap 53-bit string hash, good enough to tell data URLs apart. The
// length is folded into the key and the hit is checked against the URL anyway.
function hashUrl(url: string): string {
  let h1 = 0xdeadbeef;
  let h2 = 0x41c6ce57;
  for (let i = 0; i < url.length; i++) {
    const ch = url.charCodeAt(i);
    h1 = Math.imul(h1 ^ ch, 2654435761);
    h2 = Math.imul(h2 ^ ch, 1597334677);
  }
  h1 = Math.imul(h1 ^ (h1 >>> 16), 2246822507) ^ Math.imul(h2 ^ (h2 >>> 13), 3266489909);
  h2 = Math.imul(h2 ^ (h2 >>> 16), 2246822507) ^ Math.imul(h1 ^ (h1 >>> 13), 3266489909);
  return `${url.length}:${(h2 >>> 0).toString(36)}${(h1 >>> 0).toString(36)}`;
}

export function loadImage(url: string): Promise<HTMLImageElement> {
  const key = hashUrl(url);
  const hit = imgCache.get(key);
  // img.src holds the URL already, so the check costs no extra memory and
  // turns a hash collision into a plain miss.
  if (hit?.complete && hit.src === url) {
    imgCache.delete(key);
    imgCache.set(key, hit);
    return Promise.resolve(hit);
  }
  return new Promise((resolve, reject) => {
    const img = new Image();
    img.onload = () => {
      imgCache.delete(key);
      imgCache.set(key, img);
      while (imgCache.size > IMG_CACHE_MAX) {
        const oldest = imgCache.keys().next().value;
        if (oldest === undefined) break;
        imgCache.delete(oldest);
      }
      resolve(img);
    };
    img.onerror = reject;
    img.src = url;
  });
}

function toB64(bytes: Uint8ClampedArray): string {
  let bin = "";
  const CH = 32768;
  for (let i = 0; i < bytes.length; i += CH) {
    bin += String.fromCharCode(...bytes.subarray(i, Math.min(i + CH, bytes.length)));
  }
  return btoa(bin);
}

export async function rasterizeStamps(stamps: StampItem[]): Promise<WireStamp[]> {
  try {
    await document.fonts.ready;
  } catch {
    /* render with whatever is loaded */
  }
  const out: WireStamp[] = [];
  for (const s of stamps) {
    const rw = Math.abs(s.rect[2] - s.rect[0]);
    const rh = Math.abs(s.rect[3] - s.rect[1]);
    if (rw < 0.5 || rh < 0.5) continue;
    const w = Math.max(2, Math.round(rw * SCALE));
    const h = Math.max(2, Math.round(rh * SCALE));
    const canvas = document.createElement("canvas");
    canvas.width = w;
    canvas.height = h;
    const ctx = canvas.getContext("2d")!;
    if (s.kind === "text") {
      const size = (s.size ?? 16) * SCALE;
      const lh = s.lineHeight ?? STAMP_LINE;
      ctx.font = `${size}px ${s.fontFamily ?? STAMP_FONT}`;
      ctx.fillStyle = s.color ?? DEFAULT_TEXT_COLOR;
      ctx.textBaseline = "top";
      (s.text ?? "").split("\n").forEach((line, i) => {
        ctx.fillText(line, STAMP_PAD_X * SCALE, STAMP_PAD_Y * SCALE + i * size * lh);
      });
    } else if (s.sigDataUrl) {
      const img = await loadImage(s.sigDataUrl);
      ctx.drawImage(img, 0, 0, w, h);
    } else {
      continue;
    }
    const data = ctx.getImageData(0, 0, w, h).data;
    out.push({ src: s.src, rect: s.rect, w, h, rgbaB64: toB64(data) });
  }
  return out;
}
