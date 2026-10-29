import { useEffect, useRef, useState } from "react";
import { useStore, useActiveTab } from "../store";
import { getDoc, docPath, specBase, type DocEntry } from "../pdf";
import {
  movePage,
  insertBlankPage,
  duplicatePage,
  exportPageAsPng,
  rotateSelection,
  deleteSelection,
  extractSelected,
} from "../actions";
import { report } from "../errors";
import { useT } from "../i18n";
import { useOutsideClose } from "./ui/useOutsideClose";
import PageThumb from "./ui/PageThumb";
import { handleMenuKeys, isContextMenuKey, useFocusMenuOnOpen } from "./ui/menuKeys";
import { trackPointer } from "../pointer";
import BookmarkPanel from "./BookmarkPanel";

const THUMB_W = 132;
const BLANK = { w: 612, h: 792 };
type SideView = "pages" | "outline";

interface Menu {
  x: number;
  y: number;
  index: number;
  fromKeyboard: boolean;
}

export default function Sidebar() {
  const t = useT();
  const tab = useActiveTab();
  const [entry, setEntry] = useState<DocEntry | null>(null);
  const listRef = useRef<HTMLDivElement>(null);
  const drag = useRef<{ from: number; startY: number; active: boolean } | null>(null);
  const [insertAt, setInsertAt] = useState<number | null>(null);
  const suppressClick = useRef(false);
  const [menu, setMenu] = useState<Menu | null>(null);
  const [menuEl, setMenuEl] = useState<HTMLDivElement | null>(null);
  const [view, setView] = useState<SideView>("pages");
  const bmCount = tab?.bookmarks?.length ?? 0;

  const tabId = tab?.id;
  const docVersion = tab?.docVersion ?? 0;

  useEffect(() => {
    let on = true;
    setEntry(null);
    if (!tab) return;
    getDoc(tab.id, docPath(tab), tab.docVersion)
      .then((e) => on && setEntry(e))
      .catch((e) => report(t("stLoadFailed"), e));
    return () => {
      on = false;
    };
  }, [tabId, docVersion]);

  useEffect(() => {
    setMenu(null);
  }, [tabId]);

  // Closing a menu that was driven from the keyboard puts focus back on its thumbnail.
  const closeMenu = () => {
    const m = menu;
    setMenu(null);
    if (m && menuEl?.contains(document.activeElement)) thumbAt(m.index)?.focus();
  };

  useOutsideClose(!!menu, ".ctx-menu", closeMenu);
  useFocusMenuOnOpen(menuEl, !!menu?.fromKeyboard);

  if (!tab) return <div className="side-col empty" />;

  const thumbAt = (i: number) => listRef.current?.querySelectorAll<HTMLElement>(".thumb")[i] ?? null;

  const select = (i: number, mods: { metaKey: boolean; ctrlKey: boolean; shiftKey: boolean }) => {
    const st = useStore.getState();
    if (mods.metaKey || mods.ctrlKey) {
      const sel = tab.selected.includes(i) ? tab.selected.filter((x) => x !== i) : [...tab.selected, i];
      st.patchTab(tab.id, { selected: sel });
    } else if (mods.shiftKey && tab.selected.length) {
      const anchor = tab.selected[0];
      const [a, b] = anchor < i ? [anchor, i] : [i, anchor];
      st.patchTab(tab.id, {
        selected: Array.from({ length: b - a + 1 }, (_, k) => a + k),
      });
    } else {
      st.patchTab(tab.id, { selected: [i] });
      st.scrollTo(i + 1);
    }
  };

  const onClick = (i: number, e: React.MouseEvent) => {
    if (suppressClick.current) {
      suppressClick.current = false;
      return;
    }
    select(i, e);
  };

  const openMenuAt = (i: number, el: HTMLElement) => {
    const r = el.getBoundingClientRect();
    setMenu({ x: r.left + 12, y: r.top + 12, index: i, fromKeyboard: true });
  };

  const onThumbKey = (i: number, e: React.KeyboardEvent<HTMLDivElement>) => {
    if (e.target !== e.currentTarget) return;
    if (e.key === "Enter" || e.key === " ") {
      e.preventDefault();
      select(i, e);
    } else if (isContextMenuKey(e)) {
      e.preventDefault();
      openMenuAt(i, e.currentTarget);
    } else if (e.key === "ArrowDown" || e.key === "ArrowUp") {
      e.preventDefault();
      thumbAt(Math.max(0, Math.min(tab.pages.length - 1, i + (e.key === "ArrowDown" ? 1 : -1))))?.focus();
    } else if (e.key === "Home" || e.key === "End") {
      e.preventDefault();
      thumbAt(e.key === "Home" ? 0 : tab.pages.length - 1)?.focus();
    }
  };

  const computeInsert = (clientY: number): number => {
    const items = Array.from(listRef.current!.querySelectorAll<HTMLElement>(".thumb"));
    for (let i = 0; i < items.length; i++) {
      const r = items[i].getBoundingClientRect();
      if (clientY < r.top + r.height / 2) return i;
    }
    return items.length;
  };

  const onPointerDown = (i: number, e: React.PointerEvent) => {
    if (e.button !== 0) return;
    drag.current = { from: i, startY: e.clientY, active: false };
    const onMove = (ev: PointerEvent) => {
      const d = drag.current;
      if (!d) return;
      if (!d.active && Math.abs(ev.clientY - d.startY) > 6) d.active = true;
      if (d.active) setInsertAt(computeInsert(ev.clientY));
    };
    const onUp = (ev: PointerEvent) => {
      const d = drag.current;
      drag.current = null;
      setInsertAt(null);
      if (d?.active) {
        suppressClick.current = true;
        let to = computeInsert(ev.clientY);
        if (to > d.from) to -= 1;
        movePage(d.from, to);
      }
    };
    trackPointer(onMove, onUp, () => {
      drag.current = null;
      setInsertAt(null);
    });
  };

  const ensureSel = (i: number) => {
    if (!tab.selected.includes(i)) useStore.getState().patchTab(tab.id, { selected: [i] });
  };

  const item = (label: string, fn: () => void) => (
    <button
      role="menuitem"
      onClick={() => {
        closeMenu();
        fn();
      }}
    >
      {label}
    </button>
  );

  return (
    <div className="side-col">
      <div className="side-tabs" role="tablist">
        <button
          role="tab"
          aria-selected={view === "pages"}
          className={view === "pages" ? "on" : ""}
          onClick={() => setView("pages")}
        >
          {t("sbPages")}
        </button>
        <button
          role="tab"
          aria-selected={view === "outline"}
          className={view === "outline" ? "on" : ""}
          onClick={() => setView("outline")}
        >
          {t("sbOutline")}
          {bmCount > 0 && <span className="side-badge">{bmCount}</span>}
        </button>
      </div>

      {view === "outline" ? (
        <BookmarkPanel tab={tab} />
      ) : (
        <div
          className="sidebar"
          ref={listRef}
          role="listbox"
          aria-multiselectable="true"
          aria-label={t("sbPages")}
        >
          {entry &&
            tab.pages.map((spec, i) => (
              <div key={`${i}:${spec.src}:${spec.rotation}:${entry.version}`}>
                {insertAt === i && <div className="insert-line" />}
                <div
                  className={
                    "thumb" +
                    (tab.selected.includes(i) ? " selected" : "") +
                    (tab.currentPage === i + 1 ? " current" : "")
                  }
                  role="option"
                  tabIndex={0}
                  aria-selected={tab.selected.includes(i)}
                  aria-label={t("cmpPageN", { n: i + 1 })}
                  onClick={(e) => onClick(i, e)}
                  onDoubleClick={() => useStore.getState().scrollTo(i + 1)}
                  onPointerDown={(e) => onPointerDown(i, e)}
                  onKeyDown={(e) => onThumbKey(i, e)}
                  onContextMenu={(e) => {
                    e.preventDefault();
                    setMenu({ x: e.clientX, y: e.clientY, index: i, fromKeyboard: false });
                  }}
                >
                  <PageThumb
                    entry={entry}
                    src={spec.src}
                    base={specBase(entry, spec) ?? BLANK}
                    rotation={spec.rotation}
                    width={THUMB_W}
                  />
                  <span className="thumb-num">{i + 1}</span>
                </div>
              </div>
            ))}
          {insertAt === tab.pages.length && <div className="insert-line" />}
        </div>
      )}

      {menu && (
        <div
          className="ctx-menu"
          role="menu"
          ref={setMenuEl}
          onKeyDown={(e) => handleMenuKeys(e, menuEl)}
          style={{
            left: Math.min(menu.x, window.innerWidth - 200),
            top: Math.min(menu.y, window.innerHeight - 290),
          }}
        >
          {item(t("ctxDuplicate"), () => duplicatePage(tab.id, menu.index))}
          {item(t("ctxInsertBefore"), () => insertBlankPage(tab.id, menu.index))}
          {item(t("ctxInsertAfter"), () => insertBlankPage(tab.id, menu.index + 1))}
          {item(t("ctxExportPng"), () => exportPageAsPng(tab.id, menu.index))}
          <div className="ctx-sep" role="separator" />
          {item(t("ctxRotL"), () => {
            ensureSel(menu.index);
            rotateSelection(-90);
          })}
          {item(t("ctxRotR"), () => {
            ensureSel(menu.index);
            rotateSelection(90);
          })}
          <div className="ctx-sep" role="separator" />
          {item(t("ctxExtract"), () => {
            ensureSel(menu.index);
            extractSelected();
          })}
          {item(t("ctxDelete"), () => {
            ensureSel(menu.index);
            deleteSelection();
          })}
        </div>
      )}
    </div>
  );
}
