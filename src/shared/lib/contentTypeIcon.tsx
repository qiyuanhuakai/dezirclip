import {
    FileText,
    Image as ImageIcon,
    Link as LinkIcon,
    Code,
    File,
    Video
} from "lucide-react";

const DEFAULT_ICON_SIZE = 14;

export const getContentTypeIcon = (type: string, size: number = DEFAULT_ICON_SIZE) => {
    switch (type) {
        case "text": return <FileText size={size} />;
        case "image": return <ImageIcon size={size} />;
        case "url": return <LinkIcon size={size} />;
        case "code": return <Code size={size} />;
        case "file": return <File size={size} />;
        case "video": return <Video size={size} />;
        default: return <FileText size={size} />;
    }
};
