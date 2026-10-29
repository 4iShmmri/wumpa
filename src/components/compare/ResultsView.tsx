import { useEffect, useMemo, useRef, useState } from "react";
import { useStore, type CompareState } from "../../store";
import { getDoc, type DocEntry } from "../../pdf";
import { report } from "../../errors";
import { useT } from "../../i18n";
import Icon from "../ui/Icon";
import {
  ALL_KINDS,
  boxesFor,
  compareDocs,
  countKinds,
  isPageChange,
  onNewSide,
  onOldSide,
  passesFilter,
  wrapIndex,
  type Change,
  type KindFilter,
  type PageDiff,
  type Side,
  type View,
} from "../../compare";
import { ICON, pageTokens } from "./shared";
import Dropdown from "./Dropdown";
import TextCell, { type CellProps } from "./TextCell";
import OverlayCell from "./OverlayCell";
import ChangesPane from "./ChangesPane";

// The comparison result: toolbar, summary sheets, one row per page, the
// file chips below and the changes pane beside.
export default function ResultsView({ cmp }: { cmp: CompareState }) {
  const t = useT();
  const lang = useStore((s) => s.effectiveLang);
  const close = useStore((s) => s.closeCompare);
  const [left, setLeft] = useState<DocEntry | null>(null);
  const [right, setRight] = useState<DocEntry | null>(null);
  const [diffs, setDiffs] = useState<PageDiff[] | null>(null);
  const [view, setView] = useState<View>("side");
  const [showRows, setShowRows] = useState(true);
  const [showSummary, setShowSummary] = useState(true);
  const [showPane, setShowPane] = useState(true);
  const [filter, setFilter] = useState<KindFilter>(ALL_KINDS);
  const [cellW, setCellW] = useState(0);
  const [cur, setCur] = useState(-1);
  const [noteOpen, setNoteOpen] = useState(false);
  const [paneQuery, setPaneQuery] = useState("");
  const bodyRef = useRef<HTMLDivElement>(null);
  const rowRefs = useRef<(HTMLDivElement | null)[]>([]);
  const comparedAt = useMemo(() => new Date(), [cmp.leftKey]);

  useEffect(() => {
    let on = true;
    Promise.all([
      getDoc(cmp.leftKey, cmp.leftPath, cmp.leftVersion, true),
      getDoc(cmp.rightKey, cmp.rightPath, cmp.rightVersion, true),
    ])
      .then(([l, r]) => {
        if (!on) return;
        setLeft(l);
        setRight(r);
      })
      .catch((e) => on && report(t("stCmpFailed"), e));
    return () => {
      on = false;
    };
  }, [cmp.leftKey, cmp.rightKey]);

  const single = view === "old" || view === "new";

  useEffect(() => {
    const el = bodyRef.current;
    if (!el) return;
    const measure = () => {
      const pad = 80;
      const w = single
        ? Math.min(900, Math.max(240, el.clientWidth - pad))
        : Math.max(240, Math.floor((el.clientWidth - pad) / 2));
      setCellW((prev) => (Math.abs(prev - w) > 8 ? w : prev));
    };
    measure();
    const ro = new ResizeObserver(measure);
    ro.observe(el);
    return () => ro.disconnect();
  }, [left, single, showPane]);

  useEffect(() => {
    if (!left || !right) return;
    let on = true;
    setDiffs(null);
    setCur(-1);
    setNoteOpen(false);
    compareDocs(
      left.numPages,
      right.numPages,
      (side, page) => pageTokens(side === "old" ? left : right, page),
      () => on,
    )
      .then((out) => out && on && setDiffs(out))
      .catch((e) => on && report(t("stCmpFailed"), e));
    return () => {
      on = false;
    };
  }, [left, right]);

  const allChanges = useMemo(() => (diffs ? diffs.flatMap((d) => d.changes) : []), [diffs]);
  const all = useMemo(() => allChanges.filter((c) => passesFilter(c, filter)), [allChanges, filter]);
  const counts = useMemo(() => countKinds(allChanges), [allChanges]);
  const visibleChanges = (d: PageDiff | undefined) =>
    d ? d.changes.filter((c) => passesFilter(c, filter)) : [];
  const pos = (id: number) => all.findIndex((c) => c.id === id);

  const scrollToChange = (c: Change) => {
    const body = bodyRef.current;
    const row = rowRefs.current[c.page];
    if (!body || !row) return;
    const onNew = onNewSide(c);
    const entry = onNew ? right : left;
    const boxes = boxesFor(c, onNew ? "new" : "old");
    let y = row.offsetTop;
    if (entry && boxes.length && view !== "overlay") {
      const dims = entry.dims[c.page];
      if (dims) y += (dims.h - boxes[0][3]) * (cellW / dims.w);
    }
    body.scrollTo({ top: Math.max(0, y - body.clientHeight / 2 + 40), behavior: "smooth" });
  };

  const goTo = (idx: number, openNote = true) => {
    if (!all.length) return;
    const c = all[wrapIndex(idx, all.length)];
    if (view === "old" && onNewSide(c)) setView("side");
    if (view === "new" && onOldSide(c)) setView("side");
    setCur(c.id);
    setNoteOpen(openNote && !isPageChange(c));
    scrollToChange(c);
  };
  const jump = (dir: 1 | -1) => {
    const p = cur < 0 ? -1 : pos(cur);
    goTo(p < 0 ? (dir === 1 ? 0 : all.length - 1) : p + dir);
  };

  useEffect(() => {
    const onKey = (e: KeyboardEvent) => {
      if (document.querySelector(".cmp-overlay .tb-menu")) return;
      if (e.key === "Escape") {
        if (e.target instanceof HTMLInputElement) return;
        e.stopPropagation();
        e.preventDefault();
        if (noteOpen) setNoteOpen(false);
        else close();
      } else if ((e.metaKey || e.ctrlKey) && (e.key === "," || e.key === ".")) {
        e.stopPropagation();
        e.preventDefault();
        jump(e.key === "," ? 1 : -1);
      }
    };
    window.addEventListener("keydown", onKey, true);
    return () => window.removeEventListener("keydown", onKey, true);
  });

  const pages = Math.max(left?.numPages ?? 0, right?.numPages ?? 0);
  const ready = !!left && !!right;
  const firstPage = all.length ? all[0].page + 1 : 0;
  const curPos = cur >= 0 ? pos(cur) : -1;

  const viewLabel =
    view === "side"
      ? t("cmpViewSideBySide")
      : view === "old"
        ? t("cmpOldFile")
        : view === "new"
          ? t("cmpNewFile")
          : t("cmpModeOverlay");

  const cellProps = (side: Side, i: number, d: PageDiff | undefined, w: number): CellProps => ({
    entry: side === "old" ? left : right,
    src: i + 1,
    present: i < ((side === "old" ? left : right)?.numPages ?? 0),
    cellW: w,
    side,
    changes: visibleChanges(d),
    pageStatus: d?.status,
    current: cur,
    noteOpen,
    rows: showRows,
    total: all.length,
    onPick: (id: number) => {
      setCur(id);
      setNoteOpen(true);
    },
    onPrev: () => jump(-1),
    onNext: () => jump(1),
    onCloseNote: () => setNoteOpen(false),
  });

  const rowWidth = single ? cellW : cellW * 2 + 24;

  return (
    <div className="cmp-overlay" onClick={() => noteOpen && setNoteOpen(false)}>
      <div className="cmp-head">
        <div className="cmp-head-title">{t("cmpDlgTitle")}</div>

        <div className="cmp-tools">
          <Dropdown
            icon={ICON.sideBySide}
            label={viewLabel}
            tip={t("tipCmpView")}
            items={[
              { label: t("cmpViewSideBySide"), checked: view === "side", run: () => setView("side") },
              { label: t("cmpOldFile"), checked: view === "old", run: () => setView("old") },
              { label: t("cmpNewFile"), checked: view === "new", run: () => setView("new") },
              { label: t("cmpModeOverlay"), checked: view === "overlay", run: () => setView("overlay") },
            ]}
          />
          <span className="cmp-tb-sep" />
          <button
            className="cmp-tb-btn"
            disabled={!all.length || curPos <= 0}
            data-tip={t("tipCmpPrev")}
            onClick={() => jump(-1)}
          >
            <Icon d={ICON.up} size={15} />
            <span>{t("cmpPrev")}</span>
          </button>
          <button
            className="cmp-tb-btn"
            disabled={!all.length || curPos >= all.length - 1}
            data-tip={t("tipCmpNext")}
            onClick={() => jump(1)}
          >
            <Icon d={ICON.down} size={15} />
            <span>{t("cmpNext")}</span>
          </button>
          <span className="cmp-tb-sep" />
          <Dropdown
            icon={ICON.filter}
            label={t("cmpFilter")}
            check
            items={[
              {
                label: t("cmpReplacements"),
                checked: filter.replaced,
                run: () => setFilter((f) => ({ ...f, replaced: !f.replaced })),
              },
              {
                label: t("cmpInsertions"),
                checked: filter.inserted,
                run: () => setFilter((f) => ({ ...f, inserted: !f.inserted })),
              },
              {
                label: t("cmpDeletions"),
                checked: filter.deleted,
                run: () => setFilter((f) => ({ ...f, deleted: !f.deleted })),
              },
              {
                label: t("cmpPagesCol"),
                checked: filter.pages,
                run: () => setFilter((f) => ({ ...f, pages: !f.pages })),
              },
            ]}
          />
          <Dropdown
            icon={ICON.more}
            label={t("cmpShow")}
            check
            items={[
              { label: t("cmpShowRows"), checked: showRows, run: () => setShowRows((v) => !v) },
              { label: t("cmpShowSummary"), checked: showSummary, run: () => setShowSummary((v) => !v) },
              { label: t("cmpShowChanges"), checked: showPane, run: () => setShowPane((v) => !v) },
            ]}
          />
          <span className="cmp-count" aria-live="polite">
            {!ready || !diffs ? t("cmpComparing") : null}
          </span>
        </div>

        <button className="cmp-pill" data-tip={t("tipCmpClose")} onClick={() => close()}>
          {t("cmpDone")}
        </button>
      </div>

      <div className="cmp-main">
        <div className="cmp-body" ref={bodyRef}>
          {ready && diffs && showSummary && view !== "overlay" && (
            <div className="cmp-row cmp-summary-row" style={{ width: rowWidth }}>
              <div className="cmp-sheet cmp-overview" style={{ width: cellW }}>
                <h2>{t("cmpOverview")}</h2>
                <div className="cmp-ring">
                  <span className="cmp-ring-n">{allChanges.length}</span>
                  <span className="cmp-ring-k">{t("cmpTotal")}</span>
                </div>
                <div className="cmp-ov-legend">
                  <span>
                    <i className="cmp-lg replaced" />
                    {t("cmpShowsReplacements")}
                  </span>
                  <span>
                    <i className="cmp-lg inserted">
                      <b>+</b>
                    </i>
                    {t("cmpShowsInsertions")}
                  </span>
                  <span>
                    <i className="cmp-lg deleted">
                      <b>×</b>
                    </i>
                    {t("cmpShowsDeletions")}
                  </span>
                </div>
              </div>
              {!single && (
                <div className="cmp-sheet cmp-summary" style={{ width: cellW }}>
                  <div className="cmp-sum-date">{comparedAt.toLocaleString(lang === "ar" ? "ar" : "en")}</div>
                  <h2>{t("cmpResults")}</h2>
                  <div className="cmp-sum-files">
                    <div className="cmp-sum-file">
                      <span className="cmp-sum-k">{t("cmpOldFile")}:</span>
                      <span className="cmp-sum-n" title={cmp.leftPath}>
                        {cmp.leftName}
                      </span>
                      <span className="cmp-sum-p">{t("cmpPagesN", { n: left!.numPages })}</span>
                    </div>
                    <span className="cmp-sum-vs">{t("cmpVersus")}</span>
                    <div className="cmp-sum-file">
                      <span className="cmp-sum-k">{t("cmpNewFile")}:</span>
                      <span className="cmp-sum-n" title={cmp.rightPath}>
                        {cmp.rightName}
                      </span>
                      <span className="cmp-sum-p">{t("cmpPagesN", { n: right!.numPages })}</span>
                    </div>
                  </div>
                  <div className="cmp-sum-grid">
                    <div className="cmp-sum-col">
                      <div className="cmp-sum-h">{t("cmpTotal")}</div>
                      <div className="cmp-sum-big">{allChanges.length}</div>
                      <div className="cmp-sum-p">{t("cmpTextOnlyCmp")}</div>
                    </div>
                    <div className="cmp-sum-col">
                      <div className="cmp-sum-h">{t("cmpContent")}</div>
                      <div className="cmp-sum-row">
                        <b>{counts.replaced}</b>
                        {t("cmpReplacements")}
                      </div>
                      <div className="cmp-sum-row">
                        <b>{counts.inserted}</b>
                        {t("cmpInsertions")}
                      </div>
                      <div className="cmp-sum-row">
                        <b>{counts.deleted}</b>
                        {t("cmpDeletions")}
                      </div>
                    </div>
                    <div className="cmp-sum-col">
                      <div className="cmp-sum-h">{t("cmpPagesCol")}</div>
                      <div className="cmp-sum-row">
                        <b>{counts.pageInserted}</b>
                        {t("cmpPagesInserted")}
                      </div>
                      <div className="cmp-sum-row">
                        <b>{counts.pageDeleted}</b>
                        {t("cmpPagesDeleted")}
                      </div>
                    </div>
                  </div>
                  {all.length > 0 ? (
                    <button className="cmp-pill cmp-sum-go" onClick={() => goTo(0)}>
                      {t("cmpGoFirst", { n: firstPage })}
                    </button>
                  ) : (
                    <div className="cmp-sum-none">{t("cmpNoDiff")}</div>
                  )}
                </div>
              )}
            </div>
          )}

          {ready &&
            cellW > 0 &&
            Array.from({ length: pages }).map((_, i) => {
              const d = diffs?.[i];
              return (
                <div
                  className={"cmp-row" + (d && d.status !== "same" ? " diff" : "")}
                  key={i}
                  ref={(el) => {
                    rowRefs.current[i] = el;
                  }}
                >
                  {view === "side" && (
                    <>
                      <TextCell {...cellProps("old", i, d, cellW)} />
                      <TextCell {...cellProps("new", i, d, cellW)} />
                    </>
                  )}
                  {view === "old" && <TextCell {...cellProps("old", i, d, cellW)} />}
                  {view === "new" && <TextCell {...cellProps("new", i, d, cellW)} />}
                  {view === "overlay" && (
                    <OverlayCell left={left!} right={right!} src={i + 1} cellW={cellW * 2} />
                  )}
                </div>
              );
            })}

          {ready && cellW > 0 && view !== "overlay" && (
            <div className="cmp-chips" style={{ width: rowWidth }}>
              {view !== "new" && (
                <span className="cmp-chip" style={{ width: cellW }} title={cmp.leftPath}>
                  <b>[{t("cmpOldFile")}]</b> {cmp.leftName}
                </span>
              )}
              {view !== "old" && (
                <span className="cmp-chip" style={{ width: cellW }} title={cmp.rightPath}>
                  <b>[{t("cmpNewFile")}]</b> {cmp.rightName}
                </span>
              )}
            </div>
          )}
          {ready && view === "overlay" && (
            <div className="cmp-chips" style={{ width: rowWidth }}>
              <span className="cmp-chip">
                <i className="cmp-lg ov-old" />
                {t("cmpLegendRemoved")}
              </span>
              <span className="cmp-chip">
                <i className="cmp-lg ov-new" />
                {t("cmpLegendAdded")}
              </span>
            </div>
          )}
        </div>

        {showPane && (
          <ChangesPane
            changes={all}
            current={cur}
            query={paneQuery}
            showEmpty={ready && !!diffs}
            onQuery={setPaneQuery}
            onPick={(c) => goTo(pos(c.id))}
            onClose={() => setShowPane(false)}
          />
        )}
      </div>
    </div>
  );
}
