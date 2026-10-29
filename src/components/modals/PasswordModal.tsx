import { useEffect, useState } from "react";
import Modal from "../ui/Modal";
import { useStore } from "../../store";
import { useT } from "../../i18n";

export default function PasswordModal() {
  const t = useT();
  const prompt = useStore((s) => s.passwordPrompt);
  const [pw, setPw] = useState("");
  const [left, setLeft] = useState(0);

  useEffect(() => {
    setPw("");
    setLeft(prompt?.status?.lockedSecs ?? 0);
  }, [prompt]);

  // Lockout countdown: the backend enforces the real clock, this only mirrors it.
  useEffect(() => {
    if (left <= 0) return;
    const id = setInterval(() => setLeft((v) => Math.max(0, v - 1)), 1000);
    return () => clearInterval(id);
  }, [left > 0]);

  if (!prompt) return null;
  const st = prompt.status;
  const locked = left > 0;
  const remaining = st?.remaining ?? null;
  const submit = () => {
    if (!locked && pw) prompt.resolve(pw);
  };
  const mm = Math.floor(left / 60);
  const ss = String(left % 60).padStart(2, "0");
  return (
    <Modal onClose={() => prompt.resolve(null)} className="pw-modal">
      <h3>🔒 {t("pwTitle")}</h3>
      <p className="modal-hint">{t("pwPrompt", { name: prompt.name })}</p>
      <label className="sec-field">
        <span>{t("pwLabel")}</span>
        <input
          type="password"
          value={pw}
          autoFocus
          disabled={locked}
          onChange={(e) => setPw(e.target.value)}
          onKeyDown={(e) => {
            if (e.key === "Enter") submit();
          }}
        />
      </label>
      {prompt.wrong && !locked && <p className="modal-error">{t("pwWrong")}</p>}
      {locked && <p className="modal-error">{t("pwLocked", { time: `${mm}:${ss}` })}</p>}
      {remaining != null && st?.wipe && (
        <p className={"modal-hint" + (remaining <= 3 ? " pw-danger" : "")}>
          {t("pwRemainingWipe", { n: remaining })}
        </p>
      )}
      <div className="modal-actions">
        <button data-tip={t("tipCancelEsc")} onClick={() => prompt.resolve(null)}>
          {t("btnCancel")}
        </button>
        <button className="btn-primary" disabled={!pw || locked} onClick={submit}>
          {t("btnOpen")}
        </button>
      </div>
    </Modal>
  );
}
