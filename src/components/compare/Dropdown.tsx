import Icon from "../ui/Icon";
import { useMenuButton } from "../ui/menuKeys";
import { ICON } from "./shared";

export interface DdItem {
  label: string;
  checked?: boolean;
  run: () => void;
}

// A labelled toolbar button that opens a menu of plain or check items.
export default function Dropdown({
  icon,
  label,
  tip,
  items,
  check,
}: {
  icon?: string;
  label: string;
  tip?: string;
  items: DdItem[];
  check?: boolean;
}) {
  const { open, setOpen, wrapRef, triggerRef, triggerProps, menuProps } = useMenuButton<HTMLSpanElement>();
  return (
    <span className="cmp-dd tb-menu-wrap" ref={wrapRef}>
      <button
        ref={triggerRef}
        className={"cmp-tb-btn" + (open ? " active" : "")}
        data-tip={open ? undefined : tip}
        {...triggerProps}
      >
        {icon && <Icon d={icon} size={15} />}
        <span>{label}</span>
        <span className="tb-caret">
          <Icon d={ICON.caret} size={10} stroke={2.6} />
        </span>
      </button>
      {open && (
        <div className="tb-menu" {...menuProps}>
          {items.map((it, i) => (
            <button
              key={i}
              role={check ? "menuitemcheckbox" : "menuitem"}
              aria-checked={check ? !!it.checked : undefined}
              onClick={() => {
                if (!check) setOpen(false);
                it.run();
              }}
            >
              <span className="cmp-dd-check">{it.checked ? <Icon d={ICON.check} size={14} /> : null}</span>
              <span>{it.label}</span>
            </button>
          ))}
        </div>
      )}
    </span>
  );
}
