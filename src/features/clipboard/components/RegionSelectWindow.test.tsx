import { describe, it, expect, vi, beforeEach, beforeAll, afterAll } from "vitest";
import { render, fireEvent, screen, act, waitFor } from "@testing-library/react";
import RegionSelectWindow, { toPhysicalCaptureRect } from "./RegionSelectWindow";

vi.mock("@tauri-apps/api/core", () => ({
  invoke: vi.fn(),
}));
vi.mock("@tauri-apps/api/event", () => ({ listen: vi.fn(async () => () => {}) }));

const mockHide = vi.fn();
const mockSetFocusable = vi.fn();
const mockOuterPosition = vi.fn();
const mockScaleFactor = vi.fn();
vi.mock("@tauri-apps/api/window", () => ({
  getCurrentWindow: () => ({
    hide: mockHide,
    setFocusable: mockSetFocusable,
    outerPosition: mockOuterPosition,
    scaleFactor: mockScaleFactor,
    onFocusChanged: async () => () => {},
    onScaleChanged: async () => () => {},
    isVisible: async () => true,
  }),
}));

import { invoke } from "@tauri-apps/api/core";
const mockInvoke = vi.mocked(invoke);
const originalDecode = Object.getOwnPropertyDescriptor(HTMLImageElement.prototype, "decode");
let imageWidth = 1024;
let imageHeight = 768;
let sessionGeneration = 1;
const renderReady = async (element: Parameters<typeof render>[0]) => {
  const result = render(element);
  await waitFor(() => expect(screen.getByTestId("region-select-overlay")).toHaveAttribute("data-ready", "true"));
  return result;
};

// systemTheme re-exports the pure helpers from here, so a partial factory that
// omits them leaves `readSystemIsDark` and friends holding undefined. The two
// stubs below carry real behaviour rather than bare vi.fn() for that reason.
vi.mock("../../../shared/lib/themeRuntime", () => ({
  applyThemeClass: vi.fn(),
  applyModeClass: vi.fn(),
  mediaSystemIsDark: () => false,
  normalizeColorMode: (raw: string) =>
    raw === "light" || raw === "dark" ? raw : "system"
}));

// Appearance sync is orthogonal to the drag geometry under test here, and it
// issues its own `invoke` calls on mount that would otherwise leak into the
// command assertions below.
vi.mock("../../../shared/hooks/useBackendAppearance", () => ({
  useBackendAppearance: vi.fn(),
}));

