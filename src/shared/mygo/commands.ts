import { parsePrivacySnapshot } from "../lib/privacyRuntime";
import { readResumeAuthority } from "../lib/mainWindowResume";
import { emit, hostGeneration } from "./events";
import { shellCall } from "./bridge";
import { getCustomThemes } from "../lib/themeCatalog";
import { resolveThemeSelection } from "../lib/themePackage";
import { getPrivacyViewState } from "../lib/privacySession";
import type { TagBatchSelection } from "../../features/tag/utils/tagBatchSelection";

type Arguments = Record<string, unknown>;

function privacySnapshot(status: { privacy: { expires_at_ms: number | null } }) {
  const stamp = readResumeAuthority(status);
  return parsePrivacySnapshot({ history_revision: stamp.revision, privacy_epoch: stamp.epoch,
    session: { active: stamp.sessionActive, expires_at_ms: status.privacy.expires_at_ms } });
}

const library = <T>(method: string, ...args: unknown[]) => window.mygo.call<T>(`Library.${method}`, ...args);

let defaultAppsQuery: Promise<Record<string, string>> | undefined;
function systemDefaultApps() {
  return defaultAppsQuery ??= shellCall<Record<string, string>>("PackageTool", "default-apps")
    .finally(() => { defaultAppsQuery = undefined; });
}

function currentEpoch() {
  const view = getPrivacyViewState();
  if (view.phase !== "ready" || !view.snapshot || view.pending) throw new Error("PrivacyStateUnavailable");
  return view.snapshot.privacy_epoch;
}

function mutateEntry(args: Arguments, action: string, fields: Arguments = {}) {
  const epoch = currentEpoch();
  if (args.expectedPrivacyEpoch !== undefined && args.expectedPrivacyEpoch !== epoch) throw new Error("PrivacyEpochChanged");
  if ((args.allowSensitive || args.confirmPersistence) && args.expectedPrivacyEpoch === undefined) throw new Error("PrivacyEpochRequired");
  return library<number>("MutateEntry", { action, id: args.id, service_instance_id: args.expectedHostInstanceId,
    expected_epoch: epoch, reveal: args.allowSensitive ?? false, confirm_persistence: args.confirmPersistence, ...fields });
}

