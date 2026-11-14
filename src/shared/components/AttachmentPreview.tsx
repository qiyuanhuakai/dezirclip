import { File, FileText, FileArchive, FileCode, FileSpreadsheet, FileChartColumn, Image as ImageIcon, Video, Music } from "lucide-react";
import { useLayoutEffect, useMemo, useRef, useState } from "react";
import { attachmentKind, attachmentLayout, attachmentTiles } from "../lib/attachmentPreview";
import "../../styles/components/attachment-preview.css";

function FileIcon({ name }: { name: string }) {
    const Icon = ({ image: ImageIcon, video: Video, audio: Music, archive: FileArchive, code: FileCode,
        spreadsheet: FileSpreadsheet, presentation: FileChartColumn, document: FileText } as Record<string, typeof File>)[attachmentKind(name)] ?? File;
    return <Icon size={16} aria-hidden="true" />;
}

// All attachments remain in their original history entry, with one group action.
export function AttachmentPreview({ html, names, video = false, fileLabel, expanded = false, compact = false }: {
    html?: string | null; names: string; video?: boolean; fileLabel: string; expanded?: boolean; compact?: boolean;
}) {
    const attachments = useMemo(() => attachmentTiles(html, names).map(file =>
        !expanded && (video || attachmentKind(file.name) === 'video') ? { ...file, src: null } : file), [html, names, video, expanded]);
    const container = useRef<HTMLDivElement>(null);
    const [width, setWidth] = useState(320);
    useLayoutEffect(() => {
        const element = container.current;
        if (!element) return;
        const update = () => setWidth(element.clientWidth);
        update();
        const observer = new ResizeObserver(update);
        observer.observe(element);
        return () => observer.disconnect();
    }, []);
    const layout = attachmentLayout(attachments, width, expanded, compact);
    const ellipsis = layout.omitted > 0 ? <span className="attachment-ellipsis" title={`+${layout.omitted}`}>…</span> : null;
    return <div ref={container} aria-label={`${fileLabel} × ${attachments.length}`} data-columns={layout.columns} className={`attachment-gallery${expanded ? " attachment-expanded" : ""}`}>
        <div data-last-span={layout.lastSpan} className={`attachment-items${layout.media ? " attachment-media" : ""}`}>
            {layout.visible.map((file, index) => <figure className="attachment-tile" key={`${index}:${file.name}`} title={file.name}>
                {file.src && <img className={expanded ? "attachment-expanded-image" : "image-preview"} src={file.src} alt={file.name} loading={expanded ? "eager" : "lazy"} draggable={false} />}
                {(!file.src || expanded) && <figcaption><FileIcon name={file.name} /><span>{file.name}</span></figcaption>}
            </figure>)}
            {!layout.media && ellipsis}
        </div>
        {layout.media && (layout.names.length > 0 || layout.omitted > 0) && <div className="attachment-overflow">
            {layout.names.map((file, index) => <span className="attachment-name" key={`${index}:${file.name}`} title={file.name}><FileIcon name={file.name} /><span>{file.name}</span></span>)}
            {ellipsis}
        </div>}
    </div>;
}
