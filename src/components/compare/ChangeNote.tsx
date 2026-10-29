import { useT } from "../../i18n";
import Icon from "../ui/Icon";
import { KIND_LABEL, type Change } from "../../compare";
import { ICON, KindIcon } from "./shared";

// Acrobat sticky-note style popup for the current change.
export default function ChangeNote({
  change,
  total,
  onPrev,
  onNext,
  onClose,
}: {
  change: Change;
  total: number;
  onPrev: () => void;
  onNext: () => void;
  onClose: () => void;
}) {
  const t = useT();
  return (
    <div className={"cmp-note " + change.kind} onClick={(e) => e.stopPropagation()}>
      <div className="cmp-note-head">
        <KindIcon kind={change.kind} />
        <span className="cmp-note-title">{t(KIND_LABEL[change.kind])}</span>
        <button
          className="cmp-note-x"
          data-tip={t("tipCmpClose")}
          aria-label={t("tipCmpClose")}
          onClick={onClose}
        >
          <Icon d={ICON.close} size={13} />
        </button>
      </div>
      <div className="cmp-note-body">
        {(change.kind === "replaced" || change.kind === "deleted") && (
          <div className="cmp-note-row">
            <span>[{t("cmpOldLabel")}]:</span> <bdi>"{change.oldText}"</bdi>
          </div>
        )}
        {(change.kind === "replaced" || change.kind === "inserted") && (
          <div className="cmp-note-row">
            <span>[{t("cmpNewLabel")}]:</span> <bdi>"{change.newText}"</bdi>
          </div>
        )}
      </div>
      <div className="cmp-note-foot">
        <span>{t("cmpChangeOf", { i: change.id + 1, n: total })}</span>
        <button
          className="cmp-note-nav"
          data-tip={t("tipCmpPrev")}
          aria-label={t("tipCmpPrev")}
          onClick={onPrev}
        >
          <Icon d={ICON.up} size={14} />
        </button>
        <button
          className="cmp-note-nav"
          data-tip={t("tipCmpNext")}
          aria-label={t("tipCmpNext")}
          onClick={onNext}
        >
          <Icon d={ICON.down} size={14} />
        </button>
      </div>
    </div>
  );
}
