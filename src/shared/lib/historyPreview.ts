import type { ClipboardEntry } from "../types";
import { isSensitiveEntry } from "./privacyRuntime";

export type HistoryPreviewPhase = "loading" | "ready" | "error";
export type HistoryPreviewContent = Pick<ClipboardEntry, "preview" | "html_content">;
const readableCharacter = /[^\s\p{Cc}\p{Cf}\p{Default_Ignorable_Code_Point}]/u;

export function hasHistoryPreviewText(value: string): boolean {
    return readableCharacter.test(value);
}

export function shouldLoadHistoryPreview(entry: ClipboardEntry, compact: boolean): boolean {
    if (isSensitiveEntry(entry)) return false;
    if (entry.content_type === "rich_text" && !compact && !entry.html_content) return true;
    return ["text", "code", "url", "rich_text"].includes(entry.content_type) && !hasHistoryPreviewText(entry.preview);
}

function prefix(value: string, limit: number): string {
    let result = "";
    let length = 0;
    for (const character of value) {
        if (length++ === limit) break;
        result += character;
    }
    return result;
}

export function projectHistoryPreview(entry: ClipboardEntry): HistoryPreviewContent {
    if (isSensitiveEntry(entry)) throw new Error("ProtectedPreviewRequiresReveal");
    const summary = hasHistoryPreviewText(entry.preview) ? entry.preview : entry.content;
    const first = readableCharacter.exec(summary);
    return { preview: first ? prefix(summary.slice(first.index), 500) : "", html_content: entry.html_content };
}
