export interface AttachmentTile { name: string; src: string | null }
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
