import React, { useRef, useImperativeHandle, useCallback, useMemo } from 'react';
import { Virtuoso, VirtuosoHandle } from 'react-virtuoso';
import type { ListRange } from 'react-virtuoso';
import type { ClipboardEntry } from "../../../shared/types";
import { createScrollCoalescer } from "../../../shared/lib/scrollCoalesce";
import type { VirtualClipboardListHandle, VirtualClipboardListProps } from "../types";

type VirtuosoListContext = {
    header?: React.ReactNode;
    hasMore: boolean;
    isLoading: boolean;
};

const ListHeader = ({ context }: { context?: VirtuosoListContext }) => {
    const header = context?.header;
    return header ? <div className="list-header">{header}</div> : null;
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
            compactMode,
            header
        } = props;

        const virtuosoRef = useRef<VirtuosoHandle>(null);
        const wrapperRef = useRef<HTMLDivElement>(null);
        const visibleRangeRef = useRef<ListRange | null>(null);
        useImperativeHandle(ref, () => ({
            scrollToItem: (index: number) => {
                virtuosoRef.current?.scrollIntoView({
                    index,
                    behavior: 'smooth',
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
            Header: ListHeader,
            Footer: ListFooter
        }), []);

        const context = useMemo(() => ({
            header,
            hasMore,
            isLoading
        }), [header, hasMore, isLoading]);

        return (
            <div
                className="virtual-list-wrapper"
                ref={wrapperRef}
                style={{ height: '100%', width: '100%' }}
            >
                <Virtuoso
                    ref={virtuosoRef}
                    data={items}
                    itemContent={itemContent}
                    components={components}
                    context={context}
                    style={{ height: '100%' }}
                    endReached={handleEndReached}
                    rangeChanged={handleRangeChanged}
                    overscan={200} // Pre-render 200px of content for smoother scrolling
                />
            </div>
        );
    }
);

VirtualClipboardList.displayName = 'VirtualClipboardList';

export { VirtualClipboardList };
