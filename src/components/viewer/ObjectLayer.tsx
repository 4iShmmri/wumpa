import { useEffect, useRef, useState } from "react";
import type { PageViewport } from "pdfjs-dist";
import { useStore } from "../../store";
import type { DocEntry } from "../../pdf";
import type { PageSpec, ObjDesc, ObjectEdit } from "../../types";
import { addObjectEdits, deleteObject } from "../../actions";
import TextEditPopover from "../TextEditPopover";
import { getObjects, adjustedBox } from "../../objects";
import { confirmDialog } from "../../backend";
import { t } from "../../i18n";
import { trackPointer } from "../../pointer";
import { report } from "../../errors";

const objErrOnce = new Set<string>();

interface CssBox {
  l: number;
  t: number;
  w: number;
  h: number;
}

interface ObjBox extends CssBox {
  o: ObjDesc;
  pdf: [number, number, number, number];
}

// Move / scale / erase handles over the page's content objects (edit and
// erase tools). Source pages only: blank pages have no objects.
export default function ObjectLayer({
  tabId,
  tabPath,
  spec,
  viewport,
  edits,
  tool,
  entry,
}: {
  tabId: string;
  tabPath: string;
  spec: PageSpec;
  viewport: PageViewport;
  edits: ObjectEdit[];
  tool: "edit" | "erase";
  entry: DocEntry;
}) {
  const [objs, setObjs] = useState<ObjDesc[] | null>(null);
  const objSel = useStore((s) => s.objSel);
  const layerRef = useRef<HTMLDivElement>(null);
  const [ghost, setGhost] = useState<CssBox | null>(null);
  const [eraseRect, setEraseRect] = useState<CssBox | null>(null);
  const [textEdit, setTextEdit] = useState<{ id: number; pdf: [number, number, number, number] } | null>(
    null,
  );

  useEffect(() => {
    let on = true;
    getObjects(tabId, tabPath, spec.src)
      .then((o) => on && setObjs(o))
      .catch((e) => {
        const k = `${tabId}:${spec.src}`;
        if (!objErrOnce.has(k)) {
          objErrOnce.add(k);
          report(t("stObjEditUnavailable"), e);
        }
      });
    return () => {
      on = false;
    };
  }, [tabId, tabPath, spec.src]);

  if (!objs) return null;

  const boxes: ObjBox[] = objs
    .flatMap((o) => {
      const b = adjustedBox(o, edits, spec.src);
      if (!b) return [];
      const [ax, ay] = viewport.convertToViewportPoint(b[0], b[1]) as [number, number];
      const [bx, by] = viewport.convertToViewportPoint(b[2], b[3]) as [number, number];
      return [
        {
          o,
          pdf: b,
          l: Math.min(ax, bx),
          t: Math.min(ay, by),
          w: Math.abs(bx - ax),
          h: Math.abs(by - ay),
        },
      ];
    })
    .sort((a, b) => b.w * b.h - a.w * a.h);

  const toPdf = (clientX: number, clientY: number) => {
    const r = layerRef.current!.getBoundingClientRect();
    return viewport.convertToPdfPoint(clientX - r.left, clientY - r.top) as [number, number];
  };

  const startMove = (e: React.PointerEvent, box: ObjBox) => {
    if (tool !== "edit" || e.button !== 0) return;
    e.stopPropagation();
    setTextEdit((te) => (te && te.id !== box.o.id ? null : te));
    useStore.getState().setObjSel({ src: spec.src, id: box.o.id });
    const sx = e.clientX;
    const sy = e.clientY;
    let moved = false;
    trackPointer(
      (ev) => {
        const dx = ev.clientX - sx;
        const dy = ev.clientY - sy;
        if (!moved && Math.hypot(dx, dy) > 2) moved = true;
        if (moved) setGhost({ l: box.l + dx, t: box.t + dy, w: box.w, h: box.h });
      },
      (ev) => {
        setGhost(null);
        if (!moved) return;
        const [px0, py0] = toPdf(sx, sy);
        const [px1, py1] = toPdf(ev.clientX, ev.clientY);
        addObjectEdits(tabId, [
          { src: spec.src, id: box.o.id, action: "translate", dx: px1 - px0, dy: py1 - py0 },
        ]);
      },
    );
  };

  const startScale = (e: React.PointerEvent, box: ObjBox, corner: number) => {
    if (e.button !== 0) return;
    e.stopPropagation();
    const cssCorners: [number, number][] = [
      [box.l, box.t],
      [box.l + box.w, box.t],
      [box.l, box.t + box.h],
      [box.l + box.w, box.t + box.h],
    ];
    const opp = cssCorners[3 - corner];
    const orig = cssCorners[corner];
    const r = layerRef.current!.getBoundingClientRect();
    const anchorPdf = viewport.convertToPdfPoint(opp[0], opp[1]) as [number, number];
    const origPdf = viewport.convertToPdfPoint(orig[0], orig[1]) as [number, number];
    trackPointer(
      (ev) => {
        const cl = ev.clientX - r.left;
        const ct = ev.clientY - r.top;
        setGhost({
          l: Math.min(cl, opp[0]),
          t: Math.min(ct, opp[1]),
          w: Math.abs(cl - opp[0]),
          h: Math.abs(ct - opp[1]),
        });
      },
      (ev) => {
        setGhost(null);
        const [nx, ny] = toPdf(ev.clientX, ev.clientY);
        const clampS = (v: number) => Math.max(0.05, Math.min(20, v));
        const sx = clampS((nx - anchorPdf[0]) / (origPdf[0] - anchorPdf[0] || 1e-6));
        const sy = clampS((ny - anchorPdf[1]) / (origPdf[1] - anchorPdf[1] || 1e-6));
        addObjectEdits(tabId, [
          { src: spec.src, id: box.o.id, action: "scale", sx, sy, ax: anchorPdf[0], ay: anchorPdf[1] },
        ]);
      },
    );
  };

  const startErase = (e: React.PointerEvent) => {
    if (tool !== "erase" || e.button !== 0) return;
    const r = layerRef.current!.getBoundingClientRect();
    const sx = e.clientX;
    const sy = e.clientY;
    trackPointer(
      (ev) => {
        setEraseRect({
          l: Math.min(ev.clientX, sx) - r.left,
          t: Math.min(ev.clientY, sy) - r.top,
          w: Math.abs(ev.clientX - sx),
          h: Math.abs(ev.clientY - sy),
        });
      },
      async (ev) => {
        setEraseRect(null);
        if (Math.abs(ev.clientX - sx) < 3 && Math.abs(ev.clientY - sy) < 3) return;
        const [px0, py0] = toPdf(sx, sy);
        const [px1, py1] = toPdf(ev.clientX, ev.clientY);
        const rx1 = Math.min(px0, px1);
        const rx2 = Math.max(px0, px1);
        const ry1 = Math.min(py0, py1);
        const ry2 = Math.max(py0, py1);
        const hits = boxes.filter(
          (b) => !(b.pdf[2] < rx1 || b.pdf[0] > rx2 || b.pdf[3] < ry1 || b.pdf[1] > ry2),
        );
        if (!hits.length) {
          useStore.getState().setStatus(t("stNoObjectsArea"));
          return;
        }
        if (await confirmDialog(t("cfDeleteObjects", { n: hits.length }))) {
          addObjectEdits(
            tabId,
            hits.map((b) => ({ src: spec.src, id: b.o.id, action: "delete" as const })),
          );
        }
      },
    );
  };

  const sel = objSel && objSel.src === spec.src ? (boxes.find((b) => b.o.id === objSel.id) ?? null) : null;

  return (
    <div
      ref={layerRef}
      className="obj-layer"
      style={{ pointerEvents: tool === "erase" ? "auto" : "none" }}
      onPointerDown={tool === "erase" ? startErase : undefined}
    >
      {tool === "edit" &&
        boxes.map((b) => (
          <div
            key={b.o.id}
            className={"obj-box" + (sel?.o.id === b.o.id ? " selected" : "")}
            style={{ left: b.l, top: b.t, width: b.w, height: b.h, pointerEvents: "auto" }}
            onPointerDown={(e) => startMove(e, b)}
            onDoubleClick={(e) => {
              if (b.o.kind !== "text") return;
              e.stopPropagation();
              useStore.getState().setObjSel({ src: spec.src, id: b.o.id });
              setTextEdit({ id: b.o.id, pdf: b.pdf });
            }}
          />
        ))}
      {tool === "edit" &&
        sel &&
        (sel.o.kind === "image" || sel.o.kind === "form") &&
        [0, 1, 2, 3].map((c) => (
          <div
            key={c}
            className="obj-handle"
            style={{
              left: (c % 2 ? sel.l + sel.w : sel.l) - 4,
              top: (c > 1 ? sel.t + sel.h : sel.t) - 4,
              cursor: c === 0 || c === 3 ? "nwse-resize" : "nesw-resize",
              pointerEvents: "auto",
            }}
            onPointerDown={(e) => startScale(e, sel, c)}
          />
        ))}
      {tool === "edit" && sel && (
        <div
          className="obj-actions"
          style={{ left: sel.l, top: Math.max(0, sel.t - 30), pointerEvents: "auto" }}
        >
          {sel.o.kind === "text" && (
            <button data-tip={t("tipEditText")} onClick={() => setTextEdit({ id: sel.o.id, pdf: sel.pdf })}>
              {t("btnEditText")}
            </button>
          )}
          <button data-tip={t("tipDelete")} onClick={() => deleteObject(tabId, spec.src, sel.o.id)}>
            {t("btnDelete")}
          </button>
        </div>
      )}
      {tool === "edit" &&
        textEdit &&
        (() => {
          const [ax, ay] = viewport.convertToViewportPoint(textEdit.pdf[0], textEdit.pdf[1]) as [
            number,
            number,
          ];
          const [bx, by] = viewport.convertToViewportPoint(textEdit.pdf[2], textEdit.pdf[3]) as [
            number,
            number,
          ];
          const l = Math.max(4, Math.min(Math.min(ax, bx), viewport.width - 292));
          const top = Math.max(4, Math.min(Math.max(ay, by) + 6, viewport.height - 200));
          return (
            <TextEditPopover
              entry={entry}
              tabId={tabId}
              src={spec.src}
              objId={textEdit.id}
              bbox={textEdit.pdf}
              left={l}
              top={top}
              zoom={viewport.scale}
              onDone={() => setTextEdit(null)}
            />
          );
        })()}
      {ghost && (
        <div
          className="erase-rect"
          style={{ left: ghost.l, top: ghost.t, width: ghost.w, height: ghost.h }}
        />
      )}
      {eraseRect && (
        <div
          className="erase-rect"
          style={{ left: eraseRect.l, top: eraseRect.t, width: eraseRect.w, height: eraseRect.h }}
        />
      )}
    </div>
  );
}
