import { useCallback, useEffect, useRef, useState } from "react";
import { invoke } from "@tauri-apps/api/core";
import { listen } from "@tauri-apps/api/event";
import {
  applyQueueEvent,
  applyQueueRead,
  INITIAL_PASTE_QUEUE_COUNT_STATE,
  readToken,
  type PasteQueueCountState,
} from "../lib/pasteQueueCount";

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
 * Enqueueing is the fourth signal and the one that was missing longest: the
 * queue grows on every capture while the mode is on, which emits none of the
 * paste events, and window focus is not a substitute — on Windows the main
 * window is deliberately kept non-activating while shown, so opening it does
 * not always produce a DOM focus. `paste-queue-changed` carries the new length
 * so the count can be adopted without a round trip to `get_paste_queue`.
 *
 * That shortcut creates an ordering hazard, because the two sources do not
 * arrive in order and neither write used to know about the other. `pasteQueueCount`
 * holds the rule; this hook just routes the two paths into it.
 */
export const usePasteQueue = ({ t, pushToast }: UsePasteQueueOptions) => {
    // The ref is the authority and the state is only what gets rendered. An
    // in-flight read has to be judged against the revision it was issued at,
    // and pulling that out of React state at resolve time would already be one
    // step too late — the event that overtook it will already have moved on.
    const latestRef = useRef<PasteQueueCountState>(INITIAL_PASTE_QUEUE_COUNT_STATE);
    const [count, setCount] = useState(INITIAL_PASTE_QUEUE_COUNT_STATE.count);

    const commit = useCallback((next: PasteQueueCountState) => {
        latestRef.current = next;
        setCount(next.count);
    }, []);

    const refresh = useCallback(() => {
        const token = readToken(latestRef.current);
        invoke<number[]>("get_paste_queue")
            .then((ids) => commit(applyQueueRead(latestRef.current, token, ids.length)))
            .catch(() => commit(applyQueueRead(latestRef.current, token, 0)));
    }, [commit]);

    // A length the backend pushed retires every read already in flight, so a
    // slower answer to an older question cannot put the count back.
    const applyCount = useCallback((next: number) => {
        commit(applyQueueEvent(latestRef.current, next));
    }, [commit]);

    const clear = useCallback(() => {
        invoke("clear_paste_queue").then(refresh).catch(console.error);
    }, [refresh]);

    useEffect(() => {
        refresh();

        const unlisteners = [
            listen("queue-item-pasted", () => refresh()),
            listen("queue-finished", () => refresh()),
            listen<number>("paste-queue-changed", (event) => {
                applyCount(event.payload);
            }),
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
    }, [applyCount, refresh, pushToast, t]);

    return { pasteQueueCount: count, refreshPasteQueue: refresh, clearPasteQueue: clear };
};
