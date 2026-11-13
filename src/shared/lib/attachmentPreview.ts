export interface AttachmentTile { name: string; src: string | null }
export function attachmentKind(name: string): string {
    const extension = name.split('.').pop()?.toLowerCase() ?? '';
    for (const [kind, extensions] of Object.entries({
        image: ['png', 'jpg', 'jpeg', 'gif', 'webp', 'bmp', 'avif', 'svg', 'ico', 'tiff'],
        video: ['mp4', 'mov', 'mkv', 'webm', 'avi', 'wmv', 'm4v'],
        audio: ['mp3', 'wav', 'flac', 'ogg', 'm4a', 'aac'],
        archive: ['zip', '7z', 'rar', 'gz', 'tar', 'bz2', 'xz'],
        spreadsheet: ['xls', 'xlsx', 'csv', 'ods'], presentation: ['ppt', 'pptx', 'odp'],
        document: ['pdf', 'doc', 'docx', 'txt', 'md', 'rtf', 'odt'],
        code: ['rs', 'go', 'js', 'ts', 'tsx', 'py', 'json', 'html', 'css', 'xml', 'sh'],
    })) if (extensions.includes(extension)) return kind;
    return 'file';
}

export function attachmentLayout(attachments: AttachmentTile[], width: number, expanded: boolean, compact = false) {
    const media = attachments.some(file => file.src);
    const columns = Math.max(1, Math.min(4, Math.floor((Math.max(width, 1) + 6) / (media ? compact && !expanded ? 78 : 116 : 134))));
    const rows = expanded ? 8 : compact ? 2 : 4;
    const capacity = media ? columns : expanded ? rows : columns * rows;
    const overflow = !media && attachments.length > capacity;
    const visible = attachments.slice(0, overflow ? Math.max(0, capacity - 1) : capacity);
    const remaining = attachments.slice(visible.length);
    const nameCapacity = expanded ? 6 : columns;
    const names = media ? remaining.slice(0, remaining.length > nameCapacity ? nameCapacity - 1 : nameCapacity) : [];
    return { media, columns: !media && expanded ? 1 : columns, visible, names, omitted: remaining.length - names.length };
}
const entities: Record<string, string> = { "&amp;": "&", "&lt;": "<", "&gt;": ">", "&quot;": '"', "&#39;": "'" };
const unescape = (value: string) => value.replace(/&amp;|&lt;|&gt;|&quot;|&#39;/g, entity => entities[entity]);

// Parse only our generated gallery grammar. Do not create a DOM document, which
// can fetch resources from untrusted markup even before it is mounted.
export function attachmentTiles(html: string | null | undefined, names: string): AttachmentTile[] {
    if (html && html.length <= 2 * 1024 * 1024) {
        const pattern = /<figure>(?:<img src="(data:image\/png;base64,[A-Za-z0-9+/=]+)" alt="[^"]*">)?<figcaption>([^<]*)<\/figcaption><\/figure>/g;
        const tiles: AttachmentTile[] = [];
        let offset = 0;
        for (const match of html.matchAll(pattern)) {
            if (match.index !== offset || tiles.length >= 256) break;
            tiles.push({ name: unescape(match[2]), src: match[1] ?? null });
            offset += match[0].length;
        }
        if (offset === html.length && tiles.length) return tiles;
    }
    return names.split("\n").filter(Boolean).slice(0, 256).map(value => ({ name: value.split(/[\\/]/).pop() ?? value, src: null }));
}
