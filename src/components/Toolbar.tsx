import { useEffect, useState } from "react";
import { useStore, useActiveTab } from "../store";
import type { ShapeKind } from "../types";
import {
  openFileDialog,
  saveActive,
  mergeFiles,
  extractSelected,
  rotateSelection,
  deleteSelection,
  highlightSelection,
  printActive,
  insertImageFromFile,
  compareActive,
} from "../actions";
import { useT } from "../i18n";
import Icon from "./ui/Icon";
import { readLocal } from "../storage";
import { useOutsideClose } from "./ui/useOutsideClose";
import { useMenuButton } from "./ui/menuKeys";
import type { LangPref } from "../i18n";

const DRAW_KINDS: ShapeKind[] = ["ink", "rect", "ellipse", "line", "arrow"];
const DRAW_COLORS: [number, number, number][] = [
  [0.86, 0.21, 0.18],
  [0.1, 0.1, 0.1],
  [0.11, 0.3, 0.85],
  [0.09, 0.55, 0.25],
  [0.95, 0.6, 0.05],
  [0.55, 0.15, 0.6],
];
const DRAW_COLOR_NAMES = [
  "colorRed",
  "colorBlack",
  "colorBlue",
  "colorGreen",
  "colorOrange",
  "colorPurple",
] as const;
const DRAW_WIDTHS = [1.5, 2.5, 4.5];
const DRAW_WIDTH_NAMES = ["widthThin", "widthMedium", "widthThick"] as const;
const cssColor = (c: [number, number, number]) => `rgb(${c.map((v) => Math.round(v * 255)).join(",")})`;

function KindIcon({ k }: { k: ShapeKind }) {
  const s = {
    fill: "none",
    stroke: "currentColor",
    strokeWidth: 1.8,
    strokeLinecap: "round" as const,
    strokeLinejoin: "round" as const,
  };
  return (
    <svg width="15" height="15" viewBox="0 0 24 24">
      {k === "ink" && <path {...s} d="M3 17c3-7 5-8 6.5-5s1.5 6 3.5 6 3-8 5-6 1 5 3 5" />}
      {k === "rect" && <rect {...s} x="4" y="6" width="16" height="12" rx="1" />}
      {k === "ellipse" && <ellipse {...s} cx="12" cy="12" rx="8" ry="6" />}
      {k === "line" && <path {...s} d="M4 20L20 4" />}
      {k === "arrow" && <path {...s} d="M4 20L18 6 M11 6h7v7" />}
    </svg>
  );
}

function DrawPopover() {
  const t = useT();
  const drawKind = useStore((s) => s.drawKind);
  const drawColor = useStore((s) => s.drawColor);
  const drawWidth = useStore((s) => s.drawWidth);
  const st = useStore.getState;
  return (
    <div className="draw-popover" onPointerDown={(e) => e.stopPropagation()}>
      {DRAW_KINDS.map((k) => (
        <button
          key={k}
          className={"dk" + (drawKind === k ? " on" : "")}
          data-tip={t(
            ("k" + k[0].toUpperCase() + k.slice(1)) as "kInk" | "kRect" | "kEllipse" | "kLine" | "kArrow",
          )}
          onClick={() => st().setDrawKind(k)}
        >
          <KindIcon k={k} />
        </button>
      ))}
      <div className="psep" />
      {DRAW_COLORS.map((c, ci) => (
        <button
          key={c.join()}
          className={"cdot" + (drawColor.join() === c.join() ? " on" : "")}
          style={{ background: cssColor(c) }}
          data-tip={t("tipDrawColor", { c: t(DRAW_COLOR_NAMES[ci]) })}
          onClick={() => st().setDrawColor(c)}
        />
      ))}
      <div className="psep" />
      {DRAW_WIDTHS.map((w, i) => (
        <button
          key={w}
          className={"wdot" + (drawWidth === w ? " on" : "")}
          data-tip={t("tipWidth", { w: t(DRAW_WIDTH_NAMES[i]) })}
          onClick={() => st().setDrawWidth(w)}
        >
          <i style={{ width: 4 + i * 3, height: 4 + i * 3 }} />
        </button>
      ))}
    </div>
  );
}

