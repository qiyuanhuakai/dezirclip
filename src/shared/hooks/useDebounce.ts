import { useState, useEffect } from "react";

// Custom hook for debouncing values
export function useDebounce<T>(value: T, delay: number, enabled = true): T {
  const [debouncedValue, setDebouncedValue] = useState<T>(value);

  useEffect(() => {
    if (!enabled) return;
    const handler = setTimeout(() => {
      setDebouncedValue(value);
    }, delay);

    return () => {
      clearTimeout(handler);
    };
  }, [value, delay, enabled]);

  return debouncedValue;
}
