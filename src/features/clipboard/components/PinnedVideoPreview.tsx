import { useEffect, useRef, useState, type ReactNode } from "react";
import { convertFileSrc, invoke } from "@tauri-apps/api/core";
import { VideoPlayer } from "@deziro/ui-react/components/VideoPlayer";
import { beginPrivacyAccess } from "../../../shared/lib/privacySession";
import type { PreviewRequest } from "./compactPreviewProtocol";

const release = (token: string) => { void invoke("close_pinned_video", { token }).catch(() => undefined); };
type PlaybackSource = { token: string; url?: string };

export function PinnedVideoPreview({ request, poster, children }: {
    request: PreviewRequest;
    poster?: string;
    children: ReactNode;
}) {
    const opened = useRef<{ token?: string } | null>(null);
    const [source, setSource] = useState<PlaybackSource>();
    const [loading, setLoading] = useState(false);
    const [failure, setFailure] = useState(false);
    const english = request.language === "en";
    useEffect(() => () => {
        const current = opened.current;
        opened.current = null;
        if (current?.token) release(current.token);
    }, []);
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
        void invoke<PlaybackSource>("open_pinned_video", {
            id: request.id,
            expectedEpoch: request.privacyEpoch,
            expectedHostInstanceId: request.expectedHostInstanceId,
        }).then(source => {
            if (opened.current !== attempt) { release(source.token); return; }
            if (!access.isCurrent()) {
                release(source.token);
                opened.current = null;
                setLoading(false);
                return;
            }
            attempt.token = source.token;
            setSource(source);
        }, () => {
            if (opened.current === attempt) {
                opened.current = null;
                setLoading(false);
                setFailure(true);
            }
        });
    };
    return <VideoPlayer key={source?.token ?? "cover"} source={source && (source.url ?? convertFileSrc(source.token, "dzcvideo"))}
        poster={poster} loading={loading} onOpen={play}
        labels={english
            ? { play: "Play video", pause: "Pause", progress: "Playback position", mute: "Mute", unmute: "Unmute", volume: "Volume", loop: "Loop" }
            : { play: "播放视频", pause: "暂停", progress: "播放进度", mute: "静音", unmute: "取消静音", volume: "音量", loop: "循环" }}
        status={failure ? (english
            ? "Unable to play this video. Check that the file still exists and the system supports its format."
            : "无法播放此视频。请检查文件是否仍在原路径，以及系统是否支持该格式。") : undefined}
        onError={() => {
            if (!source || opened.current?.token !== source.token) return;
            release(source.token);
            opened.current = null;
            setSource(undefined);
            setLoading(false);
            setFailure(true);
        }}>{children}</VideoPlayer>;
}
