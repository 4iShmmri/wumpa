import { useEffect } from "react";

// Closes a menu or popover on Escape or on a pointer press outside it. `inside`
// is either a ref to the menu element or a selector that marks its subtree.
export function useOutsideClose(
  open: boolean,
  inside: React.RefObject<HTMLElement | null> | string,
  close: () => void,
) {
  useEffect(() => {
    if (!open) return;
    const onDown = (e: PointerEvent) => {
      const target = e.target as Element | null;
      const hit =
        typeof inside === "string" ? !!target?.closest?.(inside) : !!inside.current?.contains(target as Node);
      if (!hit) close();
    };
    const onKey = (e: KeyboardEvent) => {
      if (e.key === "Escape") {
        e.stopPropagation();
        close();
      }
    };
    document.addEventListener("pointerdown", onDown, true);
    window.addEventListener("keydown", onKey, true);
    return () => {
      document.removeEventListener("pointerdown", onDown, true);
      window.removeEventListener("keydown", onKey, true);
    };
  }, [open]);
}
