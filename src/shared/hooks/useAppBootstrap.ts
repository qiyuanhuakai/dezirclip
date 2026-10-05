import { useEffect, useRef } from "react";
import type { Dispatch, SetStateAction } from "react";
import type { DefaultAppsMap, InstalledAppOption } from "../../features/app/types";
import { invoke } from "@tauri-apps/api/core";

interface UseAppBootstrapOptions {
  showSettings: boolean;
  setDataPath: Dispatch<SetStateAction<string>>;
  setInstalledApps: Dispatch<SetStateAction<InstalledAppOption[]>>;
  setAutoStart: Dispatch<SetStateAction<boolean>>;
  setDefaultApps: Dispatch<SetStateAction<DefaultAppsMap>>;
}

export const useAppBootstrap = ({
  showSettings,
  setDataPath,
  setInstalledApps,
  setAutoStart,
  setDefaultApps
}: UseAppBootstrapOptions) => {
  const scannedRef = useRef(false);
  useEffect(() => {
    invoke<string>("get_data_path").then(setDataPath).catch(console.error);

    invoke<boolean>("is_autostart_enabled").then(setAutoStart).catch(console.error);

    const types = ["text", "image", "video", "code", "url"];
    types.forEach(async (type) => {
      try {
        const name = await invoke<string>("get_system_default_app", { contentType: type });
        setDefaultApps((prev) => ({ ...prev, [type]: name }));
      } catch (err) {
        console.error(`Failed to get default for ${type}`, err);
      }
    });

    return;
  }, [
    setAutoStart,
    setDataPath,
    setDefaultApps
  ]);

  // On Windows the scan shells out to PowerShell and reads the whole Start app
  // list: measured at 3.3 s per call here. The only thing it feeds is the app
  // picker inside the settings panel, so it used to pay that cost on every
  // startup -- and on every wake after the idle destroyer rebuilt the webview
  // -- for a dropdown most sessions never open. Asking when the panel is first
  // opened keeps the list identical and moves the cost to the one place that
  // can use it.
  useEffect(() => {
    if (!showSettings) return;
    if (scannedRef.current) return;
    scannedRef.current = true;

    invoke<{ name: string; path: string }[]>("scan_installed_apps")
      .then((apps) => {
        if (apps && apps.length > 0) {
          setInstalledApps(
            apps
              .map((a) => ({ label: a.name, value: a.path }))
              .sort((a, b) => a.label.localeCompare(b.label))
          );
        } else {
          console.warn("No apps found by scan_installed_apps");
        }
      })
      .catch((err) => {
        console.error("Failed to scan apps:", err);
      });
  }, [showSettings, setInstalledApps]);
};
