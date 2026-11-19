import { lazy } from "react";
import Header, { type AppHeaderProps as HeaderProps } from "@deziro/ui-react/components/AppHeader";
import { usePlatformCapabilities } from "../../../shared/hooks/usePlatformCapabilities";
import { invoke } from "../../../shared/lib/desktopInvoke";
import { getClipboardTypeName } from "../../../shared/lib/clipboardTypeName";

const TagFilterPicker = lazy(() => import("../../tag/components/TagFilterPicker"));
const contentTypeIds = ["text", "image", "file", "url", "code", "video", "rich_text"];
const dragAttributes = { "data-tauri-drag-region": true };
const activate = () => { void invoke("activate_window_focus").catch(console.error); };
const hide = () => { void invoke("hide_window_cmd").catch(console.error); };

type AppHeaderProps = Omit<HeaderProps, "desktopInteractions" | "pending" | "failed" | "onPinChange" |
  "onActivate" | "onHide" | "contentTypes" | "renderTagFilter" | "dragAttributes"> & {
  setIsWindowPinned: (pinned: boolean) => void;
  allTags: string[];
  tagColors?: Record<string, string>;
  theme: string;
};

export default function AppHeader({ setIsWindowPinned, allTags, tagColors = {}, theme, ...props }: AppHeaderProps) {
  const { desktopInteractions, pending, failed } = usePlatformCapabilities();
  return <Header {...props} desktopInteractions={desktopInteractions} pending={pending} failed={failed}
    dragAttributes={dragAttributes} onActivate={activate} onHide={hide}
    onPinChange={pinned => { void invoke("set_window_pinned", { pinned }).then(() => setIsWindowPinned(pinned)).catch(console.error); }}
    contentTypes={contentTypeIds.map(id => ({ id, label: getClipboardTypeName(id, props.t) }))}
    renderTagFilter={picker => <TagFilterPicker {...picker} tags={allTags} colors={tagColors} theme={theme} t={props.t} />} />;
}
