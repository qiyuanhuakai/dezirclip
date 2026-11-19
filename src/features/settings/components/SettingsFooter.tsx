import Footer, { type SettingsFooterProps } from "@deziro/ui-react/components/SettingsFooter";
import { openUrl } from "@tauri-apps/plugin-opener";

const repository = "https://github.com/qiyuanhuakai/dezirclip";

export default function SettingsFooter(props: Omit<SettingsFooterProps, "productName" | "onFeedback" | "onWebsite">) {
    return <Footer {...props} productName="DezirClip" onFeedback={() => { void openUrl(repository + "/issues"); }}
        onWebsite={() => { void openUrl(repository); }} />;
}
