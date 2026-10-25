import { getVersion } from "@tauri-apps/api/app";
import { Channel, invoke } from "@tauri-apps/api/core";
import { openUrl } from "@tauri-apps/plugin-opener";
import { AUTO_UPDATE_DELAY_MS, automaticCheckDue, readUpdatePreferences, updateAttemptKey, updateErrorKey } from "./updatePolicy";
import type { UpdateChannel } from "./updatePolicy";

interface UpdateSupport {
    installation: "nsis" | "app_image" | "app_image_read_only" | "package_manager" | "portable" | "unpackaged";
    canInstall: boolean;
    reason: string;
    releasesUrl: string;
}

interface AvailableUpdate { id: string; version: string; notes: string | null; releaseUrl: string }
interface UpdateCheck {
    currentVersion: string;
    channel: UpdateChannel;
    support: UpdateSupport;
    available: AvailableUpdate | null;
    unpublishedChannels: UpdateChannel[];
}

type UpdateProgress =
    | { event: "downloading"; downloaded: number; total: number | null }
    | { event: "verified" }
    | { event: "installing" };

export interface UpdaterSnapshot {
    ready: boolean;
    currentVersion: string;
    autoCheck: boolean;
    channel: UpdateChannel;
    saving: boolean;
    phase: "idle" | "checking" | "available" | "current" | "downloading" | "verified" | "installing" | "error";
    support: UpdateSupport | null;
    available: AvailableUpdate | null;
    unpublishedChannels: UpdateChannel[];
    downloaded: number;
    total: number | null;
    verified: boolean;
    checkedAt: number | null;
    errorKey: string | null;
}

let snapshot: UpdaterSnapshot = { ready: false, currentVersion: "", autoCheck: false,
    channel: "stable", saving: false, phase: "idle", support: null, available: null,
    unpublishedChannels: [], downloaded: 0, total: null, verified: false, checkedAt: null, errorKey: null };
const subscribers = new Set<() => void>();
let initialized = false;
let automaticTimer: ReturnType<typeof setTimeout> | undefined;
let checking = false;
let installing = false;

const updateSnapshot = (changes: Partial<UpdaterSnapshot>) => {
    snapshot = { ...snapshot, ...changes };
    subscribers.forEach(subscriber => subscriber());
};
export const getUpdaterSnapshot = () => snapshot;
export const subscribeUpdater = (subscriber: () => void) => {
    subscribers.add(subscriber);
    return () => { subscribers.delete(subscriber); };
};

const recordAttempt = (channel: UpdateChannel) => {
    try { localStorage.setItem(updateAttemptKey(channel), String(Date.now())); return true; }
    catch { return false; }
};

const scheduleAutomaticCheck = () => {
    clearTimeout(automaticTimer);
    if (!snapshot.autoCheck || !snapshot.ready || checking || installing) return;
    try {
        if (!automaticCheckDue(localStorage.getItem(updateAttemptKey(snapshot.channel)), Date.now())) return;
    } catch {
        updateSnapshot({ errorKey: "updater_error_storage" });
        return;
    }
    const channel = snapshot.channel;
    automaticTimer = setTimeout(() => {
        if (snapshot.autoCheck && snapshot.channel === channel) void checkForUpdate(true);
    }, AUTO_UPDATE_DELAY_MS);
};

export const initializeUpdates = async (settings: Record<string, string>) => {
    if (initialized) return;
    initialized = true;
    const [version, support] = await Promise.allSettled([
        getVersion(), invoke<UpdateSupport>("get_update_support"),
    ]);
    updateSnapshot({ ready: true,
        ...readUpdatePreferences(settings, version.status === "fulfilled" ? version.value : ""),
        currentVersion: version.status === "fulfilled" ? version.value : "",
        support: support.status === "fulfilled" ? support.value : null,
        errorKey: version.status === "rejected" || support.status === "rejected" ? "updater_error_unavailable" : null,
    });
    scheduleAutomaticCheck();
};

export const saveUpdatePreference = async (key: "autoCheck" | "channel", value: boolean | UpdateChannel) => {
    if (snapshot.saving || checking || installing) return;
    updateSnapshot({ saving: true, errorKey: null });
    try {
        await invoke("save_setting", { key: key === "autoCheck" ? "app.update_auto_check" : "app.update_channel", value: String(value) });
        if (key === "autoCheck" && typeof value === "boolean") updateSnapshot({ autoCheck: value });
        if (key === "channel" && (value === "stable" || value === "preview")) {
            updateSnapshot({ channel: value, phase: "idle", available: null, unpublishedChannels: [], checkedAt: null });
        }
    } catch { updateSnapshot({ errorKey: "updater_error_settings" }); }
    finally { updateSnapshot({ saving: false }); scheduleAutomaticCheck(); }
};

export const checkForUpdate = async (automatic = false) => {
    if (!snapshot.ready || checking || installing || snapshot.saving) return;
    const channel = snapshot.channel;
    // Persist before invoking Rust so a recreated WebView cannot start another automatic request.
    if (!recordAttempt(channel) && automatic) {
        updateSnapshot({ errorKey: "updater_error_storage" });
        return;
    }
    clearTimeout(automaticTimer);
    checking = true;
    updateSnapshot({ phase: "checking", errorKey: null, available: null,
        unpublishedChannels: [], downloaded: 0, total: null, verified: false });
    try {
        const result = await invoke<UpdateCheck>("check_for_update", { channel });
        updateSnapshot({ currentVersion: result.currentVersion, support: result.support,
            available: result.available, unpublishedChannels: result.unpublishedChannels,
            phase: result.available ? "available" : "current", checkedAt: Date.now() });
    } catch (error: unknown) { updateSnapshot({ phase: "error", errorKey: updateErrorKey(error) }); }
    finally { checking = false; }
};

export const installAvailableUpdate = async (updateId: string) => {
    const available = snapshot.available;
    if (!available || updateId !== available.id || !snapshot.support?.canInstall || checking || installing) return;
    installing = true;
    clearTimeout(automaticTimer);
    updateSnapshot({ phase: "downloading", downloaded: 0, total: null, verified: false, errorKey: null });
    let lastProgress = 0;
    const progress = new Channel<UpdateProgress>();
    progress.onmessage = event => {
        if (!installing) return;
        if (event.event === "downloading") {
            const now = Date.now();
            if (now - lastProgress >= 100 || event.downloaded === event.total) {
                lastProgress = now;
                updateSnapshot({ phase: "downloading", downloaded: event.downloaded, total: event.total });
            }
        } else if (event.event === "verified") updateSnapshot({ phase: "verified", verified: true });
        else updateSnapshot({ phase: "installing" });
    };
    try {
        await invoke("install_update", { updateId, onProgress: progress });
        updateSnapshot({ phase: "installing" });
    } catch (error: unknown) {
        installing = false;
        updateSnapshot({ phase: "error", available: null, errorKey: updateErrorKey(error) });
        // An installation failure requires another check; the previous opaque ID is not reused.
    }
};

export const openUpdateRelease = async () => {
    try { await openUrl(snapshot.available?.releaseUrl ?? snapshot.support?.releasesUrl
        ?? "https://github.com/qiyuanhuakai/dezirclip/releases"); }
    catch { updateSnapshot({ errorKey: "updater_error_link" }); }
};
