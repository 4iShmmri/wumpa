import { describe, expect, it, vi } from "vitest";
import { trackPointer } from "./pointer";

describe("trackPointer", () => {
  it("forwards moves, finishes on pointerup and removes its listeners", () => {
    const onMove = vi.fn();
    const onUp = vi.fn();
    trackPointer(onMove, onUp);
    window.dispatchEvent(new PointerEvent("pointermove"));
    window.dispatchEvent(new PointerEvent("pointerup"));
    window.dispatchEvent(new PointerEvent("pointermove"));
    expect(onMove).toHaveBeenCalledTimes(1);
    expect(onUp).toHaveBeenCalledTimes(1);
  });
  it("routes a cancelled pointer to onCancel", () => {
    const onUp = vi.fn();
    const onCancel = vi.fn();
    trackPointer(() => {}, onUp, onCancel);
    window.dispatchEvent(new PointerEvent("pointercancel"));
    expect(onUp).not.toHaveBeenCalled();
    expect(onCancel).toHaveBeenCalledTimes(1);
  });
});
