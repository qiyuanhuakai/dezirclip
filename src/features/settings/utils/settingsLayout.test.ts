import { describe, expect, it } from "vitest";
import {
    DEFAULT_SETTINGS_GROUP,
    PANE_THREE_COLUMNS_MIN_WIDTH,
    PANE_TWO_COLUMNS_MIN_WIDTH,
    SETTINGS_GROUPS,
    SPLIT_MIN_WIDTH,
    resolveSettingsLayout,
    type SettingsGroupId,
} from "./settingsLayout";
import { zh } from "../../../locales/zh";
import { en } from "../../../locales/en";
import { tw } from "../../../locales/tw";

/** Keys the accordion writes into `collapsedGroups` -- see `useAppState`. */
const COLLAPSED_GROUP_KEYS: readonly SettingsGroupId[] = [
    "general",
    "clipboard",
    "appearance",
    "default_apps",
    "data",
    "tools",
];

const locales = { zh, en, tw } as const;

describe("SETTINGS_GROUPS", () => {
    it("lists exactly the groups the accordion tracks", () => {
        expect(SETTINGS_GROUPS.map(g => g.id)).toEqual(COLLAPSED_GROUP_KEYS);
    });

    it("has no duplicate ids, so the rail cannot highlight two rows", () => {
        expect(new Set(SETTINGS_GROUPS.map(g => g.id)).size).toBe(SETTINGS_GROUPS.length);
    });

    it("starts the rail on a group that is actually in the list", () => {
        expect(SETTINGS_GROUPS.map(g => g.id)).toContain(DEFAULT_SETTINGS_GROUP);
    });

    // The rail has no header of its own; it borrows the accordion's titles. A
    // typo in a labelKey renders a blank button in the sidebar with nothing in
    // the build to complain, so it is checked here instead.
    it("borrows label keys that resolve in every locale", () => {
        for (const { labelKey } of SETTINGS_GROUPS) {
            for (const [name, dict] of Object.entries(locales)) {
                expect(
                    typeof (dict as Record<string, unknown>)[labelKey],
                    `${name}.${labelKey} is missing`
                ).toBe("string");
            }
        }
    });

    it("does not reuse one label for two groups", () => {
        const keys = SETTINGS_GROUPS.map(g => g.labelKey);
        expect(new Set(keys).size).toBe(keys.length);
    });
});

describe("resolveSettingsLayout", () => {
    it("keeps the single column below the split width", () => {
        expect(resolveSettingsLayout(SPLIT_MIN_WIDTH - 1)).toEqual({
            split: false,
            paneColumns: 1,
        });
    });

    it("splits at exactly the split width", () => {
        expect(resolveSettingsLayout(SPLIT_MIN_WIDTH)).toEqual({
            split: true,
            paneColumns: 1,
        });
    });

    it("still uses one pane column just under the two-column width", () => {
        expect(resolveSettingsLayout(PANE_TWO_COLUMNS_MIN_WIDTH - 1).paneColumns).toBe(1);
    });

    it("goes to two pane columns at the two-column width", () => {
        expect(resolveSettingsLayout(PANE_TWO_COLUMNS_MIN_WIDTH).paneColumns).toBe(2);
    });

    it("holds two columns just under the three-column width", () => {
        expect(resolveSettingsLayout(PANE_THREE_COLUMNS_MIN_WIDTH - 1).paneColumns).toBe(2);
    });

    it("goes to three pane columns at the three-column width", () => {
        expect(resolveSettingsLayout(PANE_THREE_COLUMNS_MIN_WIDTH).paneColumns).toBe(3);
    });

    it("never returns more columns than the split layout asked for", () => {
        for (const width of [0, 250, 352, 559, 560, 699, 700, 999, 1000, 4000]) {
            const layout = resolveSettingsLayout(width);
            if (!layout.split) expect(layout.paneColumns).toBe(1);
            else expect([1, 2, 3]).toContain(layout.paneColumns);
        }
    });
});
