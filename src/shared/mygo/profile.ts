import type { DesktopProfileInfo } from "../protocol/wire";

export const readMyGoProfile = (): Promise<DesktopProfileInfo> =>
  window.mygo.call("Library.ProfileInfo");

export const openMyGoDataFolder = (): Promise<void> =>
  window.mygo.call("Library.OpenDataFolder");
