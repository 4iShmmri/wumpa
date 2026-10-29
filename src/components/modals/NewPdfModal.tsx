import { useState } from "react";
import Modal from "../ui/Modal";

import { useT } from "../../i18n";

import { createNewPdf } from "../../actions";

const SIZES = [
  { key: "sizeA4" as const, w: 595.28, h: 841.89 },
  { key: "sizeLetter" as const, w: 612, h: 792 },
  { key: "sizeA5" as const, w: 419.5, h: 595.3 },
  { key: "sizeSquare" as const, w: 600, h: 600 },
  { key: "sizeCustom" as const, w: 0, h: 0 },
];

export default function NewPdfModal({ onClose }: { onClose: () => void }) {
  const t = useT();
  const [sizeIdx, setSizeIdx] = useState(0);
  const [cw, setCw] = useState(595);
  const [ch, setCh] = useState(842);
  const [pages, setPages] = useState(1);
  const custom = SIZES[sizeIdx].key === "sizeCustom";
  const run = () => {
    const w = custom ? cw : SIZES[sizeIdx].w;
    const h = custom ? ch : SIZES[sizeIdx].h;
    onClose();
    void createNewPdf(w, h, Math.max(1, Math.min(50, pages)));
  };
  return (
    <Modal onClose={onClose}>
      <h3>{t("newPdfTitle")}</h3>
      <div className="form-rows">
        <label className="form-row">
          <span>{t("newSize")}</span>
          <select value={sizeIdx} onChange={(e) => setSizeIdx(Number(e.target.value))} autoFocus>
            {SIZES.map((s, i) => (
              <option key={s.key} value={i}>
                {t(s.key)}
                {s.w > 0 ? ` — ${Math.round(s.w)}×${Math.round(s.h)}` : ""}
              </option>
            ))}
          </select>
        </label>
        {custom && (
          <>
            <label className="form-row">
              <span>{t("newWidth")}</span>
              <input
                type="number"
                min={72}
                max={5000}
                value={cw}
                onChange={(e) => setCw(Number(e.target.value) || 72)}
              />
            </label>
            <label className="form-row">
              <span>{t("newHeight")}</span>
              <input
                type="number"
                min={72}
                max={5000}
                value={ch}
                onChange={(e) => setCh(Number(e.target.value) || 72)}
              />
            </label>
          </>
        )}
        <label className="form-row">
          <span>{t("newPages")}</span>
          <input
            type="number"
            min={1}
            max={50}
            value={pages}
            onChange={(e) => setPages(Number(e.target.value) || 1)}
            onKeyDown={(e) => e.key === "Enter" && run()}
          />
        </label>
      </div>
      <div className="modal-actions">
        <button data-tip={t("tipCancelEsc")} onClick={onClose}>
          {t("btnCancel")}
        </button>
        <button className="btn-primary" data-tip={t("tipCreateGo")} onClick={run}>
          {t("btnCreate")}
        </button>
      </div>
    </Modal>
  );
}
