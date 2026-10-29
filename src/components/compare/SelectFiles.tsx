import { useEffect, useMemo, useState } from "react";
import { useStore } from "../../store";
import { getDoc, evictDoc, docPath, type DocEntry } from "../../pdf";
import { report } from "../../errors";
import { useT } from "../../i18n";
import Icon from "../ui/Icon";
import PageThumb from "../ui/PageThumb";
import { useMenuButton } from "../ui/menuKeys";
import { pickPdfs, baseName } from "../../backend";
import { startCompare } from "../../actions";
import { ICON } from "./shared";

const THUMB_FIT = { w: 146, h: 186 };

// First page of a chosen file, or a file glyph while nothing is chosen.
function FileThumb({ path }: { path: string | null }) {
  const [entry, setEntry] = useState<DocEntry | null>(null);
  const [ok, setOk] = useState(false);
  useEffect(() => {
    setEntry(null);
    setOk(false);
    if (!path) return;
    const key = "__cmpThumb__" + path;
    let on = true;
    getDoc(key, path, 0, true)
      .then((e) => on && setEntry(e))
      .catch(() => {});
    return () => {
      on = false;
      evictDoc(key);
    };
  }, [path]);
  return (
    <div className={"cmp-thumb" + (path ? " has" : "")}>
      {!ok && <Icon d={ICON.file} size={48} stroke={1.2} />}
      {entry && (
        <PageThumb
          entry={entry}
          src={1}
          base={entry.dims[0]}
          fit={THUMB_FIT}
          lazy={false}
          onRendered={() => setOk(true)}
          style={{ display: ok ? "block" : "none" }}
        />
      )}
    </div>
  );
}

function FileSlot({
  label,
  path,
  onBrowse,
  onChoose,
}: {
  label: string;
  path: string | null;
  onBrowse: () => void;
  onChoose: (p: string) => void;
}) {
  const t = useT();
  const tabs = useStore((s) => s.tabs);
  const { open, setOpen, wrapRef, triggerRef, triggerProps, menuProps } = useMenuButton<HTMLSpanElement>();
  const openFiles = useMemo(() => {
    const seen = new Set<string>();
    return tabs
      .map((tb) => ({ path: docPath(tb), name: tb.name }))
      .filter((f) => f.path && !seen.has(f.path) && seen.add(f.path));
  }, [tabs]);
  return (
    <div className="cmp-slot">
      <div className="cmp-slot-k">{label}</div>
      <FileThumb path={path} />
      <div className={"cmp-slot-n" + (path ? "" : " empty")} title={path ?? undefined}>
        {path ? baseName(path) : " "}
      </div>
      <span className="cmp-split tb-menu-wrap" ref={wrapRef}>
        <button className="cmp-split-main" onClick={onBrowse}>
          {path ? t("cmpChangeFile") : t("cmpSelectFile")}
        </button>
        <button
          ref={triggerRef}
          className={"cmp-split-caret" + (open ? " active" : "")}
          aria-label={t("cmpOpenFiles")}
          {...triggerProps}
        >
          <Icon d={ICON.caret} size={11} stroke={2.6} />
        </button>
        {open && (
          <div className="tb-menu cmp-split-menu" {...menuProps}>
            <button
              role="menuitem"
              onClick={() => {
                setOpen(false);
                onBrowse();
              }}
            >
              <span className="cmp-dd-check" />
              <span>{t("cmpBrowse")}</span>
            </button>
            {openFiles.length > 0 && <div className="ctx-sep" role="separator" />}
            {openFiles.map((f) => (
              <button
                key={f.path}
                role="menuitemradio"
                aria-checked={f.path === path}
                onClick={() => {
                  setOpen(false);
                  onChoose(f.path);
                }}
              >
                <span className="cmp-dd-check">
                  {f.path === path ? <Icon d={ICON.check} size={14} /> : null}
                </span>
                <span>{f.name}</span>
              </button>
            ))}
          </div>
        )}
      </span>
    </div>
  );
}

// The "Select Files to Compare" page.
export default function SelectFiles({ oldPath }: { oldPath: string | null }) {
  const t = useT();
  const closeDialog = useStore((s) => s.closeCompareDialog);
  const [oldFile, setOldFile] = useState<string | null>(oldPath);
  const [newFile, setNewFile] = useState<string | null>(null);

  useEffect(() => {
    const onKey = (e: KeyboardEvent) => {
      if (e.key === "Escape" && !document.querySelector(".cmp-select .tb-menu")) {
        e.stopPropagation();
        closeDialog();
      }
    };
    window.addEventListener("keydown", onKey, true);
    return () => window.removeEventListener("keydown", onKey, true);
  }, [closeDialog]);

  const pick = async (set: (p: string) => void) => {
    try {
      const paths = await pickPdfs(false);
      if (paths && paths.length) set(paths[0]);
    } catch (e) {
      report(t("stCmpFailed"), e);
    }
  };

  return (
    <div className="cmp-overlay cmp-select">
      <div className="cmp-head">
        <div className="cmp-head-title">{t("cmpDlgTitle")}</div>
        <span />
        <button className="cmp-pill ghost" data-tip={t("tipCmpClose")} onClick={closeDialog}>
          {t("cmpClose")}
        </button>
      </div>
      <div className="cmp-select-body">
        <h1>{t("cmpSelectTitle")}</h1>
        <div className="cmp-slots">
          <FileSlot
            label={t("cmpOldFile")}
            path={oldFile}
            onBrowse={() => void pick(setOldFile)}
            onChoose={setOldFile}
          />
          <button
            className="cmp-swap"
            data-tip={t("tipCmpSwap")}
            aria-label={t("tipCmpSwap")}
            onClick={() => {
              setOldFile(newFile);
              setNewFile(oldFile);
            }}
          >
            <Icon d={ICON.swap} size={44} stroke={1.3} />
          </button>
          <FileSlot
            label={t("cmpNewFile")}
            path={newFile}
            onBrowse={() => void pick(setNewFile)}
            onChoose={setNewFile}
          />
        </div>
        <label className="cmp-textonly">
          <input type="checkbox" checked readOnly />
          <span>{t("cmpTextOnly")}</span>
        </label>
        <button
          className="cmp-pill cmp-run"
          disabled={!oldFile || !newFile}
          onClick={() => oldFile && newFile && startCompare(oldFile, newFile)}
        >
          {t("cmpRun")}
        </button>
      </div>
    </div>
  );
}
