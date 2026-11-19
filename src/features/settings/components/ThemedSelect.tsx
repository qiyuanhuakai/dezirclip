import Select, { type ThemedSelectProps } from "@deziro/ui-react/components/ThemedSelect";
import { invoke } from "../../../shared/lib/desktopInvoke";

export type { ThemedSelectOption } from "@deziro/ui-react/components/ThemedSelect";

const activate = () => { void invoke("activate_window_focus").catch(console.error); };

export default function ThemedSelect(props: Omit<ThemedSelectProps, "onActivate">) {
  return <Select {...props} onActivate={activate} />;
}
