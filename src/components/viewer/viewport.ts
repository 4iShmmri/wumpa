import type { PageViewport } from "pdfjs-dist";
import { getPage, pageRotation, specBase, type DocEntry } from "../../pdf";
import type { PageSpec } from "../../types";

// pdfjs-dist exports PageViewport only as a type, so inserted blank pages
// (spec.src <= 0) get a hand-built viewport with the same contract. The
// transform math mirrors pdf.js's PageViewport so overlay layers can treat
// both kinds of page alike.
export class BlankViewport {
  viewBox: number[];
  userUnit = 1;
  scale: number;
  rotation: number;
  offsetX: number;
  offsetY: number;
  transform: number[];
  width: number;
  height: number;

  constructor({
    viewBox,
    scale,
    rotation,
    offsetX = 0,
    offsetY = 0,
    dontFlip = false,
  }: {
    viewBox: number[];
    scale: number;
    rotation: number;
    offsetX?: number;
    offsetY?: number;
    dontFlip?: boolean;
  }) {
    this.viewBox = viewBox;
    this.scale = scale;
    this.rotation = rotation;
    this.offsetX = offsetX;
    this.offsetY = offsetY;
    const centerX = (viewBox[2] + viewBox[0]) / 2;
    const centerY = (viewBox[3] + viewBox[1]) / 2;
    let a: number;
    let b: number;
    let c: number;
    let d: number;
    switch (pageRotation(rotation, 0)) {
      case 180:
        a = -1;
        b = 0;
        c = 0;
        d = 1;
        break;
      case 90:
        a = 0;
        b = 1;
        c = 1;
        d = 0;
        break;
      case 270:
        a = 0;
        b = -1;
        c = -1;
        d = 0;
        break;
      default:
        a = 1;
        b = 0;
        c = 0;
        d = -1;
        break;
    }
    if (dontFlip) {
      c = -c;
      d = -d;
    }
    let ox: number;
    let oy: number;
    if (a === 0) {
      ox = Math.abs(centerY - viewBox[1]) * scale + offsetX;
      oy = Math.abs(centerX - viewBox[0]) * scale + offsetY;
      this.width = (viewBox[3] - viewBox[1]) * scale;
      this.height = (viewBox[2] - viewBox[0]) * scale;
    } else {
      ox = Math.abs(centerX - viewBox[0]) * scale + offsetX;
      oy = Math.abs(centerY - viewBox[1]) * scale + offsetY;
      this.width = (viewBox[2] - viewBox[0]) * scale;
      this.height = (viewBox[3] - viewBox[1]) * scale;
    }
    this.transform = [
      a * scale,
      b * scale,
      c * scale,
      d * scale,
      ox - a * scale * centerX - c * scale * centerY,
      oy - b * scale * centerX - d * scale * centerY,
    ];
  }

  get rawDims(): object {
    const v = this.viewBox;
    return { pageWidth: v[2] - v[0], pageHeight: v[3] - v[1], pageX: v[0], pageY: v[1] };
  }

  clone({
    scale = this.scale,
    rotation = this.rotation,
    offsetX = this.offsetX,
    offsetY = this.offsetY,
    dontFlip = false,
  }: {
    scale?: number;
    rotation?: number;
    offsetX?: number;
    offsetY?: number;
    dontFlip?: boolean;
  } = {}): BlankViewport {
    return new BlankViewport({ viewBox: this.viewBox.slice(), scale, rotation, offsetX, offsetY, dontFlip });
  }

  convertToViewportPoint(x: number, y: number): number[] {
    const m = this.transform;
    return [m[0] * x + m[2] * y + m[4], m[1] * x + m[3] * y + m[5]];
  }

  convertToPdfPoint(x: number, y: number): number[] {
    const m = this.transform;
    const det = m[0] * m[3] - m[1] * m[2];
    return [
      (x * m[3] - y * m[2] + m[2] * m[5] - m[4] * m[3]) / det,
      (-x * m[1] + y * m[0] + m[4] * m[1] - m[5] * m[0]) / det,
    ];
  }
}

export function blankViewport(spec: PageSpec, zoom: number): PageViewport {
  const base = specBase(null, spec)!;
  return new BlankViewport({
    viewBox: [0, 0, base.w, base.h],
    scale: zoom,
    rotation: pageRotation(base.rotate, spec.rotation),
  });
}

// The viewport a page is laid out with: pdf.js's own for source pages, a
// BlankViewport for inserted blank pages.
export async function specViewport(entry: DocEntry, spec: PageSpec, zoom: number): Promise<PageViewport> {
  if (spec.src <= 0) return blankViewport(spec, zoom);
  const page = await getPage(entry, spec.src);
  return page.getViewport({ scale: zoom, rotation: pageRotation(page.rotate, spec.rotation) });
}
