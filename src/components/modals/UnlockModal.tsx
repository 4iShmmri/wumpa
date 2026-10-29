import { useState } from "react";
import Modal from "../ui/Modal";
import { useActiveTab } from "../../store";
import { useT } from "../../i18n";

import { unprotectActive } from "../../actions";

export default function DecryptModal({ onClose }: { onClose: () => void }) {
  const t = useT();
  const tab = useActiveTab();
  const [pw, setPw] = useState("");
  const [busy, setBusy] = useState(false);
  if (!tab) return null;
  const protectedFile = !!tab.security?.encrypted;
  const run = async () => {
    setBusy(true);
    const ok = await unprotectActive(pw);
    setBusy(false);
    if (ok) onClose();
  };
  return (
    <Modal onClose={onClose}>
      <h3>{t("unprotTitle")}</h3>
      {protectedFile ? (
        <>
          <p className="modal-hint">{t("unprotHint")}</p>
          <label className="sec-field">
            <span>{t("pwLabel")}</span>
            <input
              type="password"
              value={pw}
              autoFocus
              onChange={(e) => setPw(e.target.value)}
              onKeyDown={(e) => {
                if (e.key === "Enter" && pw) void run();
              }}
            />
          </label>
        </>
      ) : (
        <p className="modal-hint">{t("unprotNotProtected")}</p>
      )}
      <div className="modal-actions">
        <button data-tip={t("tipCancelEsc")} onClick={onClose}>
          {t("btnCancel")}
        </button>
        {protectedFile && (
          <button className="btn-primary" disabled={!pw || busy} onClick={run}>
            {t("btnUnprotect")}
          </button>
        )}
      </div>
    </Modal>
  );
}
