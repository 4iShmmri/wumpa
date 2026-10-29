import { useStore } from "../store";
import { closeTabSafe, openFileDialog } from "../actions";
import { useT } from "../i18n";

export default function TabBar() {
  const t = useT();
  const tabs = useStore((s) => s.tabs);
  const activeId = useStore((s) => s.activeId);

  if (!tabs.length) return null;

  // Arrow keys move focus along the tabs; Enter/Space activate the focused one.
  const onTabKey = (e: React.KeyboardEvent<HTMLDivElement>, id: string) => {
    if (e.target !== e.currentTarget) return;
    if (e.key === "Enter" || e.key === " ") {
      e.preventDefault();
      useStore.getState().setActive(id);
    } else if (e.key === "ArrowLeft" || e.key === "ArrowRight" || e.key === "Home" || e.key === "End") {
      const all = Array.from(e.currentTarget.parentElement!.querySelectorAll<HTMLElement>("[role=tab]"));
      const i = all.indexOf(e.currentTarget);
      const rtl = getComputedStyle(e.currentTarget).direction === "rtl";
      const fwd = (e.key === "ArrowRight") !== rtl;
      const next =
        e.key === "Home"
          ? 0
          : e.key === "End"
            ? all.length - 1
            : (i + (fwd ? 1 : -1) + all.length) % all.length;
      e.preventDefault();
      all[next]?.focus();
    }
  };

  return (
    <div className="tabbar" role="tablist">
      {tabs.map((tb) => (
        <div
          key={tb.id}
          className={"tab" + (tb.id === activeId ? " active" : "")}
          role="tab"
          tabIndex={0}
          aria-selected={tb.id === activeId}
          onClick={() => useStore.getState().setActive(tb.id)}
          onKeyDown={(e) => onTabKey(e, tb.id)}
          onAuxClick={(e) => {
            if (e.button === 1) closeTabSafe(tb.id);
          }}
          title={tb.path}
        >
          <span className="tab-name">
            {tb.dirty && <span className="dirty-dot" />}
            {tb.name}
          </span>
          <button
            type="button"
            className="tab-close"
            data-tip={t("tipCloseTab")}
            aria-label={t("tipCloseTab")}
            onClick={(e) => {
              e.stopPropagation();
              closeTabSafe(tb.id);
            }}
          >
            ×
          </button>
        </div>
      ))}
      <button
        className="tab-add"
        data-tip={t("tipOpenTab")}
        aria-label={t("tipOpenTab")}
        onClick={() => openFileDialog()}
      >
        +
      </button>
    </div>
  );
}
