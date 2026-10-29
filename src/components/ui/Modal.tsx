import { useEffect, useRef } from "react";

// One shell for every dialog: Escape closes, the backdrop closes only when a
// click starts and ends on it (a drag-select that leaves the dialog does not).
export default function Modal({
  children,
  onClose,
  wide,
  className,
}: {
  children: React.ReactNode;
  onClose: () => void;
  wide?: boolean;
  className?: string;
}) {
  const downOnBackdrop = useRef(false);

  useEffect(() => {
    const onKey = (e: KeyboardEvent) => {
      if (e.key === "Escape") {
        e.stopPropagation();
        onClose();
      }
    };
    window.addEventListener("keydown", onKey, true);
    return () => window.removeEventListener("keydown", onKey, true);
  }, [onClose]);

  return (
    <div
      className="modal-backdrop"
      onPointerDown={(e) => {
        e.stopPropagation();
        downOnBackdrop.current = e.target === e.currentTarget;
      }}
      onPointerUp={(e) => {
        if (downOnBackdrop.current && e.target === e.currentTarget) onClose();
        downOnBackdrop.current = false;
      }}
    >
      <div
        className={"modal" + (wide ? " modal-wide" : "") + (className ? " " + className : "")}
        role="dialog"
        aria-modal="true"
      >
        {children}
      </div>
    </div>
  );
}
