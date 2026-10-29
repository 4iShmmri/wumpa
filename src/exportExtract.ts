// Describes a text page for export: its lines with exact boxes and font
// facts from pdf.js, plus a raster of the page for rules, tables and
// pictures. Coordinates are PDF points with the origin bottom-left.
import type { PDFPageProxy } from "pdfjs-dist";
import type { TextItem } from "pdfjs-dist/types/src/display/api";
import { getPage, readTextContent, type DocEntry } from "./pdf";
import type { ExportLine, ExportWord, NativePageInput } from "./backend";

// Pixels per point of the page raster.
const RASTER_SCALE = 3;
// Items on one baseline this far apart (in font sizes) start a new line.
const LINE_GAP = 2;
// Baselines closer than this share of the font size are one line.
const BASELINE_TOL = 0.4;

interface Item {
  str: string;
  x: number;
  y: number;
  w: number;
  size: number;
  ascent: number;
  descent: number;
  bold: boolean;
  italic: boolean;
}

interface FontFacts {
  bold: boolean;
  italic: boolean;
}

function fontFacts(
  page: PDFPageProxy,
  fontName: string,
  family: string,
  cache: Map<string, FontFacts>,
): FontFacts {
  const hit = cache.get(fontName);
  if (hit) return hit;
  let name = family;
  let flags: { bold?: boolean; italic?: boolean; black?: boolean } = {};
  try {
    if (page.commonObjs.has(fontName)) {
      const f = page.commonObjs.get(fontName) as {
        name?: string;
        bold?: boolean;
        italic?: boolean;
        black?: boolean;
      };
      name = f?.name ?? name;
      flags = f ?? {};
    }
  } catch {
    // Font facts are a nicety; the text still exports.
  }
  const facts = {
    bold: !!flags.bold || !!flags.black || /bold|black|heavy|semibold|demibold/i.test(name),
    italic: !!flags.italic || /italic|oblique/i.test(name),
  };
  cache.set(fontName, facts);
  return facts;
}

function toItems(
  page: PDFPageProxy,
  items: TextItem[],
  styles: Record<string, { ascent?: number; descent?: number; fontFamily?: string }>,
  x0: number,
  y0: number,
): Item[] {
  const cache = new Map<string, FontFacts>();
  const out: Item[] = [];
  for (const it of items) {
    if (!it.str || !it.str.trim()) continue;
    const [a, b, c, d, e, f] = it.transform;
    // Vertical or heavily rotated text is left out of the flow.
    if (Math.abs(b) > Math.abs(a)) continue;
    const size = Math.hypot(c, d) || it.height || 0;
    if (size <= 0) continue;
    const st = styles[it.fontName] ?? {};
    const ascent = Math.min(1.0, Math.max(0.6, st.ascent ?? 0.8));
    const descent = Math.max(-0.35, Math.min(-0.05, st.descent ?? -0.2));
    const facts = fontFacts(page, it.fontName, st.fontFamily ?? "", cache);
    out.push({ str: it.str, x: e - x0, y: f - y0, w: it.width, size, ascent, descent, ...facts });
  }
  return out;
}

// Words of one item, with boxes estimated from the share of its characters.
function words(it: Item): ExportWord[] {
  const chars = it.str.length || 1;
  const cw = it.w / chars;
  const out: ExportWord[] = [];
  const re = /\S+/g;
  let m: RegExpExecArray | null;
  while ((m = re.exec(it.str))) {
    const x = it.x + m.index * cw;
    out.push({
      text: m[0],
      bbox: [x, it.y + it.descent * it.size, x + m[0].length * cw, it.y + it.ascent * it.size],
    });
  }
  return out;
}

const RTL_CHAR = /[\u0590-\u08FF\uFB1D-\uFDFF\uFE70-\uFEFF]/g;
const LTR_CHAR = /[A-Za-z\u00C0-\u024F]/g;

function isRtl(text: string): boolean {
  const rtl = (text.match(RTL_CHAR) ?? []).length;
  return rtl > 0 && rtl >= (text.match(LTR_CHAR) ?? []).length;
}

// One line from items sharing a baseline. Right-to-left lines list their
// items from the right so the text reads in logical order.
function lineOf(run: Item[]): ExportLine | null {
  const rtl = isRtl(run.map((i) => i.str).join(" "));
  const ordered = [...run].sort((p, q) => (rtl ? q.x - p.x : p.x - q.x));
  const ws = ordered.flatMap(words);
  if (!ws.length) return null;
  const x1 = Math.min(...run.map((i) => i.x));
  const x2 = Math.max(...run.map((i) => i.x + i.w));
  const base = run.reduce((s, i) => s + i.y, 0) / run.length;
  const size = Math.max(...run.map((i) => i.size));
  const asc = Math.max(...run.map((i) => i.ascent * i.size));
  const desc = Math.min(...run.map((i) => i.descent * i.size));
  const weight = (pick: (i: Item) => boolean) => run.filter(pick).reduce((s, i) => s + i.w, 0) * 2 > x2 - x1;
  return {
    text: ws.map((w) => w.text).join(" "),
    bbox: [x1, base + desc, x2, base + asc],
    size,
    bold: weight((i) => i.bold),
    italic: weight((i) => i.italic),
    words: ws,
  };
}

// Items on one baseline form a line; a wide gap (another column, another
// table cell) starts a new one.
function lines(items: Item[]): ExportLine[] {
  const sorted = [...items].sort((p, q) => q.y - p.y || p.x - q.x);
  const bands: Item[][] = [];
  for (const it of sorted) {
    const band = bands.find((b) => Math.abs(b[0].y - it.y) <= BASELINE_TOL * Math.min(b[0].size, it.size));
    if (band) band.push(it);
    else bands.push([it]);
  }
  const out: ExportLine[] = [];
  for (const band of bands) {
    band.sort((p, q) => p.x - q.x);
    let run: Item[] = [];
    const flush = () => {
      const line = run.length ? lineOf(run) : null;
      if (line) out.push(line);
      run = [];
    };
    for (const it of band) {
      const last = run[run.length - 1];
      if (last && it.x - (last.x + last.w) > LINE_GAP * Math.min(last.size, it.size)) flush();
      run.push(it);
    }
    flush();
  }
  return out;
}

async function rasterize(page: PDFPageProxy): Promise<string> {
  const vp = page.getViewport({ scale: RASTER_SCALE, rotation: 0 });
  const canvas = document.createElement("canvas");
  canvas.width = Math.ceil(vp.width);
  canvas.height = Math.ceil(vp.height);
  const ctx = canvas.getContext("2d");
  if (!ctx) throw new Error("no canvas context");
  ctx.fillStyle = "#fff";
  ctx.fillRect(0, 0, canvas.width, canvas.height);
  await page.render({ canvas, viewport: vp }).promise;
  const url = canvas.toDataURL("image/png");
  return url.slice(url.indexOf(",") + 1);
}

export async function extractNativePage(entry: DocEntry, src: number): Promise<NativePageInput> {
  const page = await getPage(entry, src);
  const [x0, y0, x1, y1] = page.view;
  // Loads the fonts, whose names tell bold from regular.
  await page.getOperatorList();
  const content = await readTextContent(page);
  const items = content.items.filter((it): it is TextItem => "str" in it);
  const styles = (content.styles ?? {}) as Record<
    string,
    { ascent?: number; descent?: number; fontFamily?: string }
  >;
  return {
    kind: "native",
    page: src,
    width: x1 - x0,
    height: y1 - y0,
    rasterPngB64: await rasterize(page),
    lines: lines(toItems(page, items, styles, x0, y0)),
  };
}

export const testing = { lines, words, isRtl };
