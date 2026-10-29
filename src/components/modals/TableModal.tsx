import { useState } from "react";
import Modal from "../ui/Modal";

import { useT } from "../../i18n";

import { insertTableGrid } from "../../actions";

export default function TableModal({ onClose }: { onClose: () => void }) {
  const t = useT();
  const [rows, setRows] = useState(3);
  const [cols, setCols] = useState(3);
  const run = () => {
    onClose();
    void insertTableGrid(Math.max(1, Math.min(20, rows)), Math.max(1, Math.min(10, cols)));
  };
  return (
    <Modal onClose={onClose}>
      <h3>{t("tableTitle")}</h3>
      <div className="form-rows">
        <label className="form-row">
          <span>{t("tableRows")}</span>
          <input
            type="number"
            min={1}
            max={20}
            value={rows}
            autoFocus
            onChange={(e) => setRows(Number(e.target.value) || 1)}
            onKeyDown={(e) => e.key === "Enter" && run()}
          />
        </label>
        <label className="form-row">
          <span>{t("tableCols")}</span>
          <input
            type="number"
            min={1}
            max={10}
            value={cols}
            onChange={(e) => setCols(Number(e.target.value) || 1)}
            onKeyDown={(e) => e.key === "Enter" && run()}
          />
        </label>
      </div>
      <p className="modal-hint">{t("tableHint")}</p>
      <div className="modal-actions">
        <button data-tip={t("tipCancelEsc")} onClick={onClose}>
          {t("btnCancel")}
        </button>
        <button className="btn-primary" data-tip={t("tipInsertGo")} onClick={run}>
          {t("btnInsert")}
        </button>
      </div>
    </Modal>
  );
}
