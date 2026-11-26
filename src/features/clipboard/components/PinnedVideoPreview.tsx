import { useEffect, useRef, useState, type ReactNode } from "react";
import { convertFileSrc, invoke } from "@tauri-apps/api/core";
import { beginPrivacyAccess } from "../../../shared/lib/privacySession";
import type { PreviewRequest } from "./compactPreviewProtocol";

const release = (token: string) => { void invoke("close_pinned_video", { token }).catch(() => undefined); };

export function PinnedVideoPreview({ request, poster, children }: {
    request: PreviewRequest;
    poster?: string;
    children: ReactNode;
}) {
    const video = useRef<HTMLVideoElement>(null);
    const opened = useRef<{ token?: string } | null>(null);
    const [token, setToken] = useState<string>();
    const [loading, setLoading] = useState(false);
    const [failure, setFailure] = useState(false);
    const english = request.language === "en";
    useEffect(() => () => {
        const current = opened.current;
        opened.current = null;
        if (current?.token) release(current.token);
    }, []);
    useEffect(() => {
        const element = video.current;
        return () => {
            element?.pause();
            if (element) {
                element.removeAttribute("src");
                element.load();
            }
        };
    }, [token]);
    const play = () => {
        const access = beginPrivacyAccess("list");
        if (!access || access.epoch !== request.privacyEpoch) {
            setFailure(true);
            return;
        }
        const attempt: { token?: string } = {};
        opened.current = attempt;
        setFailure(false);
        setLoading(true);
        void invoke<string>("open_pinned_video", {
            id: request.id,
            expectedEpoch: request.privacyEpoch,
            expectedHostInstanceId: request.expectedHostInstanceId,
        }).then(source => {
            if (opened.current !== attempt) { release(source); return; }
            if (!access.isCurrent()) {
                release(source);
                opened.current = null;
                setLoading(false);
                return;
            }
            attempt.token = source;
            setToken(source);
        }, () => {
            if (opened.current === attempt) {
                opened.current = null;
                setLoading(false);
                setFailure(true);
            }
        });
    };
    const playLabel = english ? "Play video" : "播放视频";
    return <div className="pinned-video-preview">
        {token ? <video ref={video} src={convertFileSrc(token, "dzcvideo")} poster={poster}
            controls controlsList="nodownload noremoteplayback" disablePictureInPicture
            autoPlay playsInline preload="metadata" aria-label={playLabel}
            onError={() => {
                release(token);
                opened.current = null;
                setToken(undefined);
                setLoading(false);
                setFailure(true);
            }} /> : <>
            <button type="button" className="btn-secondary" onClick={play} disabled={loading}>{playLabel}</button>
            {failure && <div role="status" className="preview-action-status">
                {english ? "Unable to play this video. Check that the file still exists and the system supports its format."
                    : "无法播放此视频。请检查文件是否仍在原路径，以及系统是否支持该格式。"}
            </div>}
            {children}
        </>}
    </div>;
}
