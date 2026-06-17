import { describe, it, expect } from 'vitest';
import { formatShortcut } from './format-shortcut';

describe('formatShortcut', () => {
  it('uppercases first letter of each segment', () => {
    expect(formatShortcut('ctrl+shift+v')).toBe('Ctrl+Shift+V');
  });

  it('lowercases rest of segment', () => {
    expect(formatShortcut('CTRL+SHIFT+V')).toBe('Ctrl+Shift+V');
  });

  it('handles single key', () => {
    expect(formatShortcut('escape')).toBe('Escape');
  });

  it('trims whitespace', () => {
    expect(formatShortcut(' Ctrl + Shift + V ')).toBe('Ctrl+Shift+V');
  });
});
