import { describe, it, expect, vi } from "vitest";
import { render, waitFor } from "@testing-library/react";
import AppMainContent from "./AppMainContent";
import type { ComponentProps } from "react";
import type { SettingsPanelProps } from "../../settings/components/SettingsPanel.types";
import type { ClipboardEntry } from "../../../shared/types";

type AppMainContentProps = ComponentProps<typeof AppMainContent>;

const mocks = vi.hoisted(() => ({
  settingsPanelSpy: vi.fn(() => null),
  virtualListSpy: vi.fn(() => null),
  tagManagerStub: vi.fn(() => null),
  emojiPanelStub: vi.fn(() => null)
}));

vi.mock("../../settings/components/SettingsPanel", async () => {
  const React = await import("react");
  function SettingsPanelMock() {
    mocks.settingsPanelSpy();
    return React.createElement("div", { "data-testid": "settings-panel" });
  }
  return { default: React.memo(SettingsPanelMock) };
});

vi.mock("../../clipboard/components/VirtualClipboardList", async () => {
  const React = await import("react");
  return {
    VirtualClipboardList: function VirtualClipboardListMock() {
      mocks.virtualListSpy();
      return React.createElement("div", { "data-testid": "virtual-list" });
    }
  };
});

vi.mock("../../tag/components/TagManager", async () => {
  const React = await import("react");
  return {
    default: function TagManagerMock() {
      mocks.tagManagerStub();
      return React.createElement("div", { "data-testid": "tag-manager" });
    }
  };
});

vi.mock("../../emoji/components/EmojiPanel", async () => {
  const React = await import("react");
  return {
    default: function EmojiPanelMock() {
      mocks.emojiPanelStub();
      return React.createElement("div", { "data-testid": "emoji-panel" });
    }
  };
});

function makeSettingsPanelProps(): SettingsPanelProps {
  const noop = () => {};
  const noopAsync = async () => true;
  return {
    t: (k: string) => k,
    theme: "default",
    language: "zh" as const,
    colorMode: "light",
    clipboardItemFontSize: 14,
    setClipboardItemFontSize: noop,
    clipboardTagFontSize: 12,
    setClipboardTagFontSize: noop,
    fontMain: "",
    setFontMain: noop,
    fontMono: "",
    setFontMono: noop,
    collapsedGroups: {},
    autoStart: false,
    silentStart: false,
    persistent: true,
    persistentLimitEnabled: false,
    persistentLimit: 1000,
    deduplicate: false,
    captureFiles: true,
    captureRichText: true,
    richTextSnapshotPreview: false,
    deleteAfterPaste: false,
    moveToTopAfterPaste: true,
    sequentialMode: false,
    sequentialHotkey: "",
    isRecordingSequential: false,
    richPasteHotkey: "",
    isRecordingRich: false,
    searchHotkey: "",
    isRecordingSearch: false,
    privacyProtection: false,
    privacyProtectionKinds: [],
    setPrivacyProtectionKinds: noop,
    privacyProtectionCustomRules: "",
    setPrivacyProtectionCustomRules: noop,
    hotkey: "",
    registryWinVEnabled: false,
    setRegistryWinVEnabled: noop,
    showSearchBox: true,
    setShowSearchBox: noop,
    scrollTopButtonEnabled: false,
    setScrollTopButtonEnabled: noop,
    emojiPanelEnabled: false,
    setEmojiPanelEnabled: noop,
    tagManagerEnabled: false,
    setTagManagerEnabled: noop,
    arrowKeySelection: false,
    setArrowKeySelection: noop,
    soundEnabled: false,
    setSoundEnabled: noop,
    soundVolume: 0,
    setSoundVolume: noop,
    pasteSoundEnabled: false,
    setPasteSoundEnabled: noop,
    pasteMethod: "",
    setPasteMethod: noop,
    hideTrayIcon: false,
    setHideTrayIcon: noop,
    edgeDocking: false,
    setEdgeDocking: noop,
    followMouse: false,
    setFollowMouse: noop,
    disableWebviewGpu: false,
    setDisableWebviewGpu: noop,
    idleDestroyEnabled: false,
    setIdleDestroyEnabled: noop,
    idleDestroySeconds: 60,
    setIdleDestroySeconds: noop,
    customBackground: "",
    setCustomBackground: noop,
    customBackgroundOpacity: 1,
    setCustomBackgroundOpacity: noop,
    surfaceOpacity: 1,
    setSurfaceOpacity: noop,
    installedApps: [],
    appSettings: {},
    defaultApps: {},
    dataPath: "",
    toggleGroup: noop,
    setAutoStart: noop,
    setSilentStart: noop,
    setPersistent: noop,
    setPersistentLimitEnabled: noop,
    setPersistentLimit: noop,
    setDeduplicate: noop,
    setCaptureFiles: noop,
    setCaptureRichText: noop,
    setRichTextSnapshotPreview: noop,
    setDeleteAfterPaste: noop,
    setMoveToTopAfterPaste: noop,
    saveAppSetting: noop,
    setSequentialModeState: noop,
    setIsRecordingSequential: noop,
    updateSequentialHotkey: noop,
    setIsRecordingRich: noop,
    updateRichPasteHotkey: noop,
    setIsRecordingSearch: noop,
    updateSearchHotkey: noop,
    setPrivacyProtection: noop,
    setIsRecording: noop,
    isRecording: false,
    mainHotkeys: [],
    updateHotkey: noop,
    addMainHotkey: noopAsync,
    removeMainHotkey: noopAsync,
    setTheme: noop,
    setColorMode: noop,
    setLanguage: noop,
    showAppBorder: false,
    setShowAppBorder: noop,
    compactMode: false,
    setCompactMode: noop,
    checkHotkeyConflict: () => false,
    handleResetSettings: noop
  };
}

