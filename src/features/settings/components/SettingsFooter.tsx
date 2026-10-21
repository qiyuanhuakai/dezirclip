import { Github, MessageSquare, RotateCcw } from "lucide-react";
import { openUrl } from "@tauri-apps/plugin-opener";

interface SettingsFooterProps {
    t: (key: string) => string;
    onResetSettings: () => void;
    /**
     * Wide layout folds the footer onto one line. Stacked, it is two centred
     * blocks and costs about 120px of height -- on a short landscape window that
     * is a fifth of the pane sitting on chrome that is not settings.
     */
    compact?: boolean;
}

const REPO_URL = "https://github.com/qiyuanhuakai/dezirclip";

const SettingsFooter = ({
    t,
    onResetSettings,
    compact = false,
}: SettingsFooterProps) => (
    <div className={`settings-footer${compact ? " settings-footer--compact" : ""}`}>
        <div className="settings-footer-actions">
            <div
                className="settings-group settings-footer-action settings-footer-card"
                onClick={() => openUrl(`${REPO_URL}/issues`)}
            >
                <div className="settings-footer-card-content">
                    <MessageSquare size={16} />
                    <span className="settings-footer-action-label settings-footer-card-label">
                        {t('feedback')}
                    </span>
                </div>
            </div>

            <div
                className="settings-group settings-footer-action settings-footer-card"
                onClick={() => onResetSettings()}
            >
                <div className="settings-footer-card-content">
                    <RotateCcw size={16} />
                    <span className="settings-footer-action-label settings-footer-card-label">
                        {t('reset_defaults')}
                    </span>
                </div>
            </div>
        </div>

        <div className="settings-footer-meta">
            <div className="settings-footer-version">
                DezirClip
            </div>
            <div className="settings-footer-links">
                <button type="button" className="settings-footer-link" onClick={() => openUrl(REPO_URL)}>
                    {t('official_website')}
                </button>
                <button type="button" className="settings-footer-link" onClick={() => openUrl(REPO_URL)}>
                    <Github size={12} />
                    <span>GitHub</span>
                </button>
            </div>
        </div>
    </div>
);

export default SettingsFooter;
