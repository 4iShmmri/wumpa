import { useEffect, useState } from "react";
import type { Bookmark, Tab } from "../types";
import { useT } from "../i18n";
import { useOutsideClose } from "./ui/useOutsideClose";
import { handleMenuKeys, isContextMenuKey, isKeyboardClick, useFocusMenuOnOpen } from "./ui/menuKeys";
import {
  addBookmarkHere,
  renameBookmark,
  retargetBookmark,
  deleteBookmark,
  toggleBookmarkOpen,
  moveBookmark,
  indentBookmark,
  outdentBookmark,
  gotoBookmark,
} from "../actions";

interface Target {
  bm: Bookmark;
  index: number;
  count: number;
  isRoot: boolean;
}

interface Menu extends Target {
  x: number;
  y: number;
  fromKeyboard: boolean;
  returnTo: HTMLElement | null;
}

function Row({
  bm,
  depth,
  isRoot,
  index,
  count,
  editing,
  menuId,
  setEditing,
  openMenu,
}: {
  bm: Bookmark;
  depth: number;
  isRoot: boolean;
  index: number;
  count: number;
  editing: string | null;
  menuId: string | null;
  setEditing: (id: string | null) => void;
  openMenu: (e: React.MouseEvent | React.KeyboardEvent, tg: Target) => void;
}) {
  const t = useT();
  const [draft, setDraft] = useState(bm.title);
  const hasKids = bm.children.length > 0;
  const me: Target = { bm, index, count, isRoot };

  useEffect(() => {
    if (editing === bm.id) setDraft(bm.title);
  }, [editing]);

  const commit = () => {
    const v = draft.trim();
    if (v && v !== bm.title) renameBookmark(bm.id, v);
    setEditing(null);
  };

  return (
    <>
      <div
        className={"bm-row" + (menuId === bm.id ? " menu-open" : "")}
        style={{ paddingInlineStart: 6 + depth * 14 }}
        onContextMenu={(e) => {
          e.preventDefault();
          openMenu(e, me);
        }}
        onKeyDown={(e) => {
          if (isContextMenuKey(e)) {
            e.preventDefault();
            e.stopPropagation();
            openMenu(e, me);
          }
        }}
      >
        <button
          className={"bm-twist" + (hasKids ? "" : " hidden")}
          onClick={() => toggleBookmarkOpen(bm.id)}
          tabIndex={hasKids ? 0 : -1}
          aria-expanded={hasKids ? bm.open : undefined}
          aria-hidden={hasKids ? undefined : true}
        >
          <svg
            width="10"
            height="10"
            viewBox="0 0 24 24"
            fill="none"
            stroke="currentColor"
            strokeWidth="3"
            strokeLinecap="round"
            strokeLinejoin="round"
            aria-hidden="true"
            style={{ transform: bm.open ? "rotate(90deg)" : "none" }}
          >
            <path d="M8 5l8 7-8 7" />
          </svg>
        </button>

        {editing === bm.id ? (
          <input
            className="bm-edit"
            autoFocus
            value={draft}
            onChange={(e) => setDraft(e.target.value)}
            onBlur={commit}
            onKeyDown={(e) => {
              if (e.key === "Enter") commit();
              if (e.key === "Escape") setEditing(null);
            }}
          />
        ) : (
          <button
            className="bm-title"
            onClick={() => gotoBookmark(bm)}
            onDoubleClick={() => setEditing(bm.id)}
            title={bm.title}
          >
            <span className="bm-label">{bm.title || "—"}</span>
            {bm.url ? (
              <span className="bm-tag">{t("bmLinkTag")}</span>
            ) : bm.src === 0 ? (
              <span className="bm-tag dim">{t("bmNoTarget")}</span>
            ) : (
              <span className="bm-page">{bm.src}</span>
            )}
          </button>
        )}

        <button
          className="bm-more"
          data-tip={t("tipBmMore")}
          aria-label={t("tipBmMore")}
          aria-haspopup="menu"
          aria-expanded={menuId === bm.id}
          onClick={(e) => openMenu(e, me)}
        >
          ⋯
        </button>
      </div>
      {bm.open &&
        bm.children.map((c, i) => (
          <Row
            key={c.id}
            bm={c}
            depth={depth + 1}
            isRoot={false}
            index={i}
            count={bm.children.length}
            editing={editing}
            menuId={menuId}
            setEditing={setEditing}
            openMenu={openMenu}
          />
        ))}
    </>
  );
}

