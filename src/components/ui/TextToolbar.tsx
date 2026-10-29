import { useT } from "../../i18n";
import { TEXT_COLORS, TEXT_SIZE_STEP, clampTextSize } from "./textPalette";

// Size and colour controls for free text. Renders only the buttons so the
// parent keeps its own container (the stamp bar, the popover's tool row) and
// the styling that goes with it; `children` land after the colour dots.
export default function TextToolbar({
  size,
  color,
  onSize,
  onColor,
  showSize,
  children,
}: {
  size: number;
  color: string | undefined;
  onSize: (size: number) => void;
  onColor: (hex: string) => void;
  showSize?: boolean;
  children?: React.ReactNode;
}) {
  const t = useT();
  return (
    <>
      <button data-tip={t("tipSmaller")} onClick={() => onSize(clampTextSize(size - TEXT_SIZE_STEP))}>
        A−
      </button>
      <button data-tip={t("tipLarger")} onClick={() => onSize(clampTextSize(size + TEXT_SIZE_STEP))}>
        A+
      </button>
      {showSize && <span className="te-size">{Math.round(size)}pt</span>}
      {TEXT_COLORS.map((c) => (
        <button
          key={c.hex}
          className={"dot" + (color === c.hex ? " on" : "")}
          style={{ background: c.hex }}
          data-tip={t("tipTextColor", { c: t(c.nameKey) })}
          onClick={() => onColor(c.hex)}
        />
      ))}
      {children}
    </>
  );
}
