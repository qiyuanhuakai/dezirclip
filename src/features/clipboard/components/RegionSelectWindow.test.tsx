import { describe, it, expect, vi, beforeEach } from "vitest";
import { render, fireEvent, screen, act } from "@testing-library/react";
import RegionSelectWindow from "./RegionSelectWindow";

vi.mock("@tauri-apps/api/core", () => ({
  invoke: vi.fn(),
}));

import { invoke } from "@tauri-apps/api/core";
const mockInvoke = vi.mocked(invoke);

vi.mock("../../../shared/lib/themeRuntime", () => ({
  applyThemeClass: vi.fn(),
  applyModeClass: vi.fn(),
}));

describe("RegionSelectWindow", () => {
  beforeEach(() => {
    vi.clearAllMocks();
    Storage.prototype.getItem = vi.fn(() => null);
  });

  it("renders overlay element", () => {
    render(<RegionSelectWindow />);
    const overlay = screen.getByTestId("region-select-overlay");
    expect(overlay).toBeInTheDocument();
    expect(overlay).toHaveClass("region-select-window");
  });

  it("mousedown starts selection", () => {
    render(<RegionSelectWindow />);
    const overlay = screen.getByTestId("region-select-overlay");

    act(() => {
      fireEvent.mouseDown(overlay, { clientX: 100, clientY: 100 });
    });

    // After mousedown with no move, selection is below MIN so box not rendered
    expect(screen.queryByTestId("region-select-box")).not.toBeInTheDocument();
  });

  it("mousemove updates size during drag", () => {
    render(<RegionSelectWindow />);
    const overlay = screen.getByTestId("region-select-overlay");

    act(() => {
      fireEvent.mouseDown(overlay, { clientX: 100, clientY: 100 });
    });

    act(() => {
      fireEvent.mouseMove(overlay, { clientX: 420, clientY: 300 });
    });

    const box = screen.getByTestId("region-select-box");
    expect(box).toBeInTheDocument();

    const dims = screen.getByTestId("region-select-dimensions");
    expect(dims).toHaveTextContent("320 × 200");
  });

  it("mouseup completes selection and fires onSelect with correct coords", async () => {
    const onSelect = vi.fn();
    render(<RegionSelectWindow onSelect={onSelect} />);
    const overlay = screen.getByTestId("region-select-overlay");

    act(() => {
      fireEvent.mouseDown(overlay, { clientX: 100, clientY: 100 });
    });

    act(() => {
      fireEvent.mouseMove(overlay, { clientX: 420, clientY: 300 });
    });

    await act(async () => {
      fireEvent.mouseUp(overlay);
    });

    expect(mockInvoke).toHaveBeenCalledWith("capture_region", {
      x: 100,
      y: 100,
      width: 320,
      height: 200,
    });
    expect(onSelect).toHaveBeenCalledWith({ x: 100, y: 100, width: 320, height: 200 });
  });

  it("escape cancels and fires onCancel", () => {
    const onCancel = vi.fn();
    render(<RegionSelectWindow onCancel={onCancel} />);

    act(() => {
      fireEvent.keyDown(window, { key: "Escape" });
    });

    expect(onCancel).toHaveBeenCalledTimes(1);
  });

  it("small selection is ignored (below MIN_SELECTION_SIZE)", async () => {
    const onSelect = vi.fn();
    render(<RegionSelectWindow onSelect={onSelect} />);
    const overlay = screen.getByTestId("region-select-overlay");

    act(() => {
      fireEvent.mouseDown(overlay, { clientX: 100, clientY: 100 });
    });

    act(() => {
      fireEvent.mouseMove(overlay, { clientX: 105, clientY: 105 });
    });

    await act(async () => {
      fireEvent.mouseUp(overlay);
    });

    expect(mockInvoke).not.toHaveBeenCalled();
    expect(onSelect).not.toHaveBeenCalled();
  });

  it("cancel then resume resets state properly", async () => {
    const onSelect = vi.fn();
    const onCancel = vi.fn();
    render(<RegionSelectWindow onSelect={onSelect} onCancel={onCancel} />);
    const overlay = screen.getByTestId("region-select-overlay");

    // First drag
    act(() => {
      fireEvent.mouseDown(overlay, { clientX: 50, clientY: 50 });
    });
    act(() => {
      fireEvent.mouseMove(overlay, { clientX: 200, clientY: 200 });
    });

    // Cancel with ESC
    act(() => {
      fireEvent.keyDown(window, { key: "Escape" });
    });
    expect(onCancel).toHaveBeenCalledTimes(1);
    expect(screen.queryByTestId("region-select-box")).not.toBeInTheDocument();

    // Resume — new drag
    act(() => {
      fireEvent.mouseDown(overlay, { clientX: 10, clientY: 10 });
    });
    act(() => {
      fireEvent.mouseMove(overlay, { clientX: 310, clientY: 210 });
    });

    const dims = screen.getByTestId("region-select-dimensions");
    expect(dims).toHaveTextContent("300 × 200");

    await act(async () => {
      fireEvent.mouseUp(overlay);
    });

    expect(onSelect).toHaveBeenCalledWith({ x: 10, y: 10, width: 300, height: 200 });
  });

  it("right-to-left drag normalizes coordinates", async () => {
    const onSelect = vi.fn();
    render(<RegionSelectWindow onSelect={onSelect} />);
    const overlay = screen.getByTestId("region-select-overlay");

    act(() => {
      fireEvent.mouseDown(overlay, { clientX: 400, clientY: 300 });
    });

    act(() => {
      fireEvent.mouseMove(overlay, { clientX: 100, clientY: 100 });
    });

    const dims = screen.getByTestId("region-select-dimensions");
    expect(dims).toHaveTextContent("300 × 200");

    await act(async () => {
      fireEvent.mouseUp(overlay);
    });

    expect(onSelect).toHaveBeenCalledWith({ x: 100, y: 100, width: 300, height: 200 });
  });
});
