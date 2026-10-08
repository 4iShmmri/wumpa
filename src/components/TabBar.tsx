import { useStore } from "../store";
import { closeTabSafe, openFileDialog } from "../actions";

export default function TabBar() {
  const tabs = useStore((s) => s.tabs);
  const activeId = useStore((s) => s.activeId);

  if (!tabs.length) return null;

  return (
    <div className="tabbar">
      {tabs.map((t) => (
        <div
          key={t.id}
          className={"tab" + (t.id === activeId ? " active" : "")}
          onClick={() => useStore.getState().setActive(t.id)}
          onAuxClick={(e) => {
            if (e.button === 1) closeTabSafe(t.id);
          }}
          title={t.path}
        >
          <span className="tab-name">
            {t.dirty && <span className="dirty-dot" />}
            {t.name}
          </span>
          <span
            className="tab-close"
            onClick={(e) => {
              e.stopPropagation();
              closeTabSafe(t.id);
            }}
          >
            ×
          </span>
        </div>
      ))}
      <button className="tab-add" title="Open PDF (⌘O)" onClick={() => openFileDialog()}>
        +
      </button>
    </div>
  );
}
