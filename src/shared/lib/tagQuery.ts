export function parseTagFilter(query: string): string | null {
    const match = /^\s*tag:([\s\S]*)$/i.exec(query);
    return match ? match[1].trim() : null;
}

export function matchesTagName(name: string, query: string): boolean {
    const needle = (parseTagFilter(query) ?? query).trim().toLocaleLowerCase();
    return name.toLocaleLowerCase().includes(needle);
}
