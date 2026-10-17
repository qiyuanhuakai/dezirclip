import { useEffect, useState } from "react";
import { resolveSettingsLayout, type SettingsLayout } from "../utils/settingsLayout";

const STACK: SettingsLayout = { split: false, paneColumns: 1 };

const readLayout = (): SettingsLayout =>
    typeof window === "undefined" ? STACK : resolveSettingsLayout(window.innerWidth);

/**
 * Drives the settings page's shape from the window width.
 *
 * Resize fires continuously while the window is dragged, but the layout only
 * actually changes when a breakpoint is crossed -- returning the previous object
 * for an unchanged shape keeps a resize drag from re-rendering the settings tree
 * on every frame. The rail is mounted only in split mode, so those renders are
 * not free.
 */
export const useSettingsLayout = (): SettingsLayout => {
    const [layout, setLayout] = useState<SettingsLayout>(readLayout);

    useEffect(() => {
        let frame = 0;
        const sync = () => {
            cancelAnimationFrame(frame);
            frame = requestAnimationFrame(() => {
                setLayout(prev => {
                    const next = readLayout();
                    return next.split === prev.split && next.paneColumns === prev.paneColumns
                        ? prev
                        : next;
                });
            });
        };
        window.addEventListener("resize", sync);
        return () => {
            cancelAnimationFrame(frame);
            window.removeEventListener("resize", sync);
        };
    }, []);

    return layout;
};