const icons = {
  open: "M22 19a2 2 0 0 1-2 2H4a2 2 0 0 1-2-2V5a2 2 0 0 1 2-2h5l2 3h9a2 2 0 0 1 2 2z",
  save: "M19 21H5a2 2 0 0 1-2-2V5a2 2 0 0 1 2-2h11l5 5v11a2 2 0 0 1-2 2z M17 21v-8H7v8 M7 3v5h8",
  print:
    "M6 9V2h12v7 M6 18H4a2 2 0 0 1-2-2v-5a2 2 0 0 1 2-2h16a2 2 0 0 1 2 2v5a2 2 0 0 1-2 2h-2 M6 14h12v8H6z",
  undo: "M3 7v6h6 M21 17a9 9 0 0 0-15-6.7L3 13",
  redo: "M21 7v6h-6 M3 17a9 9 0 0 1 15-6.7L21 13",
  zoomOut: "M11 19a8 8 0 1 0 0-16 8 8 0 0 0 0 16z M21 21l-4.35-4.35 M8 11h6",
  zoomIn: "M11 19a8 8 0 1 0 0-16 8 8 0 0 0 0 16z M21 21l-4.35-4.35 M11 8v6 M8 11h6",
  fit: "M8 3H5a2 2 0 0 0-2 2v3 M16 3h3a2 2 0 0 1 2 2v3 M8 21H5a2 2 0 0 1-2-2v-3 M16 21h3a2 2 0 0 0 2-2v-3",
  rotL: "M2.5 2v6h6 M2.66 15.57a10 10 0 1 0 .57-8.38",
  rotR: "M21.5 2v6h-6 M21.34 15.57a10 10 0 1 1-.57-8.38",
  del: "M3 6h18 M19 6v14a2 2 0 0 1-2 2H7a2 2 0 0 1-2-2V6 M8 6V4a2 2 0 0 1 2-2h4a2 2 0 0 1 2 2v2 M10 11v6 M14 11v6",
  extract: "M14 2H6a2 2 0 0 0-2 2v16a2 2 0 0 0 2 2h12a2 2 0 0 0 2-2V8z M14 2v6h6 M12 18v-6 M9 15l3 3 3-3",
  merge:
    "M8 3H5a2 2 0 0 0-2 2v14a2 2 0 0 0 2 2h3 M16 3h3a2 2 0 0 1 2 2v14a2 2 0 0 1-2 2h-3 M12 8v8 M9 11l3-3 3 3",
  split: "M16 3h3a2 2 0 0 1 2 2v14a2 2 0 0 1-2 2h-3 M8 3H5a2 2 0 0 0-2 2v14a2 2 0 0 0 2 2h3 M12 4v16",
  highlight:
    "M9 11l-6 6v3h3l6-6 M14 6l4 4 3.5-3.5a2.1 2.1 0 0 0 0-3L20 2a2.1 2.1 0 0 0-3 0z M9 11l4-4 4 4-4 4z",
  note: "M21 15a2 2 0 0 1-2 2H7l-4 4V5a2 2 0 0 1 2-2h14a2 2 0 0 1 2 2z",
  editObj: "M3 3l7.07 16.97 2.51-7.39 7.39-2.51L3 3z",
  erase:
    "M9 3H5a2 2 0 0 0-2 2v4 M15 3h4a2 2 0 0 1 2 2v4 M9 21H5a2 2 0 0 1-2-2v-4 M15 21h4a2 2 0 0 0 2-2v-4 M8 12h8",
  text: "M4 7V5h16v2 M12 5v14 M9 19h6",
  sign: "M3 17c3-6 5-8 6-6s-1 5 1 5 3-7 5-5-1 6 2 6h4",
  draw: "M12 19l7-7 3 3-7 7-3-3z M18 13l-1.5-7.5L2 2l3.5 14.5L13 18l5-5z",
  link: "M10 13a5 5 0 0 0 7.54.54l3-3a5 5 0 0 0-7.07-7.07l-1.72 1.71 M14 11a5 5 0 0 0-7.54-.54l-3 3a5 5 0 0 0 7.07 7.07l1.71-1.71",
  search: "M11 19a8 8 0 1 0 0-16 8 8 0 0 0 0 16z M21 21l-4.35-4.35",
  sun: "M12 17a5 5 0 1 0 0-10 5 5 0 0 0 0 10z M12 1v2 M12 21v2 M4.22 4.22l1.42 1.42 M18.36 18.36l1.42 1.42 M1 12h2 M21 12h2 M4.22 19.78l1.42-1.42 M18.36 5.64l1.42-1.42",
  moon: "M21 12.79A9 9 0 1 1 11.21 3 7 7 0 0 0 21 12.79z",
  newDoc: "M14 2H6a2 2 0 0 0-2 2v16a2 2 0 0 0 2 2h12a2 2 0 0 0 2-2V8z M14 2v6h6 M12 11v6 M9 14h6",
  scan: "M3 7V5a2 2 0 0 1 2-2h2 M17 3h2a2 2 0 0 1 2 2v2 M21 17v2a2 2 0 0 1-2 2h-2 M7 21H5a2 2 0 0 1-2-2v-2 M7 12h10 M7 8h6 M7 16h8",
  word: "M14 2H6a2 2 0 0 0-2 2v16a2 2 0 0 0 2 2h12a2 2 0 0 0 2-2V8z M14 2v6h6 M8 13l1.5 6 2.5-5 2.5 5 1.5-6",
  sheet: "M14 2H6a2 2 0 0 0-2 2v16a2 2 0 0 0 2 2h12a2 2 0 0 0 2-2V8z M14 2v6h6 M8 12h8 M8 16h8 M12 12v8",
  compress:
    "M8 3v4a1 1 0 0 1-1 1H3 M16 3v4a1 1 0 0 0 1 1h4 M8 21v-4a1 1 0 0 0-1-1H3 M16 21v-4a1 1 0 0 1 1-1h4",
  qr: "M3 3h6v6H3z M15 3h6v6h-6z M3 15h6v6H3z M15 15h3v3h-3z M21 21h-3 M21 15v3",
  image:
    "M19 3H5a2 2 0 0 0-2 2v14a2 2 0 0 0 2 2h14a2 2 0 0 0 2-2V5a2 2 0 0 0-2-2z M8.5 10a1.5 1.5 0 1 0 0-3 1.5 1.5 0 0 0 0 3z M21 15l-5-5L5 21",
  table:
    "M3 5a2 2 0 0 1 2-2h14a2 2 0 0 1 2 2v14a2 2 0 0 1-2 2H5a2 2 0 0 1-2-2z M3 10h18 M3 15h18 M9 3v18 M15 3v18",
  insertMenu: "M3 5a2 2 0 0 1 2-2h14a2 2 0 0 1 2 2v14a2 2 0 0 1-2 2H5a2 2 0 0 1-2-2z M12 8v8 M8 12h8",
  organize: "M3 3h8v8H3z M13 3h8v8h-8z M3 13h8v8H3z M13 13h8v8h-8z",
  compare: "M12 3v18 M5 6H3v11a2 2 0 0 0 2 2h4 M5 6h4V4 M19 18h2V7a2 2 0 0 0-2-2h-4 M19 18h-4v2",
  shield: "M12 22s8-4 8-10V5l-8-3-8 3v7c0 6 8 10 8 10z M9 12l2 2 4-4",
  lock: "M5 11h14a2 2 0 0 1 2 2v7a2 2 0 0 1-2 2H5a2 2 0 0 1-2-2v-7a2 2 0 0 1 2-2z M8 11V7a4 4 0 0 1 8 0v4",
  unlock: "M5 11h14a2 2 0 0 1 2 2v7a2 2 0 0 1-2 2H5a2 2 0 0 1-2-2v-7a2 2 0 0 1 2-2z M8 11V7a4 4 0 0 1 7.5-2",
};

