import Rail, { type SettingsRailProps } from "@deziro/ui-react/components/SettingsRail";
import { SETTINGS_GROUPS, type SettingsGroupId } from "../utils/settingsLayout";

export default function SettingsRail(props: Omit<SettingsRailProps<SettingsGroupId>, "groups">) {
    return <Rail {...props} groups={SETTINGS_GROUPS} />;
}
