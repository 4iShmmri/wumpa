import { useStore, useActiveTab } from "../store";

export default function StatusBar() {
  const tab = useActiveTab();
  const status = useStore((s) => s.status);

  return (
    <div className="statusbar">
      <span className="status-msg">{status}</span>
      <span className="status-right">
        {tab && (
          <>
            {tab.selected.length > 0 && <span>{tab.selected.length} selected · </span>}
            <span>
              Page {tab.currentPage} of {tab.pages.length}
            </span>
            <span> · {Math.round(tab.zoom * 100)}%</span>
          </>
        )}
      </span>
    </div>
  );
}
