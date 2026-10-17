import { describe, expect, it } from "vitest";
import {
    DEFAULT_SETTINGS_GROUP,
    PANE_THREE_COLUMNS_MIN_WIDTH,
    PANE_TWO_COLUMNS_MIN_WIDTH,
    SETTINGS_GROUPS,
    SPLIT_MIN_WIDTH,
    isSettingsGroupCollapsed,
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

/**
 * `collapsed` is a load-bearing flag, not just a visual one: `ToolsCaptureSection`
 * mounts only while tools is open and probes for an OCR engine from its mount
 * effect, and on Linux that probe is a `tesseract --version` subprocess. These
 * cases pin which groups a layout switch is allowed to open.
 */
describe("isSettingsGroupCollapsed", () => {
    const fold = (split: boolean, shown: boolean, collapsed: boolean) =>
        isSettingsGroupCollapsed({ split, shown, collapsed });

    it("leaves the narrow accordion exactly as the user folded it", () => {
        expect(fold(false, true, true)).toBe(true);
        expect(fold(false, true, false)).toBe(false);
    });

    it("opens the group the rail is pointing at, whatever the accordion said", () => {
        expect(fold(true, true, true)).toBe(false);
        expect(fold(true, true, false)).toBe(false);
    });

    // The reported defect: a group the rail is not pointing at used to open on
    // every crossing into the split layout, which started its data loads -- for
    // tools that is a subprocess probe -- behind a hidden pane, and repeated it
    // each time the window narrowed again.
    it("keeps a group the rail is not pointing at folded as the accordion left it", () => {
        expect(fold(true, false, true)).toBe(true);
        expect(fold(true, false, false)).toBe(false);
    });

    it("starts no group body when the window crosses the breakpoint with settings closed", () => {
        // Every group starts folded (`useAppState`), the rail starts on `general`,
        // and the settings page stays mounted behind the clipboard list.
        for (const width of [559, 560, 600, 700, 1000, 1200]) {
            const { split } = resolveSettingsLayout(width);
            const opened = COLLAPSED_GROUP_KEYS.filter((id) =>
                !fold(split, split && id === DEFAULT_SETTINGS_GROUP, true)
            );
            expect(opened, `width ${width} opened ${opened.join(", ")}`).toEqual(
                split ? [DEFAULT_SETTINGS_GROUP] : []
            );
        }
    });

    it("survives a group opened in a narrow window being widened out of sight", () => {
        // The export/import draft case: `data` was expanded, then the rail took
        // over on `general`. Folding it here would drop the open dialog.
        expect(fold(true, false, false)).toBe(false);
    });

    it("restores the accordion's own folds after a round trip through the rail", () => {
        // Nothing writes `collapsedGroups` while split, so narrowing has to land
        // back on the flags the accordion was left with.
        const stored = Object.fromEntries(COLLAPSED_GROUP_KEYS.map(id => [id, true])) as Record<SettingsGroupId, boolean>;
        stored.data = false;
        for (const id of COLLAPSED_GROUP_KEYS) {
            fold(true, id === DEFAULT_SETTINGS_GROUP, stored[id]);
            expect(fold(false, true, stored[id]), `${id} changed across the round trip`).toBe(stored[id]);
        }
    });
});
