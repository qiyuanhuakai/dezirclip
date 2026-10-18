import type { ClipboardEntry } from '../../shared/types';

export interface TagInfo {
    name: string;
    count: number;
}

export type TagSort = 'time' | 'count';

export interface TagItemsPage {
    items: ClipboardEntry[];
    total: number;
    has_more: boolean;
    next_offset: number | null;
}
