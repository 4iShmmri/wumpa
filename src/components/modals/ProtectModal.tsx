import { useState } from "react";
import Modal from "../ui/Modal";
import { useActiveTab } from "../../store";
import { useT } from "../../i18n";
import { confirmDialog } from "../../backend";
import { protectActive } from "../../actions";

function passwordStrength(pw: string): number {
  if (!pw) return 0;
  let score = 0;
  if (pw.length >= 8) score++;
  if (pw.length >= 12) score++;
  if (pw.length >= 16) score++;
  const kinds = [/[a-z]/, /[A-Z]/, /\d/, /[^A-Za-z0-9]/].filter((r) => r.test(pw)).length;
  if (kinds >= 3) score++;
  if (pw.length < 8) score = Math.min(score, 1);
  return Math.min(4, score);
}

function generatePassword(len = 16): string {
  const chars = "ABCDEFGHJKLMNPQRSTUVWXYZabcdefghijkmnopqrstuvwxyz23456789!@#$%&*?";
  const buf = new Uint32Array(len);
  crypto.getRandomValues(buf);
  return Array.from(buf, (n) => chars[n % chars.length]).join("");
}

function StrengthMeter({ pw }: { pw: string }) {
  const t = useT();
  const s = passwordStrength(pw);
  if (!pw) return null;
  const labels = ["pwWeak", "pwWeak", "pwFair", "pwStrong", "pwVeryStrong"] as const;
  return (
    <div className={"pw-meter s" + s}>
      <i />
      <i />
      <i />
      <i />
      <span>{t(labels[s])}</span>
    </div>
  );
}

export default function EncryptModal({ onClose }: { onClose: () => void }) {
  const t = useT();
  const tab = useActiveTab();
  const [openPw, setOpenPw] = useState("");
  const [openPw2, setOpenPw2] = useState("");
  const [ownerPw, setOwnerPw] = useState("");
  const [ownerPw2, setOwnerPw2] = useState("");
  const [perms, setPerms] = useState({ print: true, copy: true, modify: false, annotate: true });
  const [wipe, setWipe] = useState(false);
  const [maxAttempts, setMaxAttempts] = useState(10);
  const [error, setError] = useState<string | null>(null);
  if (!tab) return null;
  const already = !!tab.security?.encrypted;
  const run = async () => {
    if (!openPw && !ownerPw) return setError(t("protNeedPw"));
    if (openPw !== openPw2 || ownerPw !== ownerPw2) return setError(t("protMismatch"));
    if ((openPw && openPw.length < 8) || (ownerPw && ownerPw.length < 8)) return setError(t("protTooShort"));
    if (tab.dirty && !(await confirmDialog(t("protDirtyWarn")))) return;
    if (wipe && !(await confirmDialog(t("protWipeConfirm", { n: maxAttempts })))) return;
    onClose();
    await protectActive(openPw, ownerPw, perms, wipe ? { maxAttempts, wipe: true } : null);
  };
  const suggest = () => {
    const pw = generatePassword();
    setOpenPw(pw);
    setOpenPw2(pw);
    void navigator.clipboard?.writeText(pw).catch(() => {});
    setError(null);
  };
  const toggle = (k: keyof typeof perms) => setPerms((p) => ({ ...p, [k]: !p[k] }));
  return (
    <Modal onClose={onClose} wide>
      <h3>{t("protTitle")}</h3>
      {already ? (
        <p className="modal-hint">{t("protAlready")}</p>
      ) : (
        <>
          <div className="sec-grid">
            <label className="sec-field">
              <span>{t("protOpenPw")}</span>
              <input type="password" value={openPw} onChange={(e) => setOpenPw(e.target.value)} autoFocus />
            </label>
            <label className="sec-field">
              <span>{t("protConfirm")}</span>
              <input type="password" value={openPw2} onChange={(e) => setOpenPw2(e.target.value)} />
            </label>
          </div>
          <div className="pw-row">
            <StrengthMeter pw={openPw} />
            <button className="pw-gen" onClick={suggest}>
              {t("pwGenerate")}
            </button>
          </div>
          <p className="modal-hint">{t("protOpenPwHint")}</p>
          <div className="sec-grid">
            <label className="sec-field">
              <span>{t("protOwnerPw")}</span>
              <input type="password" value={ownerPw} onChange={(e) => setOwnerPw(e.target.value)} />
            </label>
            <label className="sec-field">
              <span>{t("protConfirm")}</span>
              <input type="password" value={ownerPw2} onChange={(e) => setOwnerPw2(e.target.value)} />
            </label>
          </div>
          <StrengthMeter pw={ownerPw} />
          <p className="modal-hint">{t("protOwnerPwHint")}</p>
          <div className="sec-perms">
            <div className="sec-perms-k">{t("protPerms")}</div>
            <label>
              <input type="checkbox" checked={perms.print} onChange={() => toggle("print")} />
              {t("permPrint")}
            </label>
            <label>
              <input type="checkbox" checked={perms.copy} onChange={() => toggle("copy")} />
              {t("permCopy")}
            </label>
            <label>
              <input type="checkbox" checked={perms.modify} onChange={() => toggle("modify")} />
              {t("permModify")}
            </label>
            <label>
              <input type="checkbox" checked={perms.annotate} onChange={() => toggle("annotate")} />
              {t("permAnnotate")}
            </label>
          </div>
          <div className="sec-perms">
            <div className="sec-perms-k">{t("protGuessing")}</div>
            <p className="modal-hint">{t("protGuessingHint")}</p>
            <label>
              <input type="checkbox" checked={wipe} onChange={() => setWipe((v) => !v)} />
              {t("protWipe")}
              <input
                type="number"
                min={3}
                max={100}
                value={maxAttempts}
                disabled={!wipe}
                onChange={(e) => setMaxAttempts(Math.max(3, Math.min(100, Number(e.target.value) || 10)))}
                className="pw-attempts"
              />
            </label>
            {wipe && <p className="modal-hint pw-danger">{t("protWipeHint")}</p>}
          </div>
          <p className="modal-hint">{t("protMethod")}</p>
          {error && <p className="modal-error">{error}</p>}
        </>
      )}
      <div className="modal-actions">
        <button data-tip={t("tipCancelEsc")} onClick={onClose}>
          {t("btnCancel")}
        </button>
        {!already && (
          <button className="btn-primary" onClick={run}>
            {t("btnProtect")}
          </button>
        )}
      </div>
    </Modal>
  );
}
