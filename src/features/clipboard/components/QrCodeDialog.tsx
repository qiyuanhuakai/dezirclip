import { forwardRef, useEffect, useCallback } from "react";
import { invoke } from "@tauri-apps/api/core";
import type { ClipboardEntry } from "../../../shared/types";
import { useQrCode } from "../hooks/useQrCode";

interface QrCodeDialogProps {
  entry: ClipboardEntry;
  onClose: () => void;
}

const MAX_CONTENT_DISPLAY = 60;

const QrCodeDialog = forwardRef<HTMLDivElement, QrCodeDialogProps>(
  ({ entry, onClose }, ref) => {
    const { dataUrl, loading, error, generate } = useQrCode();

    useEffect(() => {
      generate(entry.content, 256);
    }, [entry.content, generate]);

    const handleKeyDown = useCallback(
      (e: KeyboardEvent) => {
        if (e.key === "Escape") onClose();
      },
      [onClose]
    );

    useEffect(() => {
      document.addEventListener("keydown", handleKeyDown);
      return () => document.removeEventListener("keydown", handleKeyDown);
    }, [handleKeyDown]);

    const handleBackdropClick = useCallback(
      (e: React.MouseEvent) => {
        if (e.target === e.currentTarget) onClose();
      },
      [onClose]
    );

    const handleCopySvg = useCallback(async () => {
      try {
        const svg = await invoke<string>("generate_qr_svg", {
          content: entry.content,
        });
        await invoke("copy_to_clipboard", {
          content: svg,
          contentType: "text",
          paste: false,
          id: 0,
          deleteAfterUse: false,
          pasteWithFormat: false,
          moveToTop: false,
          pasteImageAsBase64: false,
        });
      } catch {
        // clipboard write failure is non-critical
      }
    }, [entry.content]);

    const truncatedContent =
      entry.content.length > MAX_CONTENT_DISPLAY
        ? `${entry.content.slice(0, MAX_CONTENT_DISPLAY)}...`
        : entry.content;

    return (
      <div
        className="qr-dialog-overlay"
        onClick={handleBackdropClick}
        role="presentation"
      >
        <div
          ref={ref}
          className="qr-dialog"
          role="dialog"
          aria-modal="true"
          aria-label="二维码"
        >
          <div className="qr-dialog__title">二维码</div>

          {loading && (
            <div className="qr-dialog__loading">生成中...</div>
          )}

          {error && <div className="qr-dialog__error">{error}</div>}

          {dataUrl && !loading && (
            <div className="qr-dialog__image">
              <img src={dataUrl} alt="QR Code" width={256} height={256} />
            </div>
          )}

          <div className="qr-dialog__content" title={entry.content}>
            {truncatedContent}
          </div>

          <div className="qr-dialog__actions">
            <button
              className="qr-dialog__btn"
              onClick={onClose}
              type="button"
            >
              关闭
            </button>
            <button
              className="qr-dialog__btn qr-dialog__btn--primary"
              onClick={handleCopySvg}
              type="button"
            >
              复制 SVG
            </button>
          </div>
        </div>
      </div>
    );
  }
);

QrCodeDialog.displayName = "QrCodeDialog";

export default QrCodeDialog;
