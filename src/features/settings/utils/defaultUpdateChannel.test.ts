import { describe, expect, it } from 'vitest';
import { defaultUpdateChannel } from './defaultUpdateChannel';

describe('installed release channel default', () => {
    it('lets an alpha installation discover later prereleases without another opt-in', () => {
        for (const version of ['0.2.0-alpha.1', '0.2.0-beta.2', '0.2.0-rc.1+build.3']) {
            expect(defaultUpdateChannel(version)).toBe('preview');
        }
    });
    it('keeps stable and unknown builds on stable', () => {
        for (const version of ['', '0.1.20', '0.2.0+alpha-test', 'unknown-alpha']) {
            expect(defaultUpdateChannel(version)).toBe('stable');
        }
    });
    it('preserves both explicit preferences across release families', () => {
        expect(defaultUpdateChannel('0.2.0-alpha.1', 'stable')).toBe('stable');
        expect(defaultUpdateChannel('0.2.0', 'preview')).toBe('preview');
    });
});
