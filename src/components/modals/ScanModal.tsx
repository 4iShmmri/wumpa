import { useEffect, useState } from "react";
import Modal from "../ui/Modal";
import { useActiveTab } from "../../store";
import { useT } from "../../i18n";
import { rebuildScannedActive, scannedPagesOfActive } from "../../actions";
import type { ScanProgress } from "../../backend";

const STAGES = ["detect", "prepare", "recognize", "layout", "compose"] as const;

export default function ScanModal({ onClose }: { onClose: () => void }) {
  const t = useT();
  const tab = useActiveTab();
  const [pages, setPages] = useState<number[] | null>(null);
  const [scope, setScope] = useState<"all" | "current">("all");
  const [ar, setAr] = useState(true);
  const [en, setEn] = useState(true);
  const [progress, setProgress] = useState<ScanProgress | null>(null);
  const [busy, setBusy] = useState(false);

  useEffect(() => {
    let on = true;
    void scannedPagesOfActive().then((p) => on && setPages(p));
    return () => {
      on = false;
    };
  }, [tab?.id, tab?.docVersion]);

  if (!tab) return null;
  const currentIsScan = pages?.includes(tab.pages[tab.currentPage - 1]?.src ?? -1) ?? false;
  const chosen = scope === "all" ? (pages ?? []) : currentIsScan ? [tab.pages[tab.currentPage - 1].src] : [];
  const langs = [ar && "ar", en && "en"].filter((l): l is string => !!l);

  const run = async () => {
    setBusy(true);
    const ok = await rebuildScannedActive(chosen, langs, setProgress);
    setBusy(false);
    if (ok) onClose();
    else setProgress(null);
  };

  const stageIndex = progress ? Math.max(0, STAGES.indexOf(progress.stage as (typeof STAGES)[number])) : 0;
  const pct = progress
    ? Math.round(((progress.index + (stageIndex + 1) / STAGES.length) / progress.total) * 100)
    : 0;

  return (
    <Modal onClose={busy ? () => {} : onClose} wide>
      <h3>{t("scanTitle")}</h3>
      {pages === null ? (
        <p className="modal-hint">{t("scanDetecting")}</p>
      ) : pages.length === 0 ? (
        <p className="modal-hint">{t("scanNone")}</p>
      ) : busy && progress ? (
        <div className="scan-progress">
          <div className="scan-bar">
            <i style={{ width: pct + "%" }} />
          </div>
          <p>{t("scanProgress", { page: progress.page, n: progress.index + 1, total: progress.total })}</p>
          <p className="modal-hint">{t(`scanStage_${progress.stage}` as "scanStage_recognize")}</p>
        </div>
      ) : (
        <>
          <p className="modal-hint">{t("scanFound", { n: pages.length })}</p>
          <div className="sec-perms">
            <label>
              <input type="radio" name="scope" checked={scope === "all"} onChange={() => setScope("all")} />
              {t("scanAll", { n: pages.length })}
            </label>
            <label>
              <input
                type="radio"
                name="scope"
                checked={scope === "current"}
                disabled={!currentIsScan}
                onChange={() => setScope("current")}
              />
              {t("scanCurrent", { n: tab.currentPage })}
            </label>
          </div>
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
          <p className="modal-hint">{t("scanHint")}</p>
        </>
      )}
      <div className="modal-actions">
        <button data-tip={t("tipCancelEsc")} disabled={busy} onClick={onClose}>
          {t("btnCancel")}
        </button>
        {pages && pages.length > 0 && (
          <button className="btn-primary" disabled={busy || !chosen.length || !langs.length} onClick={run}>
            {busy ? t("scanWorking") : t("btnScan")}
          </button>
        )}
      </div>
    </Modal>
  );
}
