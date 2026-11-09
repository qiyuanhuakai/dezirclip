import { invoke } from "./desktopInvoke";
import { getPrivacyViewState, refreshPrivacyAfterConfirmation } from "./privacySession";

interface CopyRequest {
  id: number;
  expectedHostInstanceId?: string;
  content: string;
  contentType: string;
  paste: boolean;
  deleteAfterUse: boolean;
  pasteWithFormat: boolean;
  pasteImageAsBase64: boolean;
  expectedPrivacyEpoch?: number;
  derivedFromId?: number;
  moveToTop?: boolean;
}

export async function copyHistorySelection(request: CopyRequest, _t: (key: string) => string, mygoCommand: "copy_to_clipboard" | "send_emoji" = "copy_to_clipboard") {
  if (import.meta.env.MODE !== "mygo") {
    await invoke("copy_to_clipboard", { ...request });
    return "completed";
  }
  const view = getPrivacyViewState();
  if (view.phase !== "ready" || view.pending || !view.snapshot) throw new Error("PrivacyStateUnavailable");
  const args = { ...request, viewEpoch: view.snapshot.privacy_epoch };
  const outcome = await invoke<string>(mygoCommand, args);
  if (outcome === "completed") return "completed";
  if (outcome !== "target_required" || !request.paste) throw new Error("UnexpectedCopyResult");
  await refreshPrivacyAfterConfirmation(request.expectedPrivacyEpoch ?? args.viewEpoch);
  const copied = await invoke<string>(mygoCommand, { ...args, paste: false, deleteAfterUse: false, moveToTop: false });
  if (copied !== "completed") throw new Error("UnexpectedCopyResult");
  return "copied";
}
