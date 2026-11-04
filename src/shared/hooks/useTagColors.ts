import { useEffect, useState } from "react";
import { invoke } from "../lib/desktopInvoke";
import { listen } from "@tauri-apps/api/event";
import { usePrivacySession } from "./usePrivacySession";

export const useTagColors = () => {
  const [tagColors, setTagColors] = useState<Record<string, string>>({});
  const privacy = usePrivacySession();
  const epoch = privacy.phase === "ready" && !privacy.pending ? privacy.snapshot?.privacy_epoch : undefined;

  useEffect(() => {
    if (epoch === undefined) { setTagColors({}); return; }
    let active = true;
    const fetchTagColors = () => {
      invoke<Record<string, string>>("get_tag_colors")
        .then(colors => { if (active) setTagColors(colors); })
        .catch(console.error);
    };

    fetchTagColors();
    const unlistenColors = listen("tag-colors-updated", fetchTagColors);

    return () => {
      active = false;
      unlistenColors.then((f) => f());
    };
  }, [epoch]);

  return tagColors;
};
