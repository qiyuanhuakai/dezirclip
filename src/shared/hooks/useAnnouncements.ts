import { useCallback, useState } from "react";
import type { Announcement } from "../types";

export type { Announcement } from "../types";

export function useAnnouncements() {
  const [announcements] = useState<Announcement[]>([]);
  const [loading] = useState(false);
  const dismissAnnouncement = useCallback((_id: string, _forever: boolean = true) => {}, []);

  return { announcements, loading, dismissAnnouncement };
}
