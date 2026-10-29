import { useEffect, useState } from "react";
import Modal from "../ui/Modal";
import { useActiveTab } from "../../store";
import { useT } from "../../i18n";
import { exportActive, scannedPagesOfActive, type ExportStatus } from "../../actions";
import type { ExportFormat } from "../../backend";

const STAGES = ["extract", "recognize", "layout", "write"] as const;

export default function ExportModal({ format, onClose }: { format: ExportFormat; onClose: () => void }) {
  const t = useT();
  const tab = useActiveTab();
  const [scanned, setScanned] = useState<number[]>([]);
  const [scope, setScope] = useState<"all" | "current">("all");
  const [ar, setAr] = useState(true);
  const [en, setEn] = useState(true);
  const [progress, setProgress] = useState<ExportStatus | null>(null);
  const [busy, setBusy] = useState(false);
  const [error, setError] = useState<string | null>(null);

  useEffect(() => {
    let on = true;
    void scannedPagesOfActive().then((p) => on && setScanned(p));
    return () => {
      on = false;
    };
  }, [tab?.id, tab?.docVersion]);

  if (!tab) return null;
  const real = tab.pages.filter((p) => p.src > 0).map((p) => p.src);
  const current = tab.pages[tab.currentPage - 1]?.src ?? -1;
  const chosen = scope === "all" ? real : current > 0 ? [current] : [];
  const scannedChosen = chosen.filter((s) => scanned.includes(s)).length;
  const langs = [ar && "ar", en && "en"].filter((l): l is string => !!l);
  const isDocx = format === "docx";

  const run = async () => {
    setBusy(true);
    setError(null);
    const res = await exportActive(format, chosen, langs, setProgress);
    setBusy(false);
    if (res.ok) onClose();
    else {
      setProgress(null);
      setError(res.error ?? null);
    }
  };

  const stageIndex = progress ? Math.max(0, STAGES.indexOf(progress.stage as (typeof STAGES)[number])) : 0;
  const pct = progress
    ? Math.round(((progress.index + (stageIndex + 1) / STAGES.length) / progress.total) * 100)
    : 0;

  return (
    <Modal onClose={busy ? () => {} : onClose} wide>
      <h3>{t(isDocx ? "exportTitleDocx" : "exportTitleXlsx")}</h3>
      {busy && progress ? (
        <div className="scan-progress">
          <div className="scan-bar">
            <i style={{ width: pct + "%" }} />
          </div>
          <p>{t("exportProgress", { page: progress.page, n: progress.index + 1, total: progress.total })}</p>
          <p className="modal-hint">{t(`exportStage_${progress.stage}` as "exportStage_layout")}</p>
        </div>
      ) : (
        <>
          <p className="modal-hint">{t(isDocx ? "exportIntroDocx" : "exportIntroXlsx")}</p>
          {error && (
            <p className="modal-error">
              {t("stExportFailed")}: {error}
            </p>
          )}
          <div className="sec-perms">
            <label>
              <input type="radio" name="scope" checked={scope === "all"} onChange={() => setScope("all")} />
              {t("exportAll", { n: real.length })}
            </label>
            <label>
              <input
                type="radio"
                name="scope"
                checked={scope === "current"}
                disabled={current <= 0}
                onChange={() => setScope("current")}
              />
              {t("exportCurrent", { n: tab.currentPage })}
            </label>
          </div>
          {scannedChosen > 0 && (
            <>
              <p className="modal-hint">{t("exportScanned", { n: scannedChosen })}</p>
              <div className="sec-perms">
                <div className="sec-perms-k">{t("scanLangs")}</div>
                <label>
                  <input type="checkbox" checked={ar} onChange={() => setAr((v) => !v)} />
                  {t("langAr")}
                </label>
                <label>
                  <input type="checkbox" checked={en} onChange={() => setEn((v) => !v)} />
                  {t("langEn")}
                </label>
              </div>
            </>
          )}
        </>
      )}
      <div className="modal-actions">
        <button data-tip={t("tipCancelEsc")} disabled={busy} onClick={onClose}>
          {t("btnCancel")}
        </button>
        <button
          className="btn-primary"
          disabled={busy || !chosen.length || (scannedChosen > 0 && !langs.length)}
          onClick={run}
        >
          {busy ? t("scanWorking") : t("btnExport")}
        </button>
      </div>
    </Modal>
  );
}