type MenuItem =
  { sep: true } | { header: string } | { icon: string; label: string; disabled?: boolean; run: () => void };

function ToolbarMenu({ icon, label, items }: { icon: string; label: string; items: MenuItem[] }) {
  const { open, setOpen, wrapRef, triggerRef, triggerProps, menuProps } = useMenuButton<HTMLSpanElement>();

  return (
    <span className="tb-menu-wrap" ref={wrapRef}>
      <button
        ref={triggerRef}
        className={"txt" + (open ? " active" : "")}
        data-tip={open ? undefined : label}
        aria-label={label}
        {...triggerProps}
      >
        <Icon d={icon} size={16} />
        <svg
          className="tb-caret"
          width="9"
          height="9"
          viewBox="0 0 24 24"
          fill="none"
          stroke="currentColor"
          strokeWidth="2.6"
          strokeLinecap="round"
          strokeLinejoin="round"
          aria-hidden="true"
        >
          <path d="M6 9l6 6 6-6" />
        </svg>
      </button>
      {open && (
        <div className="tb-menu" aria-label={label} {...menuProps}>
          {items.map((it, i) =>
            "sep" in it ? (
              <div key={i} className="ctx-sep" role="separator" />
            ) : "header" in it ? (
              <div key={i} className="tb-menu-head">
                {it.header}
              </div>
            ) : (
              <button
                key={i}
                role="menuitem"
                disabled={it.disabled}
                onClick={() => {
                  setOpen(false);
                  it.run();
                }}
              >
                <Icon d={it.icon} size={16} />
                <span>{it.label}</span>
              </button>
            ),
          )}
        </div>
      )}
    </span>
  );
}

