import { useState } from "react";
import Modal from "../ui/Modal";
import { useT } from "../../i18n";
import { splitEvery } from "../../actions";

export default function SplitModal({ onClose }: { onClose: () => void }) {
  const t = useT();
  const [n, setN] = useState(1);
  return (
    <Modal onClose={onClose}>
      <h3>{t("splitTitle")}</h3>
      <label>
        {t("splitEvery")}
        <input
          type="number"
          min={1}
          value={n}
          autoFocus
          onChange={(e) => setN(Math.max(1, Number(e.target.value) || 1))}
          onKeyDown={(e) => {
            if (e.key === "Enter") {
              onClose();
              splitEvery(n);
            }
          }}
        />
        {t("splitPages")}
      </label>
      <div className="modal-actions">
        <button data-tip={t("tipCancelEsc")} onClick={onClose}>
          {t("btnCancel")}
        </button>
        <button
          className="btn-primary"
          data-tip={t("tipSplitGo")}
          onClick={() => {
            onClose();
            splitEvery(n);
          }}
        >
          {t("btnChooseFolderSplit")}
        </button>
      </div>
    </Modal>
  );
}
