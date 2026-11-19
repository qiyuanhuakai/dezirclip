import { useState, useEffect } from "react";
import { SearchBar as Search, type SearchMode } from "@deziro/ui-react/components/SearchBar";
import { invoke } from "../../../shared/lib/desktopInvoke";
import { useSearch } from "../../../shared/hooks/useSearch";

const ALL_MODES: SearchMode[] = ["fts", "fuzzy", "regex"];

export const SearchBar = () => {
  const { results, ...search } = useSearch();
  const { mode, setMode } = search;
  const [enabledModes, setEnabledModes] = useState<SearchMode[]>(ALL_MODES);
  useEffect(() => {
    const fetchSettings = async () => {
      try {
        const settings = await invoke<Record<string, string>>("get_settings");
        const modes: SearchMode[] = ["fts"];
        if (settings["app.search_fuzzy_enabled"] !== "false") {
          modes.push("fuzzy");
        }
        if (settings["app.search_regex_enabled"] !== "false") {
          modes.push("regex");
        }
        setEnabledModes(modes);
      } catch {
        setEnabledModes(ALL_MODES);
      }
    };
    fetchSettings();
  }, []);

  useEffect(() => {
    if (!enabledModes.includes(mode)) {
      setMode("fts");
    }
  }, [enabledModes, mode, setMode]);

  return <Search {...search} enabledModes={enabledModes} resultCount={results.length} />;
};
