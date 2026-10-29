// The one text palette shared by the stamp toolbar and the inline text editor.
export const TEXT_COLORS = [
  { hex: "#111111", nameKey: "colorBlack" },
  { hex: "#c0392b", nameKey: "colorRed" },
  { hex: "#1d4ed8", nameKey: "colorBlue" },
] as const;

export const DEFAULT_TEXT_COLOR: string = TEXT_COLORS[0].hex;

export const MIN_TEXT_SIZE = 6;
export const MAX_TEXT_SIZE = 120;
export const TEXT_SIZE_STEP = 2;

export const clampTextSize = (size: number) => Math.max(MIN_TEXT_SIZE, Math.min(MAX_TEXT_SIZE, size));
