import type { ProgressToastItem } from "../hooks/useProgress";

interface ProgressToastProps {
  toasts: ProgressToastItem[];
  onDismiss: (id: string) => void;
}

const ProgressToast = ({ toasts, onDismiss }: ProgressToastProps) => {
  if (toasts.length === 0) return null;

  return (
    <div className="progress-toast-container">
      {toasts.map((toast, index) => {
        const percent = Math.min(100, Math.round(toast.progress * 100));
        const barWidth =
          toast.status === "done"
            ? "100%"
            : toast.status === "error"
              ? "0%"
              : `${percent}%`;

        return (
          <div
            key={toast.id}
            className={`progress-toast progress-toast--${toast.status}`}
            style={{ zIndex: 10000 - index }}
            data-testid={`progress-toast-${toast.id}`}
          >
            <div className="progress-toast-header">
              <span className="progress-toast-message">{toast.message}</span>
              {toast.status === "running" && (
                <span className="progress-toast-percent">{percent}%</span>
              )}
              {toast.status === "done" && (
                <span className="progress-toast-status">✓</span>
              )}
              {toast.status === "error" && (
                <button
                  className="progress-toast-close"
                  onClick={() => onDismiss(toast.id)}
                  aria-label="Dismiss"
                >
                  ×
                </button>
              )}
            </div>
            <div className="progress-toast-bar-track">
              <div
                className="progress-toast-bar-fill"
                style={{ width: barWidth }}
              />
            </div>
          </div>
        );
      })}
    </div>
  );
};

export default ProgressToast;
