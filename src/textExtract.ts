export interface RawItem {
  str: string;
  transform: ArrayLike<number>;
  width?: number;
  height?: number;
  fontName?: string;
}

export interface ExtractedText {
  text: string;
  size: number;
  lineHeight: number;
  fontName: string;
  fallbackFamily: string;
}

const RTL = /[\u0590-\u08FF\uFB1D-\uFDFF\uFE70-\uFEFF]/;

interface It {
  str: string;
  tx: number;
  ty: number;
  w: number;
  h: number;
  fs: number;
  fontName: string;
}

function toIt(raw: RawItem): It | null {
  if (!raw.str) return null;
  const tr = raw.transform;
  const tx = tr[4];
  const ty = tr[5];
  const fs = Math.abs(tr[3]) || 0;
  const h = fs || raw.height || 0;
  return { str: raw.str, tx, ty, w: raw.width || 0, h, fs, fontName: raw.fontName ?? "" };
}

// Horizontal: any overlap (engine-side bboxes underestimate widths when font
// /Widths are missing). Vertical: >=35% of the item band, so a grazing touch
// with a neighboring line is not pulled in.
function intersects(it: It, b: [number, number, number, number], gx: number, gy: number): boolean {
  const x1 = it.tx;
  const x2 = it.tx + it.w;
  const y1 = it.ty - 0.2 * it.h;
  const y2 = it.ty + 0.9 * it.h;
  if (x2 < b[0] - gx || x1 > b[2] + gx) return false;
  const ov = Math.min(y2, b[3] + gy) - Math.max(y1, b[1] - gy);
  return ov >= 0.35 * (y2 - y1);
}

function median(vals: number[]): number {
  if (!vals.length) return 0;
  const s = [...vals].sort((a, b) => a - b);
  return s[Math.floor(s.length / 2)];
}

export function extractTextInRect(
  rawItems: RawItem[],
  styles: Record<string, { fontFamily?: string } | undefined>,
  bbox: [number, number, number, number],
): ExtractedText {
  const empty: ExtractedText = {
    text: "",
    size: 0,
    lineHeight: 1.35,
    fontName: "",
    fallbackFamily: "sans-serif",
  };
  const items: It[] = [];
  for (const r of rawItems) {
    const it = toIt(r);
    if (it) items.push(it);
  }
  if (!items.length) return empty;

  let provisional = items.filter((it) => intersects(it, bbox, 4, 2));
  if (!provisional.length) provisional = items.filter((it) => intersects(it, bbox, 12, 6));
  if (!provisional.length) return empty;
  const size0 = median(provisional.map((it) => it.fs).filter((v) => v > 0)) || (bbox[3] - bbox[1]) / 1.35;

  const seeds = new Set(
    items.filter((it) => intersects(it, bbox, Math.max(4, 0.4 * size0), Math.max(2, 0.25 * size0))),
  );
  if (!seeds.size) return empty;

  const allLines: It[][] = [];
  for (const k of [...items].sort((a, b) => b.ty - a.ty || a.tx - b.tx)) {
    const line = allLines.find((l) => Math.abs(l[0].ty - k.ty) < 0.45 * Math.max(k.h, l[0].h, 1));
    if (line) line.push(k);
    else allLines.push([k]);
  }

  // The engine's text block is whole lines, but its width estimate can fall
  // short (missing /Widths). From each seeded line take the seeds' neighbors
  // too, chaining while gaps stay word-sized — a column gutter breaks the chain.
  const maxGap = 1.6 * size0;
  const lines: It[][] = [];
  for (const line of allLines) {
    if (!line.some((it) => seeds.has(it))) continue;
    const sorted = [...line].sort((a, b) => a.tx - b.tx);
    const accepted = new Array<boolean>(sorted.length).fill(false);
    sorted.forEach((it, i) => {
      if (seeds.has(it)) accepted[i] = true;
    });
    for (let i = 1; i < sorted.length; i++) {
      if (!accepted[i] && accepted[i - 1]) {
        const gap = sorted[i].tx - (sorted[i - 1].tx + sorted[i - 1].w);
        if (gap <= maxGap) accepted[i] = true;
      }
    }
    for (let i = sorted.length - 2; i >= 0; i--) {
      if (!accepted[i] && accepted[i + 1]) {
        const gap = sorted[i + 1].tx - (sorted[i].tx + sorted[i].w);
        if (gap <= maxGap) accepted[i] = true;
      }
    }
    lines.push(sorted.filter((_, i) => accepted[i]));
  }
  const kept = lines.flat();
  if (!kept.length) return empty;

  const fss = kept.map((k) => k.fs).filter((v) => v > 0);
  const fallbackSize = (bbox[3] - bbox[1]) / Math.max(1, lines.length) / 1.35;
  const size = Math.max(6, Math.min(120, median(fss) || fallbackSize));

  const gaps: number[] = [];
  for (let i = 0; i + 1 < lines.length; i++) {
    const d = lines[i][0].ty - lines[i + 1][0].ty;
    if (d > 0 && size > 0) gaps.push(d / size);
  }
  const lineHeight = gaps.length ? Math.max(1.0, Math.min(2.2, median(gaps))) : 1.35;

  const counts = new Map<string, number>();
  for (const k of kept) {
    if (k.fontName) counts.set(k.fontName, (counts.get(k.fontName) ?? 0) + 1);
  }
  let fontName = "";
  let best = 0;
  for (const [name, n] of counts) {
    if (n > best) {
      best = n;
      fontName = name;
    }
  }
  const fallbackFamily = styles[fontName]?.fontFamily ?? "sans-serif";

  const text = lines
    .map((l) => {
      const rtl = RTL.test(l.map((k) => k.str).join(""));
      l.sort((a, b) => (rtl ? b.tx - a.tx : a.tx - b.tx));
      let out = "";
      let prev: It | null = null;
      for (const k of l) {
        if (prev && out && !out.endsWith(" ")) {
          const gap = rtl ? prev.tx - (k.tx + k.w) : k.tx - (prev.tx + prev.w);
          if (gap > 0.3 * size) out += " ";
        }
        out += k.str;
        prev = k;
      }
      return out;
    })
    .join("\n");

  return { text, size, lineHeight, fontName, fallbackFamily };
}
