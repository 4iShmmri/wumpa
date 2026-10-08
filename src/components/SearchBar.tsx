import { useEffect, useRef } from "react";
import { useStore } from "../store";
import { runSearch, searchStep } from "../actions";
import { useT } from "../i18n";

export default function SearchBar() {
  const t = useT();
  const search = useStore((s) => s.search);
  const inputRef = useRef<HTMLInputElement>(null);
  const debounce = useRef<number | undefined>(undefined);

  useEffect(() => {
    if (search.open) inputRef.current?.select();
  }, [search.open]);

  if (!search.open) return null;

  const close = () => useStore.getState().setSearch({ open: false });

  return (
    <div className="searchbar">
      <input
        ref={inputRef}
        autoFocus
        placeholder={t("searchPlaceholder")}
        value={search.query}
        onChange={(e) => {
          const q = e.target.value;
          useStore.getState().setSearch({ query: q });
          window.clearTimeout(debounce.current);
          debounce.current = window.setTimeout(() => runSearch(q), 300);
        }}
        onKeyDown={(e) => {
          if (e.key === "Enter") searchStep(e.shiftKey ? -1 : 1);
          if (e.key === "Escape") close();
        }}
      />
      <span className="search-count">
        {search.matches.length ? `${search.current + 1}/${search.matches.length}` : "0"}
      </span>
      <button onClick={() => searchStep(-1)} data-tip={t("tipPrevMatch")}>‹</button>
      <button onClick={() => searchStep(1)} data-tip={t("tipNextMatch")}>›</button>
      <button onClick={close} data-tip={t("tipCloseSearch")}>×</button>
    </div>
  );
}
