import { useEffect, useState } from "react";
import { getPage, readTextContent, type DocEntry } from "../../pdf";
import { tokensFromItems, kindGlyph, type ChangeKind, type PageTokens } from "../../compare";

export const ICON = {
  swap: "M3 8h17M15 3l5 5-5 5 M21 16H4M9 11l-5 5 5 5",
  caret: "M6 9l6 6 6-6",
  up: "M18 15l-6-6-6 6",
  down: "M6 9l6 6 6-6",
  close: "M18 6L6 18M6 6l12 12",
  file: "M14 2H6a2 2 0 0 0-2 2v16a2 2 0 0 0 2 2h12a2 2 0 0 0 2-2V8z M14 2v6h6",
  check: "M20 6L9 17l-5-5",
  sideBySide: "M3 5a1 1 0 0 1 1-1h6v16H4a1 1 0 0 1-1-1z M14 4h6a1 1 0 0 1 1 1v14a1 1 0 0 1-1 1h-6z",
  filter: "M3 5h18l-7 8v6l-4-2v-4z",
  more: "M5 12h.01M12 12h.01M19 12h.01",
  search: "M11 19a8 8 0 1 0 0-16 8 8 0 0 0 0 16z M21 21l-4.3-4.3",
};

// Acrobat's change badge: a filled circle with a "T" and a small glyph for the kind.
export function KindIcon({ kind, size = 22 }: { kind: ChangeKind; size?: number }) {
  return (
    <span
      className={"cmp-kind-icon " + kind}
      style={{ width: size, height: size, fontSize: size * 0.56 }}
      aria-hidden="true"
    >
      <b>T</b>
      <i>{kindGlyph(kind)}</i>
    </span>
  );
}

// The tokens and boxes of one page of a loaded document.
export async function pageTokens(entry: DocEntry, src: number): Promise<PageTokens> {
  const page = await getPage(entry, src);
  const content = await readTextContent(page);
  return tokensFromItems(content.items);
}

// Renders a page into the canvas; `hold` receives the task so a caller can cancel it.
export async function renderToCanvas(
  entry: DocEntry,
  src: number,
  scale: number,
  canvas: HTMLCanvasElement,
  hold: (task: { cancel: () => void }) => void,
) {
  const page = await getPage(entry, src);
  const vp = page.getViewport({ scale });
  const dpr = Math.min(window.devicePixelRatio || 1, 2);
  canvas.width = Math.floor(vp.width * dpr);
  canvas.height = Math.floor(vp.height * dpr);
  const task = page.render({
    canvas,
    viewport: vp,
    transform: dpr !== 1 ? [dpr, 0, 0, dpr, 0, 0] : undefined,
  });
  hold(task);
  await task.promise;
  return vp;
}

// True once the element has scrolled near the viewport (and stays true).
export function useVisible(ref: React.RefObject<HTMLElement | null>) {
  const [visible, setVisible] = useState(false);
  useEffect(() => {
    const el = ref.current!;
    const io = new IntersectionObserver(([e]) => setVisible((v) => v || e.isIntersecting), {
      rootMargin: "800px 0px",
    });
    io.observe(el);
    return () => io.disconnect();
  }, []);
  return visible;
}
