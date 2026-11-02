import React, { useRef, useImperativeHandle, useCallback, useMemo } from 'react';
import { Virtuoso, VirtuosoHandle } from 'react-virtuoso';
import type { ListRange, ScrollSeekPlaceholderProps } from 'react-virtuoso';
import type { ClipboardEntry } from "../../../shared/types";
import { createScrollCoalescer } from "../../../shared/lib/scrollCoalesce";
import { isSensitiveEntry } from "../../../shared/lib/privacyRuntime";
import type { VirtualClipboardListHandle, VirtualClipboardListProps } from "../types";

type VirtuosoListContext = {
    items: ClipboardEntry[];
    hasMore: boolean;
    isLoading: boolean;
};

const ScrollingRow = ({ index, height, context }: ScrollSeekPlaceholderProps & { context?: VirtuosoListContext }) => {
    const item = context?.items[index];
    const summary = item && !isSensitiveEntry(item) && item.content_type === "text" ? item.preview.slice(0, 160) : "•••";
    return <div className="history-scroll-preview" style={{ height }} aria-hidden="true">{summary}</div>;
};

const fastScroll = {
    enter: (velocity: number) => Math.abs(velocity) > 1200,
    exit: (velocity: number) => Math.abs(velocity) < 350,
};

const ListFooter = ({ context }: { context?: VirtuosoListContext }) => {
    if (!context) return null;
    const { isLoading, hasMore } = context;
    if (!isLoading && !hasMore) return null;

    return (
        <div style={{
            padding: '20px',
            textAlign: 'center',
            opacity: 0.6,
            fontSize: '12px',
            color: 'var(--text-secondary)'
        }}>
            {isLoading ? '加载中...' : '加载更多...'}
        </div>
    );
};

const VirtualClipboardList = React.forwardRef<VirtualClipboardListHandle, VirtualClipboardListProps>(
    (props, ref) => {
        const {
            items,
            renderItem,
            onLoadMore,
            hasMore,
            isLoading,
            selectedIndex,
            isKeyboardMode,
            onScroll,
            onScrollFrame,
            compactMode
        } = props;

        const virtuosoRef = useRef<VirtuosoHandle>(null);
        const wrapperRef = useRef<HTMLDivElement>(null);
        const visibleRangeRef = useRef<ListRange | null>(null);
        useImperativeHandle(ref, () => ({
            scrollToItem: (index: number) => {
                // `behavior: 'smooth'` animated this one-row keyboard step, and a
                // step is exactly the thing an animation is wrong for. The list
                // glided to the centre over roughly thirty frames, each one
                // dispatching a scroll event, each one running the coalescer's
                // listener, and each one reading `scrollTop` -- a forced layout,
                // because a frame's layout has not been computed yet when its
                // scroll event is delivered. Holding an arrow key overlapped
                // several of those animations, and the effect further down was
                // asking for an instant `scrollToIndex` on the very same keypress,
                // so the two were pulling against each other.
                //
                // The destination is unchanged: still this index, still
                // `align: 'center'`. Only the flight is gone, and the edge logic
                // below already keeps the selection on screen instantly, so
                // nothing scrolls that does not have to.
                virtuosoRef.current?.scrollIntoView({
                    index,
                    behavior: 'auto',
                    align: 'center',
                });
            },
            scrollToTop: () => {
                virtuosoRef.current?.scrollTo({
                    top: 0,
                    behavior: 'auto'
                });
            },
            resetAfterIndex: (_index: number) => {
                // Not needed with Virtuoso as it handles dynamic heights automatically
            }
        }));

        // Keep keyboard selection visible even when the item is only in overscan
        React.useEffect(() => {
            if (!isKeyboardMode || selectedIndex < 0) return;

            const range = visibleRangeRef.current;
            const edgeBuffer = 1;

            if (!range) {
                virtuosoRef.current?.scrollToIndex({
                    index: selectedIndex,
                    behavior: 'auto',
                    align: 'center',
                });
                return;
            }

            if (selectedIndex < range.startIndex + edgeBuffer) {
                virtuosoRef.current?.scrollToIndex({
                    index: selectedIndex,
                    behavior: 'auto',
                    align: 'start',
                });
                return;
            }

            if (selectedIndex > range.endIndex - edgeBuffer) {
                virtuosoRef.current?.scrollToIndex({
                    index: selectedIndex,
                    behavior: 'auto',
                    align: 'end',
                });
            }
        }, [selectedIndex, isKeyboardMode]);


        // Handle scroll events
        const handleScroll = useCallback((scrollTop: number) => {
            onScroll?.(scrollTop);
        }, [onScroll]);

        const handleScrollFrame = useCallback((scrollTop: number) => {
            onScrollFrame?.(scrollTop);
        }, [onScrollFrame]);

        // A fling fires scroll events faster than the display refreshes, so the
        // scroller is observed with a passive native listener instead of a React
        // onScroll prop: the wheel heuristic still gets every offset as it
        // happens, while anything that can trigger a re-render is held to one
        // call per frame. The scroller is looked up through Virtuoso's own
        // documented test hook rather than by taking over its scrollerRef, which
        // would make this component responsible for the element Virtuoso owns.
        React.useEffect(() => {
            const scroller = wrapperRef.current?.querySelector<HTMLElement>(
                '[data-testid="virtuoso-scroller"]'
            );
            if (!scroller) return;
            const coalescer = createScrollCoalescer(handleScroll, handleScrollFrame);
            coalescer.attach(scroller);
            return () => coalescer.detach();
        }, [handleScroll, handleScrollFrame]);

        // Handle end reached for infinite loading
        const handleEndReached = useCallback(() => {
            if (hasMore && !isLoading && onLoadMore) {
                onLoadMore();
            }
        }, [hasMore, isLoading, onLoadMore]);

        const handleRangeChanged = useCallback((range: ListRange) => {
            visibleRangeRef.current = range;
        }, []);

        // Memoized item renderer for Virtuoso
        const itemContent = useCallback((index: number, item: ClipboardEntry) => {
            return (
                <div style={{ paddingBottom: compactMode ? 2 : 4 }}>
                    {renderItem(item, index, index === 0)}
                </div>
            );
        }, [compactMode, renderItem]);

        const components = useMemo(() => ({
            ScrollSeekPlaceholder: ScrollingRow,
            Footer: ListFooter
        }), []);

        const context = useMemo(() => ({
            items,
            hasMore,
            isLoading
        }), [items, hasMore, isLoading]);

        return (
            <div
                className="virtual-list-wrapper"
                ref={wrapperRef}
                style={{ height: '100%', width: '100%' }}
            >
                <Virtuoso
                    ref={virtuosoRef}
                    data={items}
                    computeItemKey={(_index, item) => item.id}
                    itemContent={itemContent}
                    components={components}
                    context={context}
                    style={{ height: '100%' }}
                    endReached={handleEndReached}
                    rangeChanged={handleRangeChanged}
                    scrollSeekConfiguration={fastScroll}
                    overscan={120}
                />
            </div>
        );
    }
);

VirtualClipboardList.displayName = 'VirtualClipboardList';

export { VirtualClipboardList };
