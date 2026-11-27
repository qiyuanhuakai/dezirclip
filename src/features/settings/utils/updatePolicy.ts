import { defaultUpdateChannel } from './defaultUpdateChannel';
import { updateAutoCheckSetting, updateChannelSetting } from '../../../shared/protocol/settings';

export type UpdateChannel = (typeof updateChannelSetting.values)[number];

export const readUpdatePreferences = (settings: Record<string, string>, version = '') => ({
    autoCheck: settings[updateAutoCheckSetting.key] === "true",
    channel: defaultUpdateChannel(version, settings[updateChannelSetting.key]),
});

export const updateErrorKey = (error: unknown): string => {
    const code = error instanceof Error ? error.message : String(error);
    switch (code) {
        case "update_check_failed": return "updater_error_network";
        case "update_server_failed": return "updater_error_server";
        case "update_channel_not_published": return "updater_error_unpublished";
        case "update_invalid_manifest":
        case "update_invalid_stable_manifest":
        case "update_invalid_version":
        case "update_invalid_endpoint": return "updater_error_metadata";
        case "update_untrusted_download":
        case "update_invalid_signature":
        case "update_signature_version_mismatch": return "updater_error_verification";
        case "update_download_or_signature_failed": return "updater_error_download_verify";
        case "update_install_failed": return "updater_error_install";
        case "update_participants_not_ready": return "updater_error_participants";
        case "update_busy": return "updater_error_busy";
        case "update_signing_not_configured": return "updater_error_signing";
        case "update_check_required": return "updater_error_recheck";
        case "update_attempt_storage_failed": return "updater_error_storage";
        case "update_installation_unsupported": return "updater_error_support";
        default: return "updater_error_unknown";
    }
};
