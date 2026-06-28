import { Github, MessageSquare, RotateCcw } from "lucide-react";
import { openUrl } from "@tauri-apps/plugin-opener";

interface SettingsFooterProps {
    t: (key: string) => string;
    appVersion: string;
    onResetSettings: () => void;
}

const SettingsFooter = ({
    t,
    appVersion,
    onResetSettings
}: SettingsFooterProps) => (
    <>
        {/* Footer Actions */}
        <div style={{
            marginTop: '16px',
            display: 'flex',
            justifyContent: 'center',
            gap: '12px',
            flexWrap: 'wrap'
        }}>
            {/* Feedback Card */}
            <div
                className="settings-group settings-footer-action"
                style={{
                    cursor: 'pointer',
                    transition: 'all 0.2s',
                    margin: 0,
                    width: 'auto',
                    padding: '10px 16px',
                    display: 'flex',
                    alignItems: 'center',
                    justifyContent: 'center',
                    marginBottom: '0'
                }}
                onClick={() => openUrl('https://github.com/qiyuanhuakai/dezirclip/issues')}
            >
                <div className="settings-footer-action-content" style={{ display: 'flex', alignItems: 'center', gap: '8px' }}>
                    <MessageSquare size={16} />
                    <span className="settings-footer-action-label" style={{ fontSize: '13px', fontWeight: 600 }}>
                        {t('feedback')}
                    </span>
                </div>
            </div>

            {/* Reset Card */}
            <div
                className="settings-group settings-footer-action"
                style={{
                    cursor: 'pointer',
                    transition: 'all 0.2s',
                    margin: 0,
                    width: 'auto',
                    padding: '10px 16px',
                    display: 'flex',
                    alignItems: 'center',
                    justifyContent: 'center',
                    marginBottom: '0'
                }}
                onClick={() => onResetSettings()}
            >
                <div className="settings-footer-action-content" style={{ display: 'flex', alignItems: 'center', gap: '8px' }}>
                    <RotateCcw size={16} />
                    <span className="settings-footer-action-label" style={{ fontSize: '13px', fontWeight: 600 }}>{t('reset_defaults')}</span>
                </div>
            </div>
        </div>

        {/* Version Info */}
        <div style={{
            marginTop: '16px',
            marginBottom: '32px',
            textAlign: 'center',
            opacity: 1
        }}>
            <div style={{
                fontSize: '13px',
                fontWeight: 600,
                color: 'var(--text-secondary)',
                letterSpacing: '0.5px',
                marginBottom: '4px',
                display: 'flex',
                alignItems: 'center',
                justifyContent: 'center',
                gap: '8px'
            }}>
                <span>DezirClip {appVersion ? `v${appVersion}` : "v0.2.0"}</span>
            </div>
            <div style={{
                fontSize: '11px',
                color: 'var(--text-secondary)',
                fontWeight: 500,
                marginBottom: '4px'
            }}>
                {t('slogan')}
            </div>
            <div style={{
                display: 'flex',
                alignItems: 'center',
                justifyContent: 'center',
                gap: '10px',
                flexWrap: 'wrap'
            }}>
                <button
                    onClick={() => openUrl('https://github.com/qiyuanhuakai/dezirclip/')}
                    style={{
                        fontSize: '11px',
                        color: 'var(--accent-color)',
                        background: 'transparent',
                        border: 'none',
                        cursor: 'pointer',
                        textDecoration: 'underline',
                        opacity: 0.7,
                        fontWeight: 600,
                        padding: '2px 4px'
                    }}
                    onMouseEnter={(e) => (e.currentTarget.style.opacity = '1')}
                    onMouseLeave={(e) => (e.currentTarget.style.opacity = '0.7')}
                >
                    {t('official_website')}
                </button>
                <button
                    onClick={() => openUrl('https://github.com/qiyuanhuakai/dezirclip')}
                    style={{
                        fontSize: '11px',
                        color: 'var(--accent-color)',
                        background: 'transparent',
                        border: 'none',
                        cursor: 'pointer',
                        textDecoration: 'underline',
                        opacity: 0.7,
                        fontWeight: 600,
                        padding: '2px 4px',
                        display: 'inline-flex',
                        alignItems: 'center',
                        gap: '4px'
                    }}
                    onMouseEnter={(e) => (e.currentTarget.style.opacity = '1')}
                    onMouseLeave={(e) => (e.currentTarget.style.opacity = '0.7')}
                >
                    <Github size={12} />
                    <span>GitHub</span>
                </button>
            </div>
        </div>
    </>
);

export default SettingsFooter;