const commands: Record<string, (args: Arguments) => Promise<unknown>> = {
  get_main_window_resume_request: () => shellCall("ResumeRequest"),
  complete_main_window_resume: args => shellCall("CompleteResume", args.requestId, args.expectedHostInstanceId, args.expectedEpoch),
  set_ignore_blur: args => shellCall("SetIgnoreBlur", args.ignore),
  check_is_admin: () => window.mygo.call("SystemSettings.IsAdmin"),
  restart_explorer: () => window.mygo.call("SystemSettings.RestartExplorer"),
  is_registry_win_v_optimized: () => window.mygo.call("SystemSettings.WinVStatus"),
  begin_registry_win_v_change: async args => {
    const result = await window.mygo.call<{value: boolean; token: string}>("SystemSettings.BeginWinV", args.enable);
    return [result.value, result.token];
  },
  finish_registry_win_v_change: args => window.mygo.call("SystemSettings.FinishWinV", args.token, args.rollback),
  set_windows_clipboard_history: args => window.mygo.call("SystemSettings.SetClipboardHistory", args.enabled),
  is_window_topmost: args => {
    if (args.label !== "compact-preview") throw new Error(`Unknown MyGo preview: ${String(args.label)}`);
    return window.mygo.call("Preview.IsAlwaysOnTop");
  },
  test_hotkey_available: args => shellCall("TestHotkeyAvailable", args.hotkey),
  register_hotkey: args => shellCall("SetMainHotkeys", args.hotkey, currentEpoch()),
  set_search_hotkey: args => shellCall("SetSearchHotkey", args.hotkey, currentEpoch()),
  set_sequential_hotkey: args => shellCall("SetPasteHotkey", "app.sequential_hotkey", args.hotkey, currentEpoch()),
  set_rich_paste_hotkey: args => shellCall("SetPasteHotkey", "app.rich_paste_hotkey", args.hotkey, currentEpoch()),
  set_recording_mode: args => shellCall("SetRecordingMode", args.enabled, args.enabled ? currentEpoch() : 0),
  set_edge_docking: args => shellCall("SetEdgeDocking", args.enabled, currentEpoch()),
  set_tray_visible: args => shellCall("SetTrayVisible", args.visible, currentEpoch()),
  set_sound_enabled: args => library("SetSetting", "app.sound_enabled", String(args.enabled), currentEpoch()),
  set_idle_destroy_enabled: args => library("SetSetting", "app.idle_destroy_enabled", String(args.enabled), currentEpoch()),
  set_idle_destroy_seconds: args => library("SetSetting", "app.idle_destroy_seconds", String(args.seconds), currentEpoch()),
  set_follow_mouse: args => library("SetSetting", "app.follow_mouse", String(args.enabled), currentEpoch()),
  set_silent_start: args => library("SetSetting", "app.silent_start", String(args.enabled), currentEpoch()),
  open_content: args => {
    const epoch = currentEpoch();
    if (args.expectedPrivacyEpoch !== undefined && args.expectedPrivacyEpoch !== epoch) throw new Error("PrivacyEpochChanged");
    if (typeof args.id === "number" && args.id < 0) throw new Error("SessionExternalOpenUnavailable");
    return library("OpenEntry", { id: args.id, service_instance_id: args.expectedHostInstanceId,
      expected_epoch: epoch, reveal: args.expectedPrivacyEpoch !== undefined });
  },
  get_file_size: async args => ({ size: await shellCall<number>("FileSize", args.path) }),
  get_custom_background_resource: async args => {
    const src = await library<string | null>("Background", currentEpoch(), args.expectedPath);
    if (!src) throw "CustomBackgroundPending";
    return src;
  },
  is_custom_background_selected: args => library<boolean>("BackgroundSelected", currentEpoch(), args.expectedPath),
  show_region_selector: () => shellCall("Capture", false, currentEpoch()),
  capture_full_screen: async () => {
    await shellCall("Capture", true, currentEpoch());
    await emit("toast", "screenshot_captured");
  },
  get_platform_info: () => shellCall("PlatformInfo"),
  list_system_fonts: () => library("SystemFonts"),
  send_emoji: args => window.mygo.call("History.SendEmoji", args.contentType, args.content, args.viewEpoch, args.paste),
  save_emoji_favorite: args => library("MutateFavorite", currentEpoch(), "import", args.sourcePath),
  save_emoji_favorite_data_url: args => library("ImportFavoriteData", currentEpoch(), args.dataUrl),
  remove_emoji_favorite: args => library("MutateFavorite", currentEpoch(), "remove", args.path),
  list_emoji_favorites: async () => (await library<{ paths: string[] }>("EmojiFavorites", currentEpoch(), null)).paths,
  get_emoji_favorite_resource: async args => {
    const result = await library<{ preview?: string }>("EmojiFavorites", currentEpoch(), args.path);
    if (!result.preview) throw new Error("UserResourceUnavailable");
    return result.preview;
  },
  get_system_theme_mode: () => shellCall("SystemThemeMode"),
  set_theme: args => shellCall("ApplyAppearance", resolveThemeSelection(String(args.theme), getCustomThemes()).base,
    args.color_mode ?? args.colorMode ?? "system", args.show_app_border ?? args.showAppBorder ?? true,
    window.matchMedia("(prefers-color-scheme: dark)").matches),
  classify_text: args => library("Classify", args.text),
  transform_text: args => library("Transform", { text: args.text, kind: args.kind }),
  generate_qr_png: args => library("QR", { content: args.content, format: "png", size_px: args.sizePx }),
  generate_qr_svg: args => library("QR", { content: args.content, format: "svg" }),
  recognize_clipboard_image: async args => {
    const result = await library<{ status: string; text?: string }>("OcrText", true, { id: args.itemId,
      service_instance_id: args.expectedHostInstanceId, expected_epoch: args.expectedPrivacyEpoch, reveal: args.allowSensitive === true });
    if (result.text === undefined) throw new Error("OcrRecognitionFailed");
    return { item_id: args.itemId, text: result.text, status: result.status, confidence: null };
  },
  get_ocr_status: async args => {
    const result = await library<{ status: string; text?: string }>("OcrText", false, { id: args.itemId,
      service_instance_id: args.expectedHostInstanceId, expected_epoch: args.expectedEpoch ?? currentEpoch() });
    return { item_id: args.itemId, ocr_status: result.status, ocr_text: result.text ?? null };
  },
  set_window_pinned: async args => {
    await library("SetWindowPinned", args.pinned, currentEpoch());
    return window.mygo.call("DesktopShell.SetAlwaysOnTop", args.pinned);
  },
  delete_clipboard_entry: args => mutateEntry(args, "delete"),
  toggle_clipboard_pin: args => mutateEntry(args, "pin", { pinned: args.isPinned }),
  update_tags: args => mutateEntry(args, "tags", { tags: args.tags }),
  update_item_content: args => mutateEntry(args, "edit", { content: args.newContent }),
  add_manual_item: args => {
    const epoch = currentEpoch();
    if (args.expectedPrivacyEpoch !== epoch) throw new Error("PrivacyEpochChanged");
    return library("AddManual", args.content, { expected_epoch: epoch, content_type: args.contentType,
      tags: args.tags, confirm_persistence: args.confirmPersistence ?? false });
  },
  get_tag_colors: () => library("TagColors", currentEpoch()),
  create_new_tag: args => library("EditTag", "create", args.tagName, "", currentEpoch()),
  remove_tag_from_all: args => library("EditTag", "remove", args.tagName, "", currentEpoch()),
  rename_tag_globally: args => library("EditTag", "rename", args.oldName, args.newName, currentEpoch()),
  get_paste_queue: () => library("PasteQueue", currentEpoch()),
  clear_paste_queue: () => library("SetPasteQueue", currentEpoch(), []),
  set_paste_queue: args => library("SetPasteQueue", currentEpoch(), args.itemIds),
  set_sequential_mode: args => library("SetSetting", "app.sequential_mode", String(args.enabled), currentEpoch()),
  preview_backup_file: args => library("PreviewBackup", currentEpoch(), args.path),
  import_from_file: args => library("ImportBackup", currentEpoch(), args.path, args.mode, args.passphrase ?? null),
  export_to_file: args => library("ExportBackup", currentEpoch(), args.path, args.format, args.passphrase ?? null),
  clear_clipboard_history: () => library("ClearHistory", currentEpoch()),
  update_pinned_order: args => library("ReorderPins", args.orders, currentEpoch()),
  delete_sensitive_tag_with_contents: args => library("DeleteTagWithContents", args.tagName, currentEpoch()),
  delete_tag_entries: async args => {
    const selection = args.selection as TagBatchSelection;
    if (selection.mode !== "selected" && selection.mode !== "all") throw new Error("InvalidSelection");
    const deletedCount = await library<number>("DeleteTagEntries", { name: args.tagName, expected_epoch: currentEpoch(),
      scope: selection.mode, ids: selection.mode === "selected" ? selection.ids : selection.excludedIds,
      query: selection.mode === "all" ? selection.query : undefined });
    return { deletedCount };
  },
  get_settings: () => library("Preferences"),
  get_custom_themes: async () => {
    const preferences = await library<Record<string, string>>("Preferences");
    return JSON.parse(preferences["app.custom_themes"] || "[]");
  },
  import_custom_theme: args => library("ImportTheme", args.path, currentEpoch()),
  export_custom_theme: args => library("ExportTheme", args.id, args.path, currentEpoch()),
  delete_custom_theme: args => library("RemoveTheme", args.id, currentEpoch()),
  save_setting: args => {
    if (args.key === "app.search_hotkey") return shellCall("SetSearchHotkey", args.value, currentEpoch());
    if (args.key === "app.sequential_hotkey" || args.key === "app.rich_paste_hotkey") return shellCall("SetPasteHotkey", args.key, args.value, currentEpoch());
    if (["app.quick_paste_hotkey", "app.screenshot_hotkey", "app.screenshot_full_hotkey"].includes(String(args.key))) {
      return shellCall("SetPanelHotkey", args.key, args.value, currentEpoch());
    }
    if (args.key === "app.hotkey") return shellCall("SetMainHotkeys", args.value, currentEpoch());
    if (args.key === "app.quick_paste_enabled" || args.key === "app.screenshot_enabled") {
      if (args.value !== "true" && args.value !== "false") throw new Error("ViewSettingInvalid");
      return shellCall("SetNativePanelEnabled", args.key === "app.quick_paste_enabled" ? "quick-paste" : "screenshot", args.value === "true", currentEpoch());
    }
    if (args.key !== "app.disable_webview_gpu") return library("SetSetting", args.key, args.value, currentEpoch());
    if (args.value !== "true" && args.value !== "false") throw new Error("ViewSettingInvalid");
    return shellCall("SetWebviewGpuDisabled", args.value === "true", currentEpoch());
  },
  copy_to_clipboard: args => window.mygo.call("History.CopyEntry", {
    id: args.id === 0 ? args.derivedFromId : args.id,
    service_instance_id: args.expectedHostInstanceId,
    expected_epoch: args.expectedPrivacyEpoch ?? args.viewEpoch,
    reveal: args.allowSensitive ?? (args.expectedPrivacyEpoch !== undefined),
    paste: args.paste, with_format: args.pasteWithFormat, image_as_base64: args.pasteImageAsBase64,
    derived_text: args.id === 0 && args.derivedFromId !== undefined ? args.content : undefined,
    move_to_top: args.moveToTop, delete_after_use: args.deleteAfterUse,
  }),
  get_clipboard_host_status: () => library("Status"),
  get_privacy_status: async () => privacySnapshot(await library("Status")),
  set_privacy_protection: args => library("SetSetting", "app.privacy_protection", String(args.enabled), currentEpoch()),
  set_privacy_protection_kinds: args => {
    if (!Array.isArray(args.kinds) || args.kinds.some(kind => typeof kind !== "string" || kind.includes(","))) throw new Error("InvalidPrivacyKinds");
    return library("SetSetting", "app.privacy_protection_kinds", args.kinds.join(","), currentEpoch());
  },
  set_privacy_protection_custom_rules: args => library("SetSetting", "app.privacy_protection_custom_rules", args.rules, currentEpoch()),
  get_privacy_rules: () => library("PolicyRules", currentEpoch()),
  get_privacy_decision: args => library("PolicyDecision", { id: args.id, service_instance_id: args.expectedHostInstanceId, expected_epoch: args.expectedEpoch }),
  privacy_feedback: async args => privacySnapshot(await library("PolicyFeedback", { id: args.id, action: args.action,
    service_instance_id: args.expectedHostInstanceId, expected_epoch: args.expectedEpoch })),
  remove_privacy_rule: async args => privacySnapshot(await library("RemovePolicyRule", args.ruleId, args.expectedEpoch)),
  set_privacy_session: async args => privacySnapshot(await library("SetPrivacy", {
    active: args.enabled, duration_ms: args.durationMs ?? undefined,
  })),
  set_privacy_session_hotkey: args => library("SetPrivacyHotkey", args.hotkey),
  get_clipboard_history: args => library("Page", { limit: args.limit, offset: args.offset, kind: args.content_type }, args.expectedEpoch),
  search_clipboard_history: args => library("Page", { limit: args.limit, offset: 0, query: args.searchTerm }, args.expectedEpoch),
  get_all_tags_info: args => library("TagCounts", args.expectedEpoch),
  get_captured_entry_summary: args => library("Summary", { id: args.id, expected_epoch: args.expectedEpoch,
    service_instance_id: args.expectedHostInstanceId }),
  get_tag_items_page: args => library("TagPage", { tag: args.tag, query: args.query ?? "", sort_by: args.sortBy ?? "time",
    limit: args.limit ?? 60, offset: args.offset ?? 0, expected_epoch: args.expectedEpoch }),
  get_privacy_entry: args => library("Entry", { id: args.id, expected_epoch: args.expectedEpoch,
    service_instance_id: args.expectedHostInstanceId, reveal: args.reveal ?? true }),
  get_source_app_icon: args => library("SourceIcon", { id: args.id, expected_epoch: args.expectedEpoch,
    service_instance_id: args.expectedHostInstanceId, reveal: false }),
  get_history_resource_preview: args => library("Entry", { id: args.id, expected_epoch: args.expectedEpoch,
    service_instance_id: args.expectedHostInstanceId, reveal: false,
    thumbnail_dimension: Math.min(1024, Math.max(64, Number(args.maxDimension ?? 384))) }),
};

