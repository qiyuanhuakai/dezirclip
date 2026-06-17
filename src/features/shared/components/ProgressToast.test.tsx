import { describe, it, expect, vi, beforeEach, afterEach } from "vitest";
import { render, screen, act } from "@testing-library/react";
import ProgressToast from "./ProgressToast";
import type { ProgressToastItem } from "../hooks/useProgress";

const listeners: Record<string, (event: { payload: unknown }) => void> = {};

vi.mock("@tauri-apps/api/event", () => ({
  listen: vi.fn(
    (event: string, handler: (event: { payload: unknown }) => void) => {
      listeners[event] = handler;
      return Promise.resolve(() => {
        delete listeners[event];
      });
    }
  ),
}));

describe("ProgressToast", () => {
  beforeEach(() => {
    vi.useFakeTimers();
  });

  afterEach(() => {
    vi.useRealTimers();
    vi.restoreAllMocks();
  });

  it("test_renders_empty: no events → no toast visible", () => {
    render(<ProgressToast toasts={[]} onDismiss={vi.fn()} />);
    expect(screen.queryByTestId(/progress-toast-/)).not.toBeInTheDocument();
  });

  it("test_renders_running_toast: emit 'running' event → toast appears with 0% progress", () => {
    const { unmount } = render(
      <ProgressToast toasts={[]} onDismiss={vi.fn()} />
    );
    unmount();

    const { rerender } = render(
      <ProgressToast toasts={[]} onDismiss={vi.fn()} />
    );

    const toasts: ProgressToastItem[] = [];
    const updateToasts = (
      payload: Partial<ProgressToastItem> & { id: string }
    ) => {
      toasts.push({
        id: payload.id,
        message: payload.message ?? "",
        progress: payload.progress ?? 0,
        status: payload.status ?? "running",
      });
      rerender(<ProgressToast toasts={[...toasts]} onDismiss={vi.fn()} />);
    };

    updateToasts({ id: "t1", message: "Syncing...", status: "running", progress: 0 });

    expect(screen.getByText("Syncing...")).toBeInTheDocument();
    expect(screen.getByText("0%")).toBeInTheDocument();
  });

  it("test_renders_done_toast: emit 'done' event → 100% bar, auto-dismiss after 3s", () => {
    const onDismiss = vi.fn();
    const toasts: ProgressToastItem[] = [
      { id: "t1", message: "Complete", progress: 1, status: "done" },
    ];

    const { rerender } = render(
      <ProgressToast toasts={toasts} onDismiss={onDismiss} />
    );

    expect(screen.getByText("Complete")).toBeInTheDocument();
    expect(screen.getByText("✓")).toBeInTheDocument();

    const bar = document.querySelector(".progress-toast-bar-fill") as HTMLElement;
    expect(bar.style.width).toBe("100%");

    act(() => {
      vi.advanceTimersByTime(3000);
    });

    toasts.length = 0;
    rerender(<ProgressToast toasts={[]} onDismiss={onDismiss} />);

    expect(screen.queryByText("Complete")).not.toBeInTheDocument();
  });

  it("test_renders_error_toast: emit 'error' event → error border, no auto-dismiss", () => {
    const toasts: ProgressToastItem[] = [
      { id: "t1", message: "Failed", progress: 0, status: "error" },
    ];

    const { container } = render(
      <ProgressToast toasts={toasts} onDismiss={vi.fn()} />
    );

    const toastEl = container.querySelector(".progress-toast--error");
    expect(toastEl).toBeInTheDocument();
    expect(screen.getByText("Failed")).toBeInTheDocument();

    act(() => {
      vi.advanceTimersByTime(10000);
    });

    expect(screen.getByText("Failed")).toBeInTheDocument();
  });

  it("malformed_input: missing fields → component does not crash", () => {
    const toasts = [
      { id: "t1", message: "", progress: 0, status: "running" as const },
    ];

    expect(() => {
      render(<ProgressToast toasts={toasts} onDismiss={vi.fn()} />);
    }).not.toThrow();
  });

  it("cancel_resume: 'done' then 'running' for same id → shows running", () => {
    const toasts: ProgressToastItem[] = [
      { id: "t1", message: "Working", progress: 0.5, status: "running" },
    ];

    const { rerender } = render(
      <ProgressToast toasts={toasts} onDismiss={vi.fn()} />
    );

    toasts[0] = { id: "t1", message: "Working", progress: 1, status: "done" };
    rerender(<ProgressToast toasts={[...toasts]} onDismiss={vi.fn()} />);
    expect(screen.getByText("✓")).toBeInTheDocument();

    toasts[0] = { id: "t1", message: "Working again", progress: 0.3, status: "running" };
    rerender(<ProgressToast toasts={[...toasts]} onDismiss={vi.fn()} />);
    expect(screen.getByText("Working again")).toBeInTheDocument();
    expect(screen.getByText("30%")).toBeInTheDocument();
  });

  it("stale_state: 'done' for old id → does not affect current toasts", () => {
    const toasts: ProgressToastItem[] = [
      { id: "t2", message: "New", progress: 0.2, status: "running" },
    ];

    const { rerender } = render(
      <ProgressToast toasts={toasts} onDismiss={vi.fn()} />
    );

    toasts[0] = { id: "t2", message: "New", progress: 0.8, status: "running" };
    rerender(<ProgressToast toasts={[...toasts]} onDismiss={vi.fn()} />);

    expect(screen.getByText("New")).toBeInTheDocument();
    expect(screen.getByText("80%")).toBeInTheDocument();
  });
});
