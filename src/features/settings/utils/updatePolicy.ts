import { defaultUpdateChannel } from './defaultUpdateChannel';

export type UpdateChannel = "stable" | "preview";
export const AUTO_UPDATE_INTERVAL_MS = 24 * 60 * 60 * 1000;
export const AUTO_UPDATE_DELAY_MS = 15_000;
export const updateAttemptKey = (channel: UpdateChannel) => `dezirclip.update.lastAttempt.${channel}`;

export const readUpdatePreferences = (settings: Record<string, string>, version = '') => ({
    autoCheck: settings["app.update_auto_check"] === "true",
    channel: defaultUpdateChannel(version, settings["app.update_channel"]),
});

export const automaticCheckDue = (stored: string | null, now: number): boolean => {
    if (stored === null) return true;
    const previous = Number(stored);
    return Number.isFinite(previous) && previous >= 0 && now - previous >= AUTO_UPDATE_INTERVAL_MS;
};

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
        case "update_installation_unsupported": return "updater_error_support";
        default: return "updater_error_unknown";
    }
};