describe("RegionSelectWindow", () => {
  beforeEach(() => {
    vi.clearAllMocks();
    sessionGeneration = 1;
    mockHide.mockResolvedValue(undefined);
    mockSetFocusable.mockResolvedValue(undefined);
    mockOuterPosition.mockResolvedValue({ x: 0, y: 0 });
    mockScaleFactor.mockResolvedValue(1);
    mockInvoke.mockImplementation(async command => {
      if (command === "get_region_capture_session") {
        const scale = await mockScaleFactor();
        imageWidth = Math.round(window.innerWidth * scale);
        imageHeight = Math.round(window.innerHeight * scale);
        return { token: sessionGeneration === 1 ? "synthetic-session" : "synthetic-session-2", ...await mockOuterPosition(), scale, width: imageWidth, height: imageHeight, generation: sessionGeneration, presented: false };
      }
      if (command === "get_region_capture_frame") return new Uint8Array([137, 80, 78, 71]).buffer;
      return undefined;
    });
    Storage.prototype.getItem = vi.fn(() => null);
  });

  // Live drag geometry is written from a requestAnimationFrame callback so a
  // thousand `mousemove` events a second cost one repaint rather than a
  // thousand re-renders. Running the frame synchronously is enough to assert
  // on what the user would see, and keeps the `waitFor` assertions below on
  // real timers.
  beforeAll(() => {
    Object.defineProperty(HTMLImageElement.prototype, "decode", { configurable: true, value: () => Promise.resolve() });
    vi.spyOn(HTMLImageElement.prototype, "getBoundingClientRect").mockImplementation(() => new DOMRect(0, 0, window.innerWidth, window.innerHeight));
    vi.stubGlobal("Image", class {
      src = "";
      get naturalWidth() { return imageWidth; }
      get naturalHeight() { return imageHeight; }
      decode() { return Promise.resolve(); }
    });
    vi.stubGlobal("URL", class extends URL {
      static createObjectURL = vi.fn(() => "blob:synthetic-frame");
      static revokeObjectURL = vi.fn();
    });
    vi.stubGlobal("requestAnimationFrame", (cb: FrameRequestCallback) => {
      cb(0);
      return 1;
    });
    vi.stubGlobal("cancelAnimationFrame", () => {});
  });

  afterAll(() => {
    if (originalDecode) Object.defineProperty(HTMLImageElement.prototype, "decode", originalDecode);
    else Reflect.deleteProperty(HTMLImageElement.prototype, "decode");
    vi.restoreAllMocks();
    vi.unstubAllGlobals();
  });

  it("renders overlay element", async () => {
    await renderReady(<RegionSelectWindow />);
    const overlay = screen.getByTestId("region-select-overlay");
    expect(overlay).toBeInTheDocument();
    expect(overlay).toHaveClass("region-select-window");
  });

  it("mousedown starts selection", async () => {
    await renderReady(<RegionSelectWindow />);
    const overlay = screen.getByTestId("region-select-overlay");

    act(() => {
      fireEvent.mouseDown(overlay, { clientX: 100, clientY: 100 });
    });

    // After mousedown with no move, selection is below MIN so box not rendered
    expect(screen.queryByTestId("region-select-box")).not.toBeInTheDocument();
  });

  it("mousemove updates size during drag", async () => {
    await renderReady(<RegionSelectWindow />);
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
    await renderReady(<RegionSelectWindow onSelect={onSelect} />);
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

    await waitFor(() => {
      expect(mockInvoke).toHaveBeenCalledWith("capture_region", {
        sessionToken: "synthetic-session",
        x: 100,
        y: 100,
        width: 320,
        height: 200,
      });
    });
    expect(onSelect).toHaveBeenCalledWith({ x: 100, y: 100, width: 320, height: 200 });
    expect(mockInvoke).toHaveBeenCalledWith("hide_region_select", { sessionToken: "synthetic-session" });
  });

  it("converts CSS selection coordinates to physical screen coordinates", async () => {
    expect(
      toPhysicalCaptureRect({ x: 100, y: 80, width: 320, height: 200 }, { x: 1920, y: 40 }, 1.5)
    ).toEqual({ x: 2070, y: 160, width: 480, height: 300 });
  });

  it("captures physical coordinates when region window is scaled and offset", async () => {
    const onSelect = vi.fn();
    mockOuterPosition.mockResolvedValue({ x: 1920, y: 40 });
    mockScaleFactor.mockResolvedValue(1.5);
    await renderReady(<RegionSelectWindow onSelect={onSelect} />);
    const overlay = screen.getByTestId("region-select-overlay");

    act(() => {
      fireEvent.mouseDown(overlay, { clientX: 100, clientY: 80 });
    });

    act(() => {
      fireEvent.mouseMove(overlay, { clientX: 420, clientY: 280 });
    });

    await act(async () => {
      fireEvent.mouseUp(overlay);
    });

    await waitFor(() => {
      expect(mockInvoke).toHaveBeenCalledWith("capture_region", {
        sessionToken: "synthetic-session",
        x: 2070,
        y: 160,
        width: 480,
        height: 300,
      });
    });
    expect(onSelect).toHaveBeenCalledWith({ x: 2070, y: 160, width: 480, height: 300 });
  });

  it("escape cancels, parks the window, and fires onCancel", async () => {
    const onCancel = vi.fn();
    await renderReady(<RegionSelectWindow onCancel={onCancel} />);

    await act(async () => {
      fireEvent.keyDown(window, { key: "Escape" });
    });

    expect(onCancel).toHaveBeenCalledTimes(1);
    // The backend owns the hide so it can drop the fullscreen window's memory
    // target; hiding from here would leave that surface allocated.
    expect(mockInvoke).toHaveBeenCalledWith("hide_region_select", { sessionToken: "synthetic-session" });
  });

  it("small selection is ignored (below MIN_SELECTION_SIZE)", async () => {
    const onSelect = vi.fn();
    await renderReady(<RegionSelectWindow onSelect={onSelect} />);
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

    // Assert on the capture command specifically rather than on `invoke` as a
    // whole: a drag below the minimum size must not trigger a screenshot.
    expect(mockInvoke).not.toHaveBeenCalledWith("capture_region", expect.anything());
    expect(onSelect).not.toHaveBeenCalled();
  });

  it("cancel then resume resets state properly", async () => {
    const onSelect = vi.fn();
    const onCancel = vi.fn();
    await renderReady(<RegionSelectWindow onSelect={onSelect} onCancel={onCancel} />);
    const overlay = screen.getByTestId("region-select-overlay");

    // First drag
    act(() => {
      fireEvent.mouseDown(overlay, { clientX: 50, clientY: 50 });
    });
    act(() => {
      fireEvent.mouseMove(overlay, { clientX: 200, clientY: 200 });
    });

    // Cancel with ESC
    await act(async () => {
      fireEvent.keyDown(window, { key: "Escape" });
    });
    expect(onCancel).toHaveBeenCalledTimes(1);
    expect(screen.queryByTestId("region-select-box")).not.toBeInTheDocument();

    // Resume — new drag
    sessionGeneration = 2;
    fireEvent.focus(window);
    await waitFor(() => expect(overlay).toHaveAttribute("data-ready", "true"));
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

    await waitFor(() => {
      expect(onSelect).toHaveBeenCalledWith({ x: 10, y: 10, width: 300, height: 200 });
    });
  });

  it("right-to-left drag normalizes coordinates", async () => {
    const onSelect = vi.fn();
    await renderReady(<RegionSelectWindow onSelect={onSelect} />);
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

    await waitFor(() => {
      expect(onSelect).toHaveBeenCalledWith({ x: 100, y: 100, width: 300, height: 200 });
    });
  });
});
