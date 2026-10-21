/**
 * The settings page has two shapes.
 *
 * Narrow windows keep the original vertical accordion: one centred column, six
 * collapsible groups stacked, the whole page scrolling inside `.main-content`.
 * That column was capped at 420px, so a landscape window left most of its width
 * empty -- and because every group stays mounted, opening a 900px-tall group
 * pushed the other five out of sight.
 *
 * Wide windows get a nav rail on the left and a single group on the right. Only
 * the selected group mounts, so the detail pane is the only thing that scrolls
 * and the rail stays a fixed overview. Below 1000px the pane still uses one
 * column, which is what the old layout looked like; past that the rows flow into
 * two and then three columns, which is what turns "19 rows of clipboard settings
 * means three screens of scrolling" into about one.
 *
 * These breakpoints are the only place the numbers live. The shape is decided in
 * JS rather than by a CSS media query because it decides *what mounts*: the rail
 * mounts six headers, the pane mounts one body, and CSS cannot unmount anything.
 * Appearance stays in `src/styles/` -- layout here, paint there.
 */

export type SettingsGroupId =
  | "general"
  | "clipboard"
  | "appearance"
  | "default_apps"
  | "data"
  | "tools"
  | "about";

export interface SettingsGroupMeta {
  readonly id: SettingsGroupId;
  /** Locale key for the group title, reused from the accordion headers. */
  readonly labelKey: string;
}

/** Render order here matches the accordion, and `id` matches `collapsedGroups`. */
export const SETTINGS_GROUPS: readonly SettingsGroupMeta[] = [
  { id: "general", labelKey: "general_settings" },
  { id: "clipboard", labelKey: "clipboard_settings" },
  { id: "appearance", labelKey: "appearance_settings" },
  { id: "default_apps", labelKey: "default_apps" },
  { id: "data", labelKey: "data_management" },
  { id: "tools", labelKey: "tools_settings" },
  { id: "about", labelKey: "about_settings" },
];

export const DEFAULT_SETTINGS_GROUP: SettingsGroupId = "general";

/**
 * Rail (160px, see `--settings-rail-width`) + gap + a detail pane still wider
 * than the 420px column it replaces.
 */
export const SPLIT_MIN_WIDTH = 560;

/** Detail pane reaches ~530px, which fits two ~250px rows. */
export const PANE_TWO_COLUMNS_MIN_WIDTH = 700;

/** Detail pane reaches ~830px, which fits three ~265px rows. */
export const PANE_THREE_COLUMNS_MIN_WIDTH = 1000;

export interface SettingsLayout {
  readonly split: boolean;
  readonly paneColumns: 1 | 2 | 3;
}

const STACK: SettingsLayout = { split: false, paneColumns: 1 };

export const resolveSettingsLayout = (width: number): SettingsLayout => {
  if (width < SPLIT_MIN_WIDTH) return STACK;
  if (width >= PANE_THREE_COLUMNS_MIN_WIDTH) return { split: true, paneColumns: 3 };
  if (width >= PANE_TWO_COLUMNS_MIN_WIDTH) return { split: true, paneColumns: 2 };
  return { split: true, paneColumns: 1 };
};

/**
 * Whether a group renders folded away, which is also the switch its data loads
 * hang off: `ToolsCaptureSection` mounts only while tools is open, and
 * `AppearanceSettingsGroup` starts its font scan only when `collapsed` is false.
 *
 * The narrow layout is the accordion, untouched. The split layout opens the one
 * group the rail is pointing at -- its header reads as a title, and `onToggle`
 * is a no-op there -- and leaves every other group folded the way the accordion
 * left it. Forcing all six open instead made hidden groups start work nobody
 * could see: on Linux each crossing back to a wide window respawned a
 * `tesseract --version` probe, while the settings page was still closed.
 *
 * `collapsed` is deliberately still honoured for a hidden group. Someone who
 * opened the export dialog in a narrow window keeps it, and its draft, after the
 * window is widened past the rail -- folding it away here is the same class of
 * bug as unmounting it.
 */
export const isSettingsGroupCollapsed = ({
  split,
  shown,
  collapsed,
}: {
  readonly split: boolean;
  readonly shown: boolean;
  readonly collapsed: boolean;
}): boolean => collapsed && !(split && shown);
