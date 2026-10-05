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
  const defaultsReadRef = useRef(false);
  useEffect(() => {
    invoke<string>("get_data_path").then(setDataPath).catch(console.error);

    invoke<boolean>("is_autostart_enabled").then(setAutoStart).catch(console.error);

    return;
  }, [
    setAutoStart,
    setDataPath
  ]);

  // The system default per content type is another settings-panel answer: the
  // only thing that reads it is the "open with" row in the default-apps group,
  // which is collapsed. It used to cost five round trips on every startup and on
  // every wake after the idle destroyer rebuilt the webview, and each answer
  // wrote its own new object into the map, so the five writes were five separate
  // updates rather than one.
  //
  // Asking when the panel is first opened keeps the map identical, folds the
  // five writes into one, and moves the cost to the one place that can use it.
  useEffect(() => {
    if (!showSettings) return;
    if (defaultsReadRef.current) return;
    defaultsReadRef.current = true;

    const types = ["text", "image", "video", "code", "url"];
    Promise.all(
      types.map((type) =>
        invoke<string>("get_system_default_app", { contentType: type })
          .then((name) => [type, name] as const)
          .catch((err) => {
            console.error(`Failed to get default for ${type}`, err);
            return null;
          })
      )
    ).then((answers) => {
      const next: DefaultAppsMap = {};
      for (const answer of answers) {
        if (answer) next[answer[0]] = answer[1];
      }
      setDefaultApps(next);
    });
  }, [showSettings, setDefaultApps]);

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
