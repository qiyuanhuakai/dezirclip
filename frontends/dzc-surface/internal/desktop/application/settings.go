package application

import (
	"context"
	"errors"

	"github.com/qiyuanhuakai/dezirclip/clients/go/client"
	"github.com/qiyuanhuakai/dezirclip/clients/go/protocol"
)

func SetSetting(ctx context.Context, host *client.Client, key, value string, epoch uint64) error {
	instance := host.Hello().State.ServiceInstanceId
	var result protocol.OperationResult
	var err error
	switch key {
	case "app.text", "app.rich_text", "app.image", "app.video", "app.code", "app.url":
		params := protocol.ApplicationSelection{ServiceInstanceId: instance, ExpectedEpoch: epoch, Key: key, Value: value}
		err = host.DesktopCall(ctx, protocol.MethodDesktopApplicationsSelect, params, "OperationResult", &result)
	case "app.use_win_v_shortcut", "app.theme", "app.color_mode", "app.language", "app.compact_mode", "app.show_app_border",
		"app.show_scroll_top_button", "app.show_search_box", "app.tag_manager_enabled", "app.rich_text_snapshot_preview", "app.rich_text_snapshot_cache_limit", "app.arrow_key_selection", "app.preview_keyboard_enabled", protocol.SettingUpdateAutoCheck, protocol.SettingUpdateChannel, "app.silent_start", "app.hide_tray_icon", "app.edge_docking", "app.follow_mouse", "app.disable_webview_gpu", "app.idle_destroy_enabled", "app.idle_destroy_seconds", "app.quick_paste_enabled", protocol.SettingQuickPasteLayout, "app.quick_paste_full_sectors", "app.quick_paste_half_sectors", "app.screenshot_enabled", "app.sound_enabled", "app.sound_paste_enabled", "app.sound_volume",
		"app.clipboard_item_font_size", "app.clipboard_tag_font_size", "app.surface_opacity", "app.preview_opacity", "app.font_main", "app.font_mono",
		"app.custom_background", "app.custom_background_opacity", "app.emoji_panel_enabled", "app.emoji_panel_tab", "app.emoji_favorites":
		params := protocol.ViewSettingParams{ServiceInstanceId: instance, ExpectedEpoch: epoch, Key: key, Value: value}
		err = host.DesktopCall(ctx, protocol.MethodDesktopSettingsViewSet, params, "OperationResult", &result)
	case "app.sequential_mode", "app.delete_after_paste", "app.move_to_top_after_paste", "app.paste_method":
		params := protocol.QueueSettingParams{ServiceInstanceId: instance, ExpectedEpoch: epoch, Key: key, Value: value}
		err = host.DesktopCall(ctx, protocol.MethodDesktopQueueConfigure, params, "OperationResult", &result)
	case "app.persistent", "app.persistent_limit_enabled", "app.persistent_limit", "app.deduplicate", "app.capture_files", "app.capture_rich_text", "app.privacy_protection",
		"app.privacy_protection_kinds", "app.privacy_protection_custom_rules", "app.excluded_source_apps", "app.ocr_enabled":
		params := protocol.CaptureSettingParams{ServiceInstanceId: instance, ExpectedEpoch: epoch, Key: key, Value: value}
		err = host.DesktopCall(ctx, protocol.MethodDesktopSettingsCaptureSet, params, "OperationResult", &result)
	default:
		return errors.New("UnsupportedDesktopSetting")
	}
	if err == nil && !result.Performed {
		return client.ErrProtocol
	}
	return err
}
