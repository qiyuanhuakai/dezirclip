import { useEffect, useRef, useState } from "react";
import { invoke } from "../../../../shared/lib/desktopInvoke";
import { emit } from "@tauri-apps/api/event";
import ThemedSelect from "../ThemedSelect";

type Availability = "checking" | "installed" | "missing" | "failed";

export default function QuickPasteRendererSetting({ t, renderer }: {
    t: (key: string) => string;
    renderer: string;
}) {
    const [value, setValue] = useState(renderer === "mygo" ? "mygo" : "tauri");
    const [saving, setSaving] = useState(false);
    const [failed, setFailed] = useState(false);
    const [availability, setAvailability] = useState<Availability>("checking");
    const inFlight = useRef(false);
    const mounted = useRef(false);
    useEffect(() => {
        mounted.current = true;
        return () => { mounted.current = false; };
    }, []);
    useEffect(() => { setValue(renderer === "mygo" ? "mygo" : "tauri"); }, [renderer]);
    useEffect(() => {
        let cancelled = false;
        setAvailability("checking");
        void invoke<boolean>("is_quick_paste_surface_helper_installed")
            .then((installed) => {
                if (!cancelled) setAvailability(installed === true ? "installed" : installed === false ? "missing" : "failed");
            })
            .catch(() => { if (!cancelled) setAvailability("failed"); });
        return () => { cancelled = true; };
    }, [renderer]);

    const changeRenderer = async (next: string) => {
        if (inFlight.current || next === value || !["tauri", "mygo"].includes(next)
            || next === "mygo" && availability !== "installed") return;
        inFlight.current = true;
        setSaving(true);
        setFailed(false);
        try {
            if (next === "mygo") {
                let installed: boolean;
                try {
                    installed = await invoke<boolean>("is_quick_paste_surface_helper_installed");
                } catch {
                    if (mounted.current) setAvailability("failed");
                    return;
                }
                if (!mounted.current) return;
                if (installed !== true) {
                    setAvailability(installed === false ? "missing" : "failed");
                    return;
                }
            }
            await invoke("save_setting", { key: "app.quick_paste_renderer", value: next });
            if (mounted.current) setValue(next);
            void emit("settings-changed").catch(console.error);
        } catch {
            if (mounted.current) setFailed(true);
        } finally {
            inFlight.current = false;
            if (mounted.current) setSaving(false);
        }
    };

    const notice = availability === "checking" ? "quick_paste_renderer_component_checking"
        : availability === "failed" ? "quick_paste_renderer_component_check_failed"
        : availability === "missing" ? value === "mygo"
            ? "quick_paste_renderer_component_missing_preference" : "quick_paste_renderer_component_missing"
        : null;

    return <div className="setting-item setting-item--detail quick-paste-renderer-setting">
        <div className="item-label-group"><label className="item-label" htmlFor="quick-paste-renderer">{t("quick_paste_renderer")}</label></div>
        <ThemedSelect inputId="quick-paste-renderer" value={value} onChange={changeRenderer} isDisabled={saving}
            options={[
                { value: "tauri", label: t("quick_paste_renderer_tauri") },
                { value: "mygo", label: t(availability === "missing" || availability === "failed"
                    ? "quick_paste_renderer_mygo_unavailable" : "quick_paste_renderer_mygo"), disabled: availability !== "installed" },
            ]} />
        {notice && <p role="status" className="settings-description">{t(notice)}</p>}
        {failed && <p role="alert" className="quick-paste-renderer-error">{t("quick_paste_renderer_save_failed")}</p>}
    </div>;
}