export async function invokeMyGo<T>(command: string, args: Arguments = {}): Promise<T> {
  if (command === "restart_as_admin") return shellCall<T>("RestartAsAdmin");
  if (command === "reset_settings") return shellCall<T>("ResetSettings", currentEpoch());
  if (command === "get_update_support") return shellCall<T>("PackageTool", "update-support");
  if (command === "check_for_update") return shellCall<T>("CheckForUpdate", args.channel);
  if (command === "set_data_path") return shellCall<T>("MoveProfile", args.newPath, currentEpoch());
  if (command === "relaunch") return shellCall<T>("RelaunchProfile");
  if (command === "scan_installed_apps") return shellCall<T>("PackageTool", "installed-apps");
  if (command === "get_system_default_app") {
    const names = await systemDefaultApps();
    return (names[String(args.contentType)] ?? "") as T;
  }
  if (command === "is_autostart_enabled") return shellCall<T>("PackageTool", "login-status");
  if (command === "toggle_autostart") return shellCall<T>("PackageTool", args.enabled ? "enable-login" : "disable-login");
  if (command === "get_cli_info") return shellCall<T>("PackageTool", "info");
  if (command === "add_cli_to_path") return shellCall<T>("PackageTool", "add-to-path");
  if (command === "open_install_folder") return shellCall<T>("PackageTool", "open-install-folder");
  if (command === "open_agent_skill_folder") {
    if (args.skill !== "dzc-cli" && args.skill !== "dezirclip-theme") throw new Error("Unknown bundled guide");
    return shellCall<T>("PackageTool", args.skill === "dzc-cli" ? "open-cli-guide" : "open-theme-guide");
  }
  if (command === "get_data_path") return (await library<{data_directory: T}>("ProfileInfo")).data_directory;
  if (command === "open_data_folder") return library<T>("OpenDataFolder");
  if (command === "get_build_info") {
    const [desktop, profile] = await Promise.all([
      shellCall<{ mygo_version: string; build_id: string; source_revision: string }>("BuildInfo"),
      library<{ database_schema_version: number }>("ProfileInfo"),
    ]);
    return { ...desktop, build_id: desktop.build_id === "unknown" ? null : desktop.build_id,
      source_revision: desktop.source_revision === "unknown" ? null : desktop.source_revision,
      database_schema_version: profile.database_schema_version,
      linked_components: [{ component: "MyGo", version: desktop.mygo_version }] } as T;
  }
  if (command === "restore_window_pinned") return window.mygo.call<T>("DesktopShell.SetAlwaysOnTop", args.pinned);
  if (command === "activate_window_focus" || command === "focus_clipboard_window") return window.mygo.call<T>("DesktopShell.Focus");
  if (command === "hide_window_cmd") return args.afterPaste ? window.mygo.call<T>("mygo:window.Close") : shellCall<T>("HideMain");
  if (command === "toggle_window_cmd") return window.mygo.call<T>("mygo:window.Close");
  if (command === "get_platform_capabilities") return library<T>("Platform");
  if (command === "check_ocr_engine_available") return library<T>("OcrEngine");
  if (command === "list_transform_kinds" || command === "get_supported_kinds") {
    const catalog = await library<{ transforms: T; content_kinds: T }>("ToolCatalog");
    return command === "list_transform_kinds" ? catalog.transforms : catalog.content_kinds;
  }
  if (!Object.prototype.hasOwnProperty.call(commands, command)) throw new Error(`Unsupported MyGo desktop command: ${command}`);
  const generation = hostGeneration();
  const result = await commands[command](args);
  if (!["set_privacy_session", "privacy_feedback", "remove_privacy_rule", "set_privacy_protection", "set_privacy_protection_kinds", "set_privacy_protection_custom_rules", "copy_to_clipboard", "send_emoji"].includes(command) && hostGeneration() !== generation) throw new Error("PrivacyEpochChanged");
  return result as T;
}
