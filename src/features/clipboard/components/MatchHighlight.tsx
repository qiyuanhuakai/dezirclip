import { useMemo } from "react";

interface MatchRange {
  start: number;
  end: number;
}

interface MatchHighlightProps {
  text: string;
  matches?: MatchRange[];
  query?: string;
  snippet?: boolean;
  className?: string;
}

function computeQueryMatches(text: string, query: string): MatchRange[] {
  if (!query) return [];
  const lower = text.toLowerCase();
  const lowerQuery = query.toLowerCase();
  const ranges: MatchRange[] = [];
  let idx = 0;
  while (idx < lower.length) {
    const pos = lower.indexOf(lowerQuery, idx);
    if (pos === -1) break;
    ranges.push({ start: pos, end: pos + query.length });
    idx = pos + 1;
  }
  return ranges;
}

function buildSegments(
  text: string,
  matches: MatchRange[]
): Array<{ text: string; highlighted: boolean }> {
  if (matches.length === 0) return [{ text, highlighted: false }];

  const sorted = [...matches].sort((a, b) => a.start - b.start);
  const merged: MatchRange[] = [];
  for (const range of sorted) {
    if (merged.length > 0 && range.start <= merged[merged.length - 1].end) {
      merged[merged.length - 1].end = Math.max(
        merged[merged.length - 1].end,
        range.end
      );
    } else {
      merged.push({ ...range });
    }
  }

  const segments: Array<{ text: string; highlighted: boolean }> = [];
  let cursor = 0;
  for (const range of merged) {
    if (cursor < range.start) {
      segments.push({ text: text.slice(cursor, range.start), highlighted: false });
    }
    segments.push({ text: text.slice(range.start, range.end), highlighted: true });
    cursor = range.end;
  }
  if (cursor < text.length) {
    segments.push({ text: text.slice(cursor), highlighted: false });
  }
  return segments;
}

export const MatchHighlight = ({
  text,
  matches,
  query,
  snippet = false,
  className,
}: MatchHighlightProps) => {
  const resolvedMatches = useMemo(() => {
    if (matches) return matches;
    if (query) return computeQueryMatches(text, query);
    return [];
  }, [text, matches, query]);

  if (snippet) {
    return (
      <span
        className={`match-highlight${className ? ` ${className}` : ""}`}
        dangerouslySetInnerHTML={{ __html: text }}
      />
    );
  }

  const segments = buildSegments(text, resolvedMatches);

  return (
    <span className={`match-highlight${className ? ` ${className}` : ""}`} data-test-match-highlight>
      {segments.map((seg, i) =>
        seg.highlighted ? (
          <mark key={i}>{seg.text}</mark>
        ) : (
          <span key={i}>{seg.text}</span>
        )
      )}
    </span>
  );
};
