import { useEffect, useRef, useState } from "react";
import { invoke } from "@tauri-apps/api/core";
import { emit } from "@tauri-apps/api/event";
import ThemedSelect from "../ThemedSelect";

export default function QuickPasteRendererSetting({ t, renderer }: {
    t: (key: string) => string;
    renderer: string;
}) {
    const [value, setValue] = useState(renderer === "mygo" ? "mygo" : "tauri");
    const [saving, setSaving] = useState(false);
    const [failed, setFailed] = useState(false);
    const inFlight = useRef(false);
    useEffect(() => { setValue(renderer === "mygo" ? "mygo" : "tauri"); }, [renderer]);

    const changeRenderer = async (next: string) => {
        if (inFlight.current || next === value || !["tauri", "mygo"].includes(next)) return;
        inFlight.current = true;
        setSaving(true);
        setFailed(false);
        try {
            await invoke("save_setting", { key: "app.quick_paste_renderer", value: next });
            setValue(next);
            void emit("settings-changed").catch(console.error);
        } catch {
            setFailed(true);
        } finally {
            inFlight.current = false;
            setSaving(false);
        }
    };

    return <div className="setting-item setting-item--detail quick-paste-renderer-setting">
        <div className="item-label-group"><label className="item-label" htmlFor="quick-paste-renderer">{t("quick_paste_renderer")}</label></div>
        <ThemedSelect inputId="quick-paste-renderer" value={value} onChange={changeRenderer} isDisabled={saving}
            options={[
                { value: "tauri", label: t("quick_paste_renderer_tauri") },
                { value: "mygo", label: t("quick_paste_renderer_mygo") },
            ]} />
        {failed && <p role="alert" className="quick-paste-renderer-error">{t("quick_paste_renderer_save_failed")}</p>}
    </div>;
}
