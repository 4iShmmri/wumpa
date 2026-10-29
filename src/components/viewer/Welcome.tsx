import { useState } from "react";
import { useStore } from "../../store";
import {
  openFileDialog,
  openPaths,
  mergeFiles,
  getRecentFiles,
  dropRecent,
  type RecentFile,
} from "../../actions";
import { useT } from "../../i18n";

// Empty state shown when no tab is open: open / new / merge plus recent files.
export default function Welcome() {
  const t = useT();
  // Mounted fresh each time the last tab closes, so reading once is current.
  const [recents, setRecents] = useState<RecentFile[]>(() => getRecentFiles());

  return (
    <div className="viewer welcome">
      <aside className="welcome-side">
        <div className="welcome-brand">
          <svg
            width="34"
            height="34"
            viewBox="0 0 24 24"
            fill="none"
            stroke="var(--accent)"
            strokeWidth="1.6"
            strokeLinecap="round"
            strokeLinejoin="round"
          >
            <path d="M14 2H6a2 2 0 0 0-2 2v16a2 2 0 0 0 2 2h12a2 2 0 0 0 2-2V8z" />
            <path d="M14 2v6h6" />
            <path d="M8 13h8 M8 17h5" />
          </svg>
          <div>
            <h1>Wumpa</h1>
            <p>{t("wTagline")}</p>
          </div>
        </div>
        <button
          className="btn-primary welcome-open"
          data-tip={t("tipWelcomeOpen")}
          onClick={() => openFileDialog()}
        >
          {t("wOpen")}
        </button>
        <button
          className="welcome-secondary"
          data-tip={t("tipWelcomeNew")}
          onClick={() => useStore.getState().setUtilModal("newpdf")}
        >
          {t("tipNewPdf")}
        </button>
        <button className="welcome-secondary" data-tip={t("tipWelcomeMerge")} onClick={() => mergeFiles()}>
          {t("wMerge")}
        </button>
        <div className="welcome-recent">
          <h3>{t("wRecent")}</h3>
          {recents.length === 0 && <p className="recent-empty">{t("wRecentEmpty")}</p>}
          {recents.map((r) => (
            <button
              key={r.path}
              className="recent-row"
              title={r.path}
              onClick={async () => {
                await openPaths([r.path]);
                if (!useStore.getState().tabs.some((t) => t.path === r.path)) {
                  dropRecent(r.path);
                  setRecents(getRecentFiles());
                }
              }}
            >
              <svg
                width="15"
                height="15"
                viewBox="0 0 24 24"
                fill="none"
                stroke="currentColor"
                strokeWidth="1.6"
              >
                <path d="M14 2H6a2 2 0 0 0-2 2v16a2 2 0 0 0 2 2h12a2 2 0 0 0 2-2V8z M14 2v6h6" />
              </svg>
              <span className="recent-meta">
                <span className="recent-name">{r.name}</span>
                <span className="recent-path">{r.path}</span>
              </span>
            </button>
          ))}
        </div>
      </aside>
      <div className="welcome-main">
        <div className="welcome-hero">
          <svg
            width="120"
            height="120"
            viewBox="0 0 24 24"
            fill="none"
            stroke="currentColor"
            strokeWidth="0.9"
            strokeLinecap="round"
            strokeLinejoin="round"
          >
            <path d="M14 2H6a2 2 0 0 0-2 2v16a2 2 0 0 0 2 2h12a2 2 0 0 0 2-2V8z" />
            <path d="M14 2v6h6" />
            <path d="M8 12h8 M8 15.5h8 M8 19h4" />
          </svg>
          <h2>{t("wHeroTitle")}</h2>
          <p>
            {t("wHeroOr")} <kbd>⌘O</kbd> {t("wHeroBrowse")}
          </p>
          <div className="welcome-hints">
            <span>
              <kbd>E</kbd> {t("wHintEdit")}
            </span>
            <span>
              <kbd>T</kbd> {t("wHintText")}
            </span>
            <span>
              <kbd>S</kbd> {t("wHintSign")}
            </span>
            <span>
              <kbd>D</kbd> {t("wHintDraw")}
            </span>
            <span>
              <kbd>⌘F</kbd> {t("wHintSearch")}
            </span>
          </div>
        </div>
      </div>
    </div>
  );
}
