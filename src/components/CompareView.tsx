import { useStore } from "../store";
import ResultsView from "./compare/ResultsView";
import SelectFiles from "./compare/SelectFiles";

// Full-window compare feature: the file picker page, then the results.
export default function CompareView() {
  const cmp = useStore((s) => s.compare);
  const dlg = useStore((s) => s.compareDialog);
  if (cmp) return <ResultsView cmp={cmp} key={cmp.leftKey} />;
  if (dlg) return <SelectFiles oldPath={dlg.oldPath} />;
  return null;
}
