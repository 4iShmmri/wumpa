import { useState } from "react";
import { useStore, useActiveTab } from "../store";
import { useT } from "../i18n";

export default function StatusBar() {
  const t = useT();
  const tab = useActiveTab();
  const status = useStore((s) => s.status);
  const [editVal, setEditVal] = useState<string | null>(null);

  const total = tab?.pages.length ?? 0;

  const goTo = (n: number) => {
    if (!tab) return;
    const page = Math.max(1, Math.min(total, n));
    useStore.getState().scrollTo(page);
  };

  const commit = (el?: HTMLInputElement) => {
    if (editVal !== null) {
      const n = parseInt(editVal, 10);
      if (Number.isFinite(n)) goTo(n);
      setEditVal(null);
    }
    el?.blur();
  };

  return (
    <div className="statusbar">
      <span className="status-msg">{status}</span>
      <span className="status-right">
        {tab && (
          <>
            {tab.selected.length > 0 && (
              <span>{t("sbSelected", { n: tab.selected.length })} · </span>
            )}
            <span className="page-nav">
              <button
                data-tip={t("tipPrevPage")}
                disabled={tab.currentPage <= 1}
                onClick={() => goTo(tab.currentPage - 1)}
              >
                ‹
              </button>
              <span className="goto-wrap" data-tip={t("tipGoToPage")}>
                <input
                  type="number"
                  min={1}
                  max={total}
                  value={editVal ?? String(tab.currentPage)}
                  onFocus={(e) => e.target.select()}
                  onChange={(e) => setEditVal(e.target.value)}
                  onBlur={(e) => commit(e.target)}
                  onKeyDown={(e) => {
                    if (e.key === "Enter") commit(e.currentTarget);
                    if (e.key === "Escape") {
                      setEditVal(null);
                      e.currentTarget.blur();
                    }
                  }}
                />
              </span>
              <span className="page-total">{t("sbOf", { total })}</span>
              <button
                data-tip={t("tipNextPage")}
                disabled={tab.currentPage >= total}
                onClick={() => goTo(tab.currentPage + 1)}
              >
                ›
              </button>
            </span>
            <span> · {Math.round(tab.zoom * 100)}%</span>
          </>
        )}
      </span>
    </div>
  );
}
