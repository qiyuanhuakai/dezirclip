export interface FuzzyResult {
  matched: boolean;
  score: number;
  matchedIndices: number[];
}

const EMPTY_RESULT: FuzzyResult = { matched: false, score: 0, matchedIndices: [] };

export function fuzzyMatch(input: string, target: string): FuzzyResult {
  if (!input) return { matched: true, score: 1, matchedIndices: [] };
  if (!target) return EMPTY_RESULT;

  const inputLower = input.toLowerCase();
  const targetLower = target.toLowerCase();
  const inputLen = inputLower.length;
  const targetLen = targetLower.length;

  if (inputLen > targetLen) return EMPTY_RESULT;

  if (inputLower === targetLower) {
    return {
      matched: true,
      score: 1000,
      matchedIndices: Array.from({ length: targetLen }, (_, i) => i)
    };
  }

  if (targetLower.includes(inputLower)) {
    const startIdx = targetLower.indexOf(inputLower);
    const matchedIndices = Array.from({ length: inputLen }, (_, i) => startIdx + i);
    const lengthPenalty = (targetLen - inputLen) * 2;
    const startBonus = startIdx === 0 ? 100 : 50;
    return {
      matched: true,
      score: 500 - lengthPenalty + startBonus,
      matchedIndices
    };
  }

  let inputIdx = 0;
  let targetIdx = 0;
  const matchedIndices: number[] = [];
  let consecutiveBonus = 0;
  let lastMatchIdx = -2;

  while (inputIdx < inputLen && targetIdx < targetLen) {
    if (inputLower[inputIdx] === targetLower[targetIdx]) {
      matchedIndices.push(targetIdx);
      if (targetIdx === lastMatchIdx + 1) {
        consecutiveBonus += 10;
      }
      lastMatchIdx = targetIdx;
      inputIdx++;
    }
    targetIdx++;
  }

  if (inputIdx < inputLen) return EMPTY_RESULT;

  const lengthPenalty = (targetLen - inputLen) * 3;
  const gapPenalty = (matchedIndices.length > 0
    ? matchedIndices[matchedIndices.length - 1] - matchedIndices[0] + 1 - inputLen
    : 0) * 5;
  const baseScore = 200 - lengthPenalty - gapPenalty + consecutiveBonus;

  return {
    matched: true,
    score: Math.max(baseScore, 1),
    matchedIndices
  };
}

export interface FuzzyQuery {
  raw: string;
  terms: string[];
}

export function parseFuzzyQuery(query: string): FuzzyQuery {
  const trimmed = query.trim();
  if (!trimmed) return { raw: "", terms: [] };
  const terms = trimmed.split(/\s+/).filter(Boolean);
  return { raw: trimmed, terms };
}

export function fuzzyFilter<T>(
  items: T[],
  query: string,
  getTarget: (item: T) => string
): Array<{ item: T; score: number; matchedIndices: number[] }> {
  const { terms } = parseFuzzyQuery(query);
  if (terms.length === 0) {
    return items.map((item) => ({ item, score: 0, matchedIndices: [] }));
  }

  const scored: Array<{ item: T; score: number; matchedIndices: number[] }> = [];
  for (const item of items) {
    const target = getTarget(item);
    let totalScore = 0;
    const allIndices: number[] = [];
    let allMatched = true;

    for (const term of terms) {
      const result = fuzzyMatch(term, target);
      if (!result.matched) {
        allMatched = false;
        break;
      }
      totalScore += result.score;
      allIndices.push(...result.matchedIndices);
    }

    if (allMatched) {
      scored.push({ item, score: totalScore, matchedIndices: allIndices });
    }
  }

  scored.sort((a, b) => {
    if (b.score !== a.score) return b.score - a.score;
    return 0;
  });

  return scored;
}
