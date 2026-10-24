import { describe, expect, it } from 'vitest';
import { matchesTagName, parseTagFilter } from './tagQuery';

describe('tag query syntax', () => {
    it('keeps tag selection distinct from content search', () => {
        expect(parseTagFilter('password')).toBeNull();
        expect(parseTagFilter('notes tag:work')).toBeNull();
        expect(parseTagFilter('tag:password')).toBe('password');
        expect(parseTagFilter(' TAG: 密码 ')).toBe('密码');
        expect(parseTagFilter('tag:')).toBe('');
    });

    it('preserves exact tag names, punctuation and internal spaces', () => {
        expect(parseTagFilter('tag: Team A: 100%_ ')).toBe('Team A: 100%_');
        expect(parseTagFilter('tag:sensitive')).toBe('sensitive');
    });

    it('filters tag names with or without the selection prefix', () => {
        expect(matchesTagName('Password', 'pass')).toBe(true);
        expect(matchesTagName('Password', ' TAG:PASS ')).toBe(true);
        expect(matchesTagName('密码', 'tag: 密')).toBe(true);
        expect(matchesTagName('work', 'tag:password')).toBe(false);
        expect(matchesTagName('work', 'tag:')).toBe(true);
    });
});
