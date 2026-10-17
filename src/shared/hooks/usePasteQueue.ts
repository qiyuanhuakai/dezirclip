import { useCallback, useEffect, useState } from "react";
import { invoke } from "@tauri-apps/api/core";
import { listen } from "@tauri-apps/api/event";

interface UsePasteQueueOptions {
    readonly t: (key: string) => string;
    readonly pushToast: (msg: string, duration?: number) => number;
}

/**
 * Surface for the sequential-paste queue.
 *
 * The backend has emitted `queue-item-pasted` and `queue-finished` from the
 * start, and nothing listened to either. The result was a queue the user
 * could not see: no way to tell how many items were waiting, no way to abandon
 * a batch, and pressing the hotkey with nothing queued was indistinguishable
 * from a batch that had just run dry.
 *
 * The queue also grows with no event at all -- `pipeline.rs` appends on every
 * capture while the mode is on -- so the count is refreshed on window focus as
 * well, which is the only signal available for "you copied something".
 */
export const usePasteQueue = ({ t, pushToast }: UsePasteQueueOptions) => {
    const [count, setCount] = useState(0);

    const refresh = useCallback(() => {
        invoke<number[]>("get_paste_queue")
            .then((ids) => setCount(ids.length))
            .catch(() => setCount(0));
    }, []);

    const clear = useCallback(() => {
        invoke("clear_paste_queue").then(refresh).catch(console.error);
    }, [refresh]);

    useEffect(() => {
        refresh();

        const unlisteners = [
            listen("queue-item-pasted", () => refresh()),
            listen("queue-finished", () => refresh()),
            listen("paste-queue-empty", () => {
                refresh();
                pushToast(t("sequential_paste_queue_empty"));
            }),
        ];
        const onFocus = () => refresh();
        window.addEventListener("focus", onFocus);

        return () => {
            unlisteners.forEach((unlisten) => unlisten.then((f) => f()));
            window.removeEventListener("focus", onFocus);
        };
    }, [refresh, pushToast, t]);

    return { pasteQueueCount: count, refreshPasteQueue: refresh, clearPasteQueue: clear };
};
