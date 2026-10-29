import { describe, expect, it } from "vitest";
import { testing } from "./exportExtract";

const { lines, isRtl } = testing;

const item = (str: string, x: number, y: number, w: number, size = 10, bold = false) => ({
  str,
  x,
  y,
  w,
  size,
  ascent: 0.8,
  descent: -0.2,
  bold,
  italic: false,
});

describe("export line building", () => {
  it("joins items on one baseline and splits at wide gaps", () => {
    const out = lines([
      item("Country of", 50, 700, 60),
      item("Origin", 113, 700, 40),
      item("UNITED STATES", 168, 700.5, 100),
      item("far right column", 500, 700, 90),
      item("next line", 50, 688, 60),
    ]);
    expect(out.map((l) => l.text)).toEqual([
      "Country of Origin UNITED STATES",
      "far right column",
      "next line",
    ]);
    expect(out[0].bbox[0]).toBe(50);
    expect(out[0].bbox[2]).toBe(268);
    expect(out[0].words.length).toBe(5);
    expect(out[0].bbox[1]).toBeCloseTo(698, 0);
    expect(out[0].bbox[3]).toBeCloseTo(708, 0);
  });

  it("reads right-to-left lines from the right", () => {
    const out = lines([
      item("2024", 50, 500, 30),
      item("مارس", 85, 500, 30),
      item("14", 120, 500, 15),
      item("التاريخ:", 140, 500, 40),
    ]);
    expect(out).toHaveLength(1);
    expect(out[0].text).toBe("التاريخ: 14 مارس 2024");
    expect(out[0].words[0].text).toBe("التاريخ:");
    expect(isRtl("Invoice 12")).toBe(false);
    expect(isRtl("فاتورة 12")).toBe(true);
  });

  it("calls a line bold when most of its width is bold", () => {
    const out = lines([item("Total", 50, 100, 80, 10, true), item("x", 132, 100, 10)]);
    expect(out[0].bold).toBe(true);
    const mixed = lines([item("A", 50, 100, 10, 10, true), item("long regular tail", 62, 100, 120)]);
    expect(mixed[0].bold).toBe(false);
  });

  it("drops empty items", () => {
    expect(lines([])).toEqual([]);
  });
});
