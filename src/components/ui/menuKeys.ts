import { useCallback, useEffect, useRef, useState } from "react";
import { useOutsideClose } from "./useOutsideClose";

// Keyboard support shared by every menu: arrow keys, Home and End move focus
// between the enabled items, and a menu opened from the keyboard focuses its
// first (or last) item. Escape closing is handled by useOutsideClose.

const ITEM_SEL = "button:not(:disabled), [role^=menuitem]:not([aria-disabled=true])";

export function menuItems(root: HTMLElement | null): HTMLElement[] {
  return root ? Array.from(root.querySelectorAll<HTMLElement>(ITEM_SEL)) : [];
}

export type MenuEdge = "first" | "last";

export function focusMenuItem(root: HTMLElement | null, which: MenuEdge) {
  const items = menuItems(root);
  if (items.length) items[which === "first" ? 0 : items.length - 1].focus();
}

// Call from a menu's onKeyDown. Returns true when the key was consumed.
export function handleMenuKeys(e: React.KeyboardEvent, root: HTMLElement | null): boolean {
  const items = menuItems(root);
  if (!items.length) return false;
  const cur = items.indexOf(document.activeElement as HTMLElement);
  let next: number;
  switch (e.key) {
    case "ArrowDown":
      next = cur < 0 ? 0 : (cur + 1) % items.length;
      break;
    case "ArrowUp":
      next = cur < 0 ? items.length - 1 : (cur - 1 + items.length) % items.length;
      break;
    case "Home":
      next = 0;
      break;
    case "End":
      next = items.length - 1;
      break;
    default:
      return false;
  }
  e.preventDefault();
  e.stopPropagation();
  items[next].focus();
  return true;
}

// Whether a click came from the keyboard (Enter/Space on a button) rather
// than a pointer, so a menu can anchor to the control instead of the pointer.
export const isKeyboardClick = (e: React.MouseEvent) => e.detail === 0 && e.clientX === 0 && e.clientY === 0;

// Whether a keydown asks for the context menu (Shift+F10 or the Menu key).
export const isContextMenuKey = (e: React.KeyboardEvent) =>
  e.key === "ContextMenu" || (e.key === "F10" && e.shiftKey);

// Focuses the first item of a context menu once it has mounted. Pass the
// element each render; it runs when the element appears.
export function useFocusMenuOnOpen(menu: HTMLElement | null, wanted: boolean) {
  useEffect(() => {
    if (menu && wanted) focusMenuItem(menu, "first");
  }, [menu, wanted]);
}

// State and props for a trigger button that opens a menu below it.
// - Enter/Space/ArrowDown on the trigger open the menu and focus its first
//   item; ArrowUp focuses the last. A pointer click opens without moving focus.
// - Closing returns focus to the trigger when it was inside the menu.
export function useMenuButton<W extends HTMLElement, B extends HTMLButtonElement = HTMLButtonElement>() {
  const [open, setOpen] = useState(false);
  const wrapRef = useRef<W>(null);
  const menuRef = useRef<HTMLDivElement>(null);
  const triggerRef = useRef<B>(null);
  const pending = useRef<MenuEdge | null>(null);

  const close = useCallback(() => {
    setOpen(false);
    if (wrapRef.current?.contains(document.activeElement)) triggerRef.current?.focus();
  }, []);

  useOutsideClose(open, wrapRef, close);

  useEffect(() => {
    if (open && pending.current) {
      focusMenuItem(menuRef.current, pending.current);
      pending.current = null;
    }
  }, [open]);

  const onTriggerKeyDown = (e: React.KeyboardEvent) => {
    const edge: MenuEdge | null =
      e.key === "ArrowDown"
        ? "first"
        : e.key === "ArrowUp"
          ? "last"
          : (e.key === "Enter" || e.key === " ") && !open
            ? "first"
            : null;
    if (!edge) return;
    e.preventDefault();
    if (open) focusMenuItem(menuRef.current, edge);
    else {
      pending.current = edge;
      setOpen(true);
    }
  };

  return {
    open,
    setOpen,
    close,
    wrapRef,
    menuRef,
    triggerRef,
    triggerProps: {
      "aria-haspopup": "menu" as const,
      "aria-expanded": open,
      onClick: () => setOpen((o) => !o),
      onKeyDown: onTriggerKeyDown,
    },
    menuProps: {
      ref: menuRef,
      role: "menu" as const,
      onKeyDown: (e: React.KeyboardEvent) => void handleMenuKeys(e, menuRef.current),
    },
  };
}
