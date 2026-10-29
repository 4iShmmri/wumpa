import { useMemo } from "react";
import { useT } from "../../i18n";
import Icon from "../ui/Icon";
import { KIND_LABEL, groupByPage, type Change } from "../../compare";
import { ICON, KindIcon } from "./shared";

// The side list of changes, grouped by page and filtered by the search box.
export default function ChangesPane({
  changes,
  current,
  query,
  showEmpty,
  onQuery,
  onPick,
  onClose,
}: {
  changes: Change[];
  current: number;
  query: string;
  showEmpty: boolean;
  onQuery: (q: string) => void;
  onPick: (c: Change) => void;
  onClose: () => void;
}) {
  const t = useT();
  const groups = useMemo(() => groupByPage(changes, query), [changes, query]);

  return (
    <aside className="cmp-pane" onClick={(e) => e.stopPropagation()}>
      <div className="cmp-pane-head">
        <button
          className="cmp-pane-x"
          data-tip={t("tipCmpClose")}
          aria-label={t("tipCmpClose")}
          onClick={onClose}
        >
          <Icon d={ICON.close} size={13} />
        </button>
        <span className="cmp-pane-title">{t("cmpChanges", { n: changes.length })}</span>
      </div>
      <label className="cmp-pane-search">
        <Icon d={ICON.search} size={14} />
        <input value={query} onChange={(e) => onQuery(e.target.value)} placeholder={t("cmpSearchChanges")} />
      </label>
      <div className="cmp-pane-list">
        {groups.map(([page, list]) => (
          <div className="cmp-pane-group" key={page}>
            <div className="cmp-pane-gh">
              <span>{t("cmpPageN", { n: page + 1 })}</span>
              <b>{list.length}</b>
            </div>
            {list.map((c) => (
              <button
                key={c.id}
                className={"cmp-pane-item " + c.kind + (c.id === current ? " sel" : "")}
                aria-current={c.id === current ? "true" : undefined}
                onClick={() => onPick(c)}
              >
                <KindIcon kind={c.kind} size={24} />
                <span className="cmp-pane-it">
                  <span className="cmp-pane-kind">{t(KIND_LABEL[c.kind])}</span>
                  {(c.kind === "replaced" || c.kind === "deleted") && (
                    <span className="cmp-pane-txt">
                      [{t("cmpOldLabel")}]: <bdi>"{c.oldText}"</bdi>
                    </span>
                  )}
                  {(c.kind === "replaced" || c.kind === "inserted") && (
                    <span className="cmp-pane-txt">
                      [{t("cmpNewLabel")}]: <bdi>"{c.newText}"</bdi>
                    </span>
                  )}
                </span>
              </button>
            ))}
          </div>
        ))}
        {showEmpty && !groups.length && <div className="cmp-pane-empty">{t("cmpNoDiff")}</div>}
      </div>
    </aside>
  );
}