function makeEntry(id: number, content: string): ClipboardEntry {
  return {
    id,
    content_type: "text",
    content,
    source_app: "test",
    timestamp: Date.now(),
    preview: content,
    is_pinned: false,
    tags: []
  };
}

const stableSettingsPanelProps = makeSettingsPanelProps();

function makeBaseProps(overrides: Partial<AppMainContentProps> = {}): AppMainContentProps {
  const noopRender: AppMainContentProps["renderItemContent"] = () => null;
  const noopFn = () => {};
  return {
    t: (k: string) => k,
    theme: "default",
    showSettings: false,
    showTagManager: false,
    tagManagerEnabled: true,
    showEmojiPanel: false,
    settingsPanelProps: stableSettingsPanelProps,
    emojiFavorites: [],
    setEmojiFavorites: noopFn as AppMainContentProps["setEmojiFavorites"],
    emojiPanelTab: "emoji",
    setEmojiPanelTab: noopFn as AppMainContentProps["setEmojiPanelTab"],
    saveSetting: noopFn,
    filteredHistory: [],
    search: "",
    pinnedItems: [],
    unpinnedItems: [],
    compactMode: false,
    selectedIndex: 0,
    isKeyboardMode: false,
    virtualListRef: { current: null },
    handlePinnedReorder: noopFn,
    renderItemContent: noopRender,
    loadMoreHistory: noopFn,
    handleListScroll: noopFn,
    hasMore: false,
    isLoadingMore: false,
    showScrollTop: false,
    onScrollTop: noopFn,
    ...overrides
  };
}

describe("AppMainContent", () => {
  it("keeps SettingsPanel mounted when toggling showSettings", async () => {
    mocks.settingsPanelSpy.mockClear();
    mocks.virtualListSpy.mockClear();

    const history: ClipboardEntry[] = [makeEntry(1, "hello")];

    const { rerender } = render(
      <AppMainContent {...makeBaseProps({ showSettings: true, filteredHistory: history })} />
    );

    await waitFor(() => {
      expect(document.querySelector('[data-testid="settings-panel"]')).not.toBeNull();
    });

    const initialCount = mocks.settingsPanelSpy.mock.calls.length;
    expect(initialCount).toBe(1);

    for (let i = 0; i < 20; i += 1) {
      rerender(
        <AppMainContent
          {...makeBaseProps({ showSettings: i % 2 === 0, filteredHistory: history })}
        />
      );
    }

    expect(mocks.settingsPanelSpy.mock.calls.length).toBe(1);
    expect(document.querySelector('[data-testid="settings-panel"]')).not.toBeNull();
    expect(document.querySelector('[data-testid="virtual-list"]')).not.toBeNull();
  });

  it("keeps VirtualClipboardList in the DOM when toggling showSettings", async () => {
    mocks.settingsPanelSpy.mockClear();
    mocks.virtualListSpy.mockClear();

    const history: ClipboardEntry[] = [makeEntry(1, "hello")];

    const { rerender } = render(
      <AppMainContent {...makeBaseProps({ showSettings: true, filteredHistory: history })} />
    );

    await waitFor(() => {
      expect(document.querySelector('[data-testid="settings-panel"]')).not.toBeNull();
    });

    expect(document.querySelector('[data-testid="virtual-list"]')).not.toBeNull();

    rerender(
      <AppMainContent {...makeBaseProps({ showSettings: false, filteredHistory: history })} />
    );
    expect(document.querySelector('[data-testid="virtual-list"]')).not.toBeNull();

    rerender(
      <AppMainContent {...makeBaseProps({ showSettings: true, filteredHistory: history })} />
    );
    expect(document.querySelector('[data-testid="virtual-list"]')).not.toBeNull();

    for (let i = 0; i < 20; i += 1) {
      rerender(
        <AppMainContent
          {...makeBaseProps({ showSettings: i % 2 === 0, filteredHistory: history })}
        />
      );
    }
    expect(document.querySelector('[data-testid="virtual-list"]')).not.toBeNull();
    expect(document.querySelector('[data-testid="settings-panel"]')).not.toBeNull();
  });

  it("applies display:none to the hidden branch via inline style", async () => {
    mocks.settingsPanelSpy.mockClear();
    mocks.virtualListSpy.mockClear();

    const history: ClipboardEntry[] = [makeEntry(1, "hello")];

    const { rerender } = render(
      <AppMainContent {...makeBaseProps({ showSettings: true, filteredHistory: history })} />
    );

    await waitFor(() => {
      expect(document.querySelector('[data-testid="settings-panel"]')).not.toBeNull();
    });

    const settingsView = document.querySelector(".settings-view") as HTMLElement | null;
    const listView = document.querySelector(".list-view") as HTMLElement | null;
    expect(settingsView?.style.display).toBe("flex");
    expect(listView?.style.display).toBe("none");

    rerender(
      <AppMainContent {...makeBaseProps({ showSettings: false, filteredHistory: history })} />
    );

    expect(document.querySelector(".settings-view")?.getAttribute("style")).toContain("display: none");
    expect(document.querySelector(".list-view")?.getAttribute("style")).toContain("display: flex");
  });
});