export default function Toolbar() {
  const t = useT();
  const tab = useActiveTab();
  const theme = useStore((s) => s.theme);
  const lang = useStore((s) => s.lang);
  const effectiveLang = useStore((s) => s.effectiveLang);
  const searchOpen = useStore((s) => s.search.open);
  const tool = useStore((s) => s.tool);
  const st = useStore.getState;
  const has = !!tab;
  const [drawOpen, setDrawOpen] = useState(false);

  useEffect(() => {
    if (tool !== "draw") setDrawOpen(false);
  }, [tool]);

  useOutsideClose(drawOpen, ".draw-wrap", () => setDrawOpen(false));

  const setZoom = (z: number) => {
    if (!tab) return;
    st().patchTab(tab.id, { zoom: Math.max(0.25, Math.min(6, Math.round(z * 100) / 100)) });
  };

  const toggleTool = (k: typeof tool) => st().setTool(tool === k ? "select" : k);

  const insertItems: MenuItem[] = [
    { icon: icons.image, label: t("tipInsertImage"), disabled: !has, run: () => void insertImageFromFile() },
    { icon: icons.table, label: t("tipInsertTable"), disabled: !has, run: () => st().setUtilModal("table") },
  ];

  const protectItems: MenuItem[] = [
    { icon: icons.lock, label: t("mProtect"), disabled: !has, run: () => st().setUtilModal("encrypt") },
    { icon: icons.unlock, label: t("mUnprotect"), disabled: !has, run: () => st().setUtilModal("decrypt") },
  ];
  const organizeItems: MenuItem[] = [
    { header: t("mhPages") },
    { icon: icons.rotL, label: t("tipRotL"), disabled: !has, run: () => rotateSelection(-90) },
    { icon: icons.rotR, label: t("tipRotR"), disabled: !has, run: () => rotateSelection(90) },
    { icon: icons.del, label: t("tipDelPages"), disabled: !has, run: () => deleteSelection() },
    { icon: icons.extract, label: t("ctxExtract"), disabled: !has, run: () => void extractSelected() },
    { sep: true },
    { header: t("mhDocument") },
    { icon: icons.merge, label: t("wMerge"), run: () => void mergeFiles() },
    { icon: icons.split, label: t("miSplit"), disabled: !has, run: () => st().setUtilModal("split") },
    { icon: icons.scan, label: t("miScan"), disabled: !has, run: () => st().setUtilModal("scan") },
    {
      icon: icons.word,
      label: t("miExportWord"),
      disabled: !has,
      run: () => st().setUtilModal("exportDocx"),
    },
    {
      icon: icons.sheet,
      label: t("miExportExcel"),
      disabled: !has,
      run: () => st().setUtilModal("exportXlsx"),
    },
    {
      icon: icons.compress,
      label: t("tipCompress"),
      disabled: !has,
      run: () => st().setUtilModal("compress"),
    },
    { sep: true },
    { header: t("mhReview") },
    { icon: icons.compare, label: t("cmpMenu"), run: () => compareActive() },
  ];

  return (
    <div className="toolbar">
      <button
        data-tip={t("tipNewPdf")}
        aria-label={t("tipNewPdf")}
        onClick={() => st().setUtilModal("newpdf")}
      >
        <Icon d={icons.newDoc} />
      </button>
      <button data-tip={t("tipOpen")} aria-label={t("tipOpen")} onClick={() => openFileDialog()}>
        <Icon d={icons.open} />
      </button>
      <button
        data-tip={t("tipSave")}
        aria-label={t("tipSave")}
        disabled={!has}
        onClick={() => saveActive(false)}
      >
        <Icon d={icons.save} />
      </button>
      <button
        className="txt"
        data-tip={t("tipSaveAs")}
        aria-label={t("tipSaveAs")}
        disabled={!has}
        onClick={() => saveActive(true)}
      >
        {t("btnSaveAs")}
      </button>
      <button
        data-tip={t("tipPrint")}
        aria-label={t("tipPrint")}
        disabled={!has}
        onClick={() => printActive()}
      >
        <Icon d={icons.print} />
      </button>
      <div className="sep" />
      <button
        data-tip={t("tipUndo")}
        aria-label={t("tipUndo")}
        disabled={!has || !tab!.past.length}
        onClick={() => tab && st().undo(tab.id)}
      >
        <Icon d={icons.undo} />
      </button>
      <button
        data-tip={t("tipRedo")}
        aria-label={t("tipRedo")}
        disabled={!has || !tab!.future.length}
        onClick={() => tab && st().redo(tab.id)}
      >
        <Icon d={icons.redo} />
      </button>
      <div className="sep" />
      <button
        data-tip={t("tipZoomOut")}
        aria-label={t("tipZoomOut")}
        disabled={!has}
        onClick={() => tab && setZoom(tab.zoom / 1.2)}
      >
        <Icon d={icons.zoomOut} />
      </button>
      <span className="zoom-label">{tab ? Math.round(tab.zoom * 100) + "%" : "—"}</span>
      <button
        data-tip={t("tipZoomIn")}
        aria-label={t("tipZoomIn")}
        disabled={!has}
        onClick={() => tab && setZoom(tab.zoom * 1.2)}
      >
        <Icon d={icons.zoomIn} />
      </button>
      <button data-tip={t("tipFit")} aria-label={t("tipFit")} disabled={!has} onClick={() => st().fitWidth()}>
        <Icon d={icons.fit} />
      </button>
      <div className="sep" />
      <button
        data-tip={t("tipHighlight")}
        aria-label={t("tipHighlight")}
        disabled={!has}
        onClick={() => highlightSelection()}
      >
        <Icon d={icons.highlight} />
      </button>
      <button
        data-tip={t("tipNote")}
        aria-label={t("tipNote")}
        disabled={!has}
        className={tool === "note" ? "active" : ""}
        onClick={() => toggleTool("note")}
      >
        <Icon d={icons.note} />
      </button>
      <span className="draw-wrap">
        <button
          data-tip={t("tipDraw")}
          aria-label={t("tipDraw")}
          disabled={!has}
          className={tool === "draw" ? "active" : ""}
          onClick={() => {
            if (tool !== "draw") {
              st().setTool("draw");
              setDrawOpen(true);
            } else if (drawOpen) {
              setDrawOpen(false);
            } else {
              st().setTool("select");
            }
          }}
        >
          <Icon d={icons.draw} />
        </button>
        {drawOpen && <DrawPopover />}
      </span>
      <button
        data-tip={t("tipText")}
        aria-label={t("tipText")}
        disabled={!has}
        className={tool === "text" ? "active" : ""}
        onClick={() => toggleTool("text")}
      >
        <Icon d={icons.text} />
      </button>
      <button
        data-tip={t("tipSign")}
        aria-label={t("tipSign")}
        disabled={!has}
        className={tool === "sign" ? "active" : ""}
        onClick={() => {
          if (tool === "sign") {
            st().setTool("select");
            return;
          }
          const sig = readLocal("signature");
          if (sig) st().setTool("sign");
          else st().setSignatureModal(true);
        }}
      >
        <Icon d={icons.sign} />
      </button>
      <button
        data-tip={t("tipLink")}
        aria-label={t("tipLink")}
        disabled={!has}
        className={tool === "link" ? "active" : ""}
        onClick={() => toggleTool("link")}
      >
        <Icon d={icons.link} />
      </button>
      <button
        data-tip={t("tipEdit")}
        aria-label={t("tipEdit")}
        disabled={!has}
        className={tool === "edit" ? "active" : ""}
        onClick={() => toggleTool("edit")}
      >
        <Icon d={icons.editObj} />
      </button>
      <button
        data-tip={t("tipErase")}
        aria-label={t("tipErase")}
        disabled={!has}
        className={tool === "erase" ? "active" : ""}
        onClick={() => toggleTool("erase")}
      >
        <Icon d={icons.erase} />
      </button>
      <div className="sep" />
      <ToolbarMenu icon={icons.insertMenu} label={t("menuInsert")} items={insertItems} />
      <ToolbarMenu icon={icons.organize} label={t("menuOrganize")} items={organizeItems} />
      <ToolbarMenu icon={icons.shield} label={t("menuProtect")} items={protectItems} />
      <div className="spacer" />
      <button data-tip={t("tipQr")} aria-label={t("tipQr")} onClick={() => st().setUtilModal("qr")}>
        <Icon d={icons.qr} />
      </button>
      <button
        data-tip={t("tipSearch")}
        aria-label={t("tipSearch")}
        disabled={!has}
        className={searchOpen ? "active" : ""}
        onClick={() => st().setSearch({ open: !searchOpen })}
      >
        <Icon d={icons.search} />
      </button>
      <button
        className="txt"
        data-tip={t("tipLang", {
          lang: lang === "auto" ? t("langAuto") : lang === "ar" ? t("langAr") : t("langEn"),
        })}
        aria-label={t("tipLang", {
          lang: lang === "auto" ? t("langAuto") : lang === "ar" ? t("langAr") : t("langEn"),
        })}
        onClick={() => {
          const next: LangPref = lang === "en" ? "ar" : lang === "ar" ? "auto" : "en";
          st().setLang(next);
        }}
      >
        {lang === "auto" ? `A·${effectiveLang === "ar" ? "ع" : "EN"}` : lang === "en" ? "ع" : "EN"}
      </button>
      <button
        data-tip={t("tipTheme")}
        aria-label={t("tipTheme")}
        onClick={() => st().setTheme(theme === "dark" ? "light" : "dark")}
      >
        <Icon d={theme === "dark" ? icons.sun : icons.moon} />
      </button>
    </div>
  );
}
