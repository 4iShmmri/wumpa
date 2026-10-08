import { useStore } from "../store";
import { closeTabSafe, openFileDialog } from "../actions";
import { useT } from "../i18n";

export default function TabBar() {
  const t = useT();
  const tabs = useStore((s) => s.tabs);
  const activeId = useStore((s) => s.activeId);

  if (!tabs.length) return null;

  return (
    <div className="tabbar">
      {tabs.map((tb) => (
        <div
          key={tb.id}
          className={"tab" + (tb.id === activeId ? " active" : "")}
          onClick={() => useStore.getState().setActive(tb.id)}
          onAuxClick={(e) => {
            if (e.button === 1) closeTabSafe(tb.id);
          }}
          title={tb.path}
        >
          <span className="tab-name">
            {tb.dirty && <span className="dirty-dot" />}
            {tb.name}
          </span>
          <span
            className="tab-close"
            data-tip={t("tipCloseTab")}
            onClick={(e) => {
              e.stopPropagation();
              closeTabSafe(tb.id);
            }}
          >
            ×
          </span>
        </div>
      ))}
      <button className="tab-add" data-tip={t("tipOpenTab")} onClick={() => openFileDialog()}>
        +
      </button>
    </div>
  );
}
