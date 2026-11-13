import { File, Image as ImageIcon, Video } from "lucide-react";
import { useMemo } from "react";
import { attachmentTiles } from "../lib/attachmentPreview";
import "../../styles/components/attachment-preview.css";

// Accept only the generated RAM-image gallery; never mount arbitrary attachment HTML.
export function AttachmentPreview({ html, names, video = false }: { html?: string | null; names: string; video?: boolean }) {
    const attachments = useMemo(() => attachmentTiles(html, names), [html, names]);
    return <div className={`attachment-gallery${video ? " attachment-video" : ""}`}>
        {attachments.map((file, index) => <figure className="attachment-tile" key={`${index}:${file.name}`} title={file.name}>
            {file.src ? <img src={file.src} alt={file.name} loading="lazy" draggable={false} />
                : video ? <Video size={24} /> : /\.(png|jpe?g|gif|webp|bmp|avif)$/i.test(file.name) ? <ImageIcon size={24} /> : <File size={24} />}
            <figcaption>{file.name}</figcaption>
        </figure>)}
    </div>;
}
