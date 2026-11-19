import { useEffect, useId, useRef, type ReactNode, type RefObject } from 'react';
import { X } from 'lucide-react';

export interface TagDialogProps {
    title: string;
    closeLabel: string;
    busy: boolean;
    onClose: () => void;
    children: ReactNode;
    wide?: boolean;
    initialFocus?: RefObject<HTMLElement | null>;
}

export function TagDialog({ title, closeLabel, busy, onClose, children, wide = false, initialFocus }: TagDialogProps) {
    const dialog = useRef<HTMLDialogElement>(null);
    const titleId = useId();
    useEffect(() => {
        const element = dialog.current;
        element?.showModal();
        if (element && initialFocus?.current && element.contains(initialFocus.current)) {
            initialFocus.current.focus();
        }
        return () => element?.close();
    }, [initialFocus]);

    return (
        <dialog ref={dialog} className={`confirm-dialog tag-manager-dialog${wide ? ' tag-manager-dialog--wide' : ''}`}
            aria-labelledby={titleId} aria-busy={busy}
            onCancel={event => { event.preventDefault(); if (!busy) onClose(); }}>
            <div className="tag-manager-dialog-heading">
                <h2 className="confirm-dialog-title" id={titleId}>{title}</h2>
                <button type="button" className="btn-icon" onClick={onClose} disabled={busy}
                    aria-label={closeLabel} title={closeLabel}><X size={18} /></button>
            </div>
            {children}
        </dialog>
    );
}
