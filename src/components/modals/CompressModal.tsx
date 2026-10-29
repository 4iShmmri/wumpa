import { useState } from "react";
import Modal from "../ui/Modal";
import { useActiveTab } from "../../store";
import { useT } from "../../i18n";
import { confirmDialog } from "../../backend";
import { compressActive } from "../../actions";

const PRESETS = [
  { key: "compressHigh" as const, desc: "compressHighDesc" as const, q: 85, dim: 2400 },
  { key: "compressBalanced" as const, desc: "compressBalancedDesc" as const, q: 70, dim: 1600 },
  { key: "compressSmallest" as const, desc: "compressSmallestDesc" as const, q: 55, dim: 1100 },
];

export default function CompressModal({ onClose }: { onClose: () => void }) {
  const t = useT();
  const tab = useActiveTab();
  const [sel, setSel] = useState(1);
  if (!tab) return null;
  const run = async () => {
    if (tab.dirty && !(await confirmDialog(t("compressDirtyWarn")))) return;
    onClose();
    const p = PRESETS[sel];
    await compressActive(p.q, p.dim);
  };
  return (
    <Modal onClose={onClose}>
      <h3>{t("compressTitle")}</h3>
      <div className="preset-list">
        {PRESETS.map((p, i) => (
          <label key={p.key} className={"preset-row" + (sel === i ? " on" : "")}>
            <input type="radio" name="preset" checked={sel === i} onChange={() => setSel(i)} />
            <span className="preset-meta">
              <span className="preset-name">{t(p.key)}</span>
              <span className="preset-desc">{t(p.desc)}</span>
            </span>
          </label>
        ))}
      </div>
      <div className="modal-actions">
        <button data-tip={t("tipCancelEsc")} onClick={onClose}>
          {t("btnCancel")}
        </button>
        <button className="btn-primary" data-tip={t("tipCompressGo")} onClick={run}>
          {t("btnCompress")}
        </button>
      </div>
    </Modal>
  );
}
