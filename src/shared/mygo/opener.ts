import { shellCall } from "./bridge";

export const openUrl = (url: string) => shellCall<void>("OpenURL", url);