export default function BookmarkPanel({ tab }: { tab: Tab }) {
  const t = useT();
  const bms = tab.bookmarks ?? [];
  const [editing, setEditing] = useState<string | null>(null);
  const [menu, setMenu] = useState<Menu | null>(null);
  const [menuEl, setMenuEl] = useState<HTMLDivElement | null>(null);

  useEffect(() => {
    setMenu(null);
    setEditing(null);
  }, [tab.id]);

  // A menu driven from the keyboard hands focus back to the control that opened it.
  const closeMenu = () => {
    const m = menu;
    setMenu(null);
    if (m && menuEl?.contains(document.activeElement)) m.returnTo?.focus();
  };

  useOutsideClose(!!menu, ".ctx-menu", closeMenu);
  useFocusMenuOnOpen(menuEl, !!menu?.fromKeyboard);

  // From a pointer the menu opens at the pointer; from the keyboard it anchors
  // to the control (the "more" button or the row) instead of at (0,0).
  const openMenu = (e: React.MouseEvent | React.KeyboardEvent, tg: Target) => {
    const el = e.currentTarget as HTMLElement;
    const keyboard = !("clientX" in e) || isKeyboardClick(e);
    if (keyboard) {
      const r = el.getBoundingClientRect();
      const more = el.classList.contains("bm-more") ? el : el.querySelector<HTMLElement>(".bm-more");
      const a = (more ?? el).getBoundingClientRect();
      setMenu({ ...tg, x: a.left, y: Math.max(r.bottom, a.bottom) + 2, fromKeyboard: true, returnTo: el });
    } else {
      setMenu({ ...tg, x: e.clientX, y: e.clientY, fromKeyboard: false, returnTo: null });
    }
  };

  const item = (label: string, fn: () => void, disabled = false) => (
    <button
      role="menuitem"
      disabled={disabled}
      onClick={() => {
        closeMenu();
        fn();
      }}
    >
      {label}
    </button>
  );

  return (
    <div className="outline-panel">
      <button className="bm-add" data-tip={t("tipBmAdd")} onClick={() => addBookmarkHere()}>
        <svg
          width="14"
          height="14"
          viewBox="0 0 24 24"
          fill="none"
          stroke="currentColor"
          strokeWidth="2"
          strokeLinecap="round"
          strokeLinejoin="round"
          aria-hidden="true"
        >
          <path d="M19 21l-7-5-7 5V5a2 2 0 0 1 2-2h10a2 2 0 0 1 2 2z M12 8v6 M9 11h6" />
        </svg>
        <span>{t("bmAdd")}</span>
      </button>

      {bms.length === 0 ? (
        <div className="bm-empty">
          <p>{t("bmEmpty")}</p>
          <p className="dim">{t("bmHint")}</p>
        </div>
      ) : (
        <div className="bm-tree">
          {bms.map((b, i) => (
            <Row
              key={b.id}
              bm={b}
              depth={0}
              isRoot
              index={i}
              count={bms.length}
              editing={editing}
              menuId={menu?.bm.id ?? null}
              setEditing={setEditing}
              openMenu={openMenu}
            />
          ))}
        </div>
      )}

      {menu && (
        <div
          className="ctx-menu"
          role="menu"
          ref={setMenuEl}
          onKeyDown={(e) => handleMenuKeys(e, menuEl)}
          style={{
            left: Math.min(menu.x, window.innerWidth - 210),
            top: Math.min(menu.y, window.innerHeight - 290),
          }}
        >
          {item(t("tipBmRename"), () => setEditing(menu.bm.id))}
          {item(t("tipBmRetarget"), () => retargetBookmark(menu.bm.id))}
          <div className="ctx-sep" role="separator" />
          {item(t("tipBmUp"), () => moveBookmark(menu.bm.id, -1), menu.index === 0)}
          {item(t("tipBmDown"), () => moveBookmark(menu.bm.id, 1), menu.index === menu.count - 1)}
          {item(t("tipBmIndent"), () => indentBookmark(menu.bm.id), menu.index === 0)}
          {item(t("tipBmOutdent"), () => outdentBookmark(menu.bm.id), menu.isRoot)}
          <div className="ctx-sep" role="separator" />
          {item(t("tipBmDelete"), () => deleteBookmark(menu.bm.id))}
        </div>
      )}
    </div>
  );
}
