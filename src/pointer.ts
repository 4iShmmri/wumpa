// Follows a pointer gesture to its end. A cancelled pointer (touchpad gesture,
// OS dialog) ends the gesture too instead of leaving listeners behind.
export function trackPointer(
  onMove: (e: PointerEvent) => void,
  onUp: (e: PointerEvent) => void,
  onCancel: (e: PointerEvent) => void = onUp,
) {
  const stop = () => {
    window.removeEventListener("pointermove", onMove);
    window.removeEventListener("pointerup", up);
    window.removeEventListener("pointercancel", cancel);
  };
  const up = (e: PointerEvent) => {
    stop();
    onUp(e);
  };
  const cancel = (e: PointerEvent) => {
    stop();
    onCancel(e);
  };
  window.addEventListener("pointermove", onMove);
  window.addEventListener("pointerup", up);
  window.addEventListener("pointercancel", cancel);
  return stop;
}
