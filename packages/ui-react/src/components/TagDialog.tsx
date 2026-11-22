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

export interface TagConfirmationProps extends Pick<TagDialogProps, 'title' | 'closeLabel' | 'busy' | 'onClose'> {
    message: string;
    cancelLabel: string;
    confirmLabel: string;
    onConfirm: () => void;
    error?: string | null;
}

export function TagConfirmation({ message, cancelLabel, confirmLabel, onConfirm, error, ...dialog }: TagConfirmationProps) {
    return <TagDialog {...dialog}>
        <form onSubmit={event => { event.preventDefault(); if (!dialog.busy) onConfirm(); }}>
            <p className="confirm-dialog-message">{message}</p>
            {error && <p className="tag-manager-dialog-error" role="alert">{error}</p>}
            <div className="confirm-dialog-buttons">
                <button type="button" className="confirm-dialog-button" disabled={dialog.busy} onClick={dialog.onClose}>{cancelLabel}</button>
                <button type="submit" className="confirm-dialog-button primary" disabled={dialog.busy}>{confirmLabel}</button>
            </div>
        </form>
    </TagDialog>;
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
