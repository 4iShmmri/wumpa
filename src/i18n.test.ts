import { describe, expect, it } from "vitest";
import { t, dict } from "./i18n";

describe("translations", () => {
  it("every key has both languages and no empty text", () => {
    for (const [key, entry] of Object.entries(dict)) {
      expect(entry.en, key).toBeTruthy();
      expect(entry.ar, key).toBeTruthy();
    }
  });
  it("keeps the same placeholders in both languages", () => {
    const vars = (s: string) => (s.match(/\{\w+\}/g) ?? []).sort();
    for (const [key, entry] of Object.entries(dict)) {
      expect(vars(entry.ar), key).toEqual(vars(entry.en));
    }
  });
  it("interpolates variables", () => {
    expect(t("stSaved", { name: "a.pdf" })).toContain("a.pdf");
  });
});
