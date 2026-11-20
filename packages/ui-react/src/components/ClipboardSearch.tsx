import { Suspense, useEffect, useId, useRef } from "react";
import type { KeyboardEvent, ReactNode, RefObject } from "react";
import { Search } from "lucide-react";

export interface TagFilterHandle {
  handleInputKeyDown: (event: KeyboardEvent<HTMLInputElement>) => void;
}

export interface HeaderTagFilter {
  ref: RefObject<TagFilterHandle | null>;
  id: string;
  query: string;
  isComposing: boolean;
  inputRef: RefObject<HTMLInputElement | null>;
  onClose: () => void;
  onSelect: (tag: string) => void;
}

export interface ClipboardSearchProps {
  t: (key: string) => string;
  onActivate?: () => void;
  renderTagFilter?: (props: HeaderTagFilter) => ReactNode;
  contentTypes: readonly { id: string; label: string }[];
  search: string;
  setSearch: (val: string) => void;
  isComposing: boolean;
  setIsComposing: (val: boolean) => void;
  searchInputRef: RefObject<HTMLInputElement | null>;
  showTagFilter: boolean;
  setShowTagFilter: (val: boolean) => void;
  searchIsFocused: boolean;
  setSearchIsFocused: (val: boolean) => void;
  setEditingTagsId: (val: number | null) => void;
  typeFilter: string | null;
  setTypeFilter: (val: string | null) => void;
}

export function ClipboardSearch({
  t, onActivate, renderTagFilter, contentTypes, search, setSearch, isComposing, setIsComposing,
  searchInputRef, showTagFilter, setShowTagFilter, searchIsFocused, setSearchIsFocused,
  setEditingTagsId, typeFilter, setTypeFilter,
}: ClipboardSearchProps) {
  const tagFilter = useRef<TagFilterHandle>(null);
  const tagFilterId = useId();
  const blurTimer = useRef<ReturnType<typeof setTimeout> | null>(null);
  const tagFilterVisible = Boolean(renderTagFilter && showTagFilter && searchIsFocused);
  useEffect(() => () => { if (blurTimer.current !== null) clearTimeout(blurTimer.current); }, []);

  return (
    <div className="search-container" data-clipboard-search>
      <div className="clipboard-search-field">
        <Search size={14} className="search-icon" aria-hidden="true" />
        <input
          ref={searchInputRef}
          type="text"
          className={`search-input ${tagFilterVisible ? 'dropdown-open' : ''}`}
          placeholder={t('search_placeholder')}
          aria-label={t('search_placeholder')}
          value={search}
          role={renderTagFilter ? "combobox" : undefined}
          aria-autocomplete={renderTagFilter ? "list" : undefined}
          aria-expanded={renderTagFilter ? tagFilterVisible : undefined}
          aria-controls={tagFilterVisible ? tagFilterId : undefined}
          onKeyDown={event => {
            if (tagFilter.current) tagFilter.current.handleInputKeyDown(event);
            else if (!event.nativeEvent.isComposing && tagFilterVisible && ['ArrowDown', 'ArrowUp', 'Enter', 'Escape'].includes(event.key)) {
              event.preventDefault(); event.stopPropagation();
              if (event.key === 'Escape') setShowTagFilter(false);
            }
          }}
          onCompositionStart={() => setIsComposing(true)}
          onCompositionEnd={(e) => {
            setIsComposing(false);
            setSearch((e.target as HTMLInputElement).value);
          }}
          onChange={(e) => {
            setSearch(e.target.value);
          }}
          onMouseDown={onActivate}
          onClick={() => { setShowTagFilter(true); setEditingTagsId(null); }}
          onFocus={() => {
            if (blurTimer.current !== null) clearTimeout(blurTimer.current);
            onActivate?.();
            setShowTagFilter(true);
            setSearchIsFocused(true);
            setEditingTagsId(null);
          }}
          onBlur={() => {
            setIsComposing(false);
            blurTimer.current = setTimeout(() => {
              blurTimer.current = null;
              if (document.activeElement?.closest('.dc-tag-filter-picker')) return;
              setShowTagFilter(false);
              setSearchIsFocused(false);
            }, 200);
          }}
        />
        {tagFilterVisible && (
          <Suspense fallback={<div className="dc-tag-filter-picker" id={tagFilterId} role="listbox" aria-label={t('tags')}><div className="dc-tag-filter-empty">{t('common_loading')}</div></div>}>
            {renderTagFilter?.({
              ref: tagFilter, id: tagFilterId, query: search, isComposing, inputRef: searchInputRef,
              onClose: () => {
                setShowTagFilter(false);
                if (document.activeElement !== searchInputRef.current) setSearchIsFocused(false);
              },
              onSelect: tag => { setSearch("tag:" + tag); setShowTagFilter(false); },
            })}
          </Suspense>
        )}
      </div>
      <div
        className="hide-scrollbar clipboard-search-types"
        onWheel={(e) => {
          if (e.deltaY !== 0) {
            e.currentTarget.scrollLeft += e.deltaY;
          }
        }}
      >
        {contentTypes.map(({ id: type, label }) => (
          <button
            key={type}
            type="button"
            className={`btn-icon ${typeFilter === type ? 'active' : ''}`}
            onClick={() => setTypeFilter(typeFilter === type ? null : type)}
            aria-pressed={typeFilter === type}
            title={label}
          >
            {label}
          </button>
        ))}
      </div>
    </div>
  );
}
