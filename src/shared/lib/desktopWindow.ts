import { getCurrentWindow as tauriWindow, getAllWindows as tauriWindows } from "@tauri-apps/api/window";
import { mainWindow } from "../mygo/mainWindow";

export const getCurrentWindow = () => import.meta.env.MODE === "mygo" ? mainWindow : tauriWindow();
export const getAllWindows = async () => import.meta.env.MODE === "mygo" ? [mainWindow] : tauriWindows();
