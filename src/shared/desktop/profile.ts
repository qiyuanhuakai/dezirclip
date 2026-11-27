import { invoke as tauriInvoke } from "@tauri-apps/api/core";
import { openMyGoDataFolder, readMyGoProfile } from "../mygo/profile";

interface DesktopProfilePort {
  dataPath(): Promise<string>;
  openDataFolder(): Promise<void>;
}

export const desktopProfile: DesktopProfilePort = import.meta.env.MODE === "mygo" ? {
  async dataPath() { return (await readMyGoProfile()).data_directory; },
  openDataFolder: openMyGoDataFolder,
} : {
  dataPath: () => tauriInvoke("get_data_path"),
  openDataFolder: () => tauriInvoke("open_data_folder"),
};
