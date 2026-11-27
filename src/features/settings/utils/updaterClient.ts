import { getVersion } from "@tauri-apps/api/app";
import { invoke } from "../../../shared/lib/desktopInvoke";
import { Channel } from "@tauri-apps/api/core";
import { listen } from "@tauri-apps/api/event";
import { openUrl } from "@tauri-apps/plugin-opener";
import { readUpdatePreferences, updateErrorKey } from "./updatePolicy";
import type { UpdateChannel } from "./updatePolicy";

interface UpdateSupport {
    installation: "nsis" | "app_image" | "app_image_read_only" | "package_manager" | "portable" | "unpackaged";
    canInstall: boolean;
    reason: string;
    releasesUrl: string;
}

interface AvailableUpdate { id: string; version: string; notes: string | null; releaseUrl: string }
type UpdateCheckStatus = "available" | "current" | "ahead" | "unpublished";

interface UpdateCheck {
    currentVersion: string;
    channel: UpdateChannel;
    channelVersion: string | null;
    status: UpdateCheckStatus;
    support: UpdateSupport;
    available: AvailableUpdate | null;
    unpublishedChannels: UpdateChannel[];
}

interface CompletedCheck {
    channel: UpdateChannel;
    checkedAt: number;
    result: { Ok: UpdateCheck } | { Err: string };
}

type UpdateProgress =
    | { event: "downloading"; downloaded: number; total: number | null }
    | { event: "verified" }
    | { event: "installing" };

export interface UpdaterSnapshot {
    ready: boolean;
    currentVersion: string;
    channelVersion: string | null;
    autoCheck: boolean;
    channel: UpdateChannel;
    saving: boolean;
    phase: "idle" | "checking" | UpdateCheckStatus | "downloading" | "verified" | "installing" | "error";
    support: UpdateSupport | null;
    available: AvailableUpdate | null;
    unpublishedChannels: UpdateChannel[];
    downloaded: number;
    total: number | null;
    verified: boolean;
    checkedAt: number | null;
    errorKey: string | null;
}

let snapshot: UpdaterSnapshot = { ready: false, currentVersion: "", channelVersion: null, autoCheck: false,
    channel: "stable", saving: false, phase: "idle", support: null, available: null,
    unpublishedChannels: [], downloaded: 0, total: null, verified: false, checkedAt: null, errorKey: null };
const subscribers = new Set<() => void>();
let initialized = false;
let latestCompletion = 0;
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

const applyCheck = (result: UpdateCheck, checkedAt: number) => {
    updateSnapshot({ currentVersion: result.currentVersion, support: result.support,
        channelVersion: result.channelVersion, available: result.available,
        unpublishedChannels: result.unpublishedChannels, phase: result.status, checkedAt,
        errorKey: null });
};

const acceptCompletedCheck = (completed: CompletedCheck) => {
    if (!snapshot.ready || checking || installing || completed.channel !== snapshot.channel
        || completed.checkedAt <= latestCompletion) return;
    latestCompletion = completed.checkedAt;
    if ("Ok" in completed.result) applyCheck(completed.result.Ok, completed.checkedAt);
    else updateSnapshot({ phase: "error", available: null, channelVersion: null,
        unpublishedChannels: [], errorKey: updateErrorKey(completed.result.Err) });
};

const restoreCompletedCheck = async () => {
    const completed = await invoke<CompletedCheck | null>("get_completed_update_check", { channel: snapshot.channel });
    if (completed) acceptCompletedCheck(completed);
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
    await listen<CompletedCheck>("update-check-completed", event => acceptCompletedCheck(event.payload)).catch(() => {});
    await restoreCompletedCheck().catch(() => {});
};

export const saveUpdatePreference = async (key: "autoCheck" | "channel", value: boolean | UpdateChannel) => {
    if (snapshot.saving || checking || installing) return;
    updateSnapshot({ saving: true, errorKey: null });
    try {
        await invoke("save_setting", { key: key === "autoCheck" ? "app.update_auto_check" : "app.update_channel", value: String(value) });
        if (key === "autoCheck" && typeof value === "boolean") updateSnapshot({ autoCheck: value });
        if (key === "channel" && (value === "stable" || value === "preview")) {
            latestCompletion = 0;
            updateSnapshot({ channel: value, phase: "idle", channelVersion: null, available: null, unpublishedChannels: [], checkedAt: null });
            void restoreCompletedCheck().catch(() => {});
        }
    } catch { updateSnapshot({ errorKey: "updater_error_settings" }); }
    finally { updateSnapshot({ saving: false }); }
};

export const checkForUpdate = async () => {
    if (!snapshot.ready || checking || installing || snapshot.saving) return;
    const channel = snapshot.channel;
    checking = true;
    updateSnapshot({ phase: "checking", errorKey: null, channelVersion: null, available: null,
        unpublishedChannels: [], downloaded: 0, total: null, verified: false });
    try {
        const result = await invoke<UpdateCheck>("check_for_update", { channel });
        latestCompletion = Date.now();
        applyCheck(result, latestCompletion);
    } catch (error: unknown) { updateSnapshot({ phase: "error", errorKey: updateErrorKey(error) }); }
    finally { checking = false; }
};

export const installAvailableUpdate = async (updateId: string) => {
    const available = snapshot.available;
    if (!available || updateId !== available.id || !snapshot.support?.canInstall || checking || installing) return;
    installing = true;
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
