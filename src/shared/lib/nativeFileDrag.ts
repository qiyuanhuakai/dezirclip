import { invoke } from "./desktopInvoke";

let active = 0;
export const nativeFileDragActive = () => active > 0;

export async function startNativeFileDrag(args: {
  id: number;
  expectedHostInstanceId: string;
  expectedPrivacyEpoch: number;
  allowSensitive: boolean;
}) {
  active += 1;
  try {
    await invoke("start_file_drag", args);
  } finally {
    active -= 1;
  }
}
