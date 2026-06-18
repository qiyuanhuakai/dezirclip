import { describe, it, expect, vi, beforeEach } from "vitest";
import { render, fireEvent, waitFor } from "@testing-library/react";
import { invoke } from "@tauri-apps/api/core";
import { FilterChips } from "./FilterChips";

vi.mock("@tauri-apps/api/core");

const mockInvoke = vi.mocked(invoke);

describe("FilterChips", () => {
  beforeEach(() => {
    mockInvoke.mockReset();
    mockInvoke.mockResolvedValue("");
  });

  it("test_renders_11_chips: renders 11 filter chips", () => {
    const { container } = render(<FilterChips />);
    const chips = container.querySelectorAll("[data-test-filter-chip]");
    expect(chips).toHaveLength(11);
  });

  it("test_click_chip_toggles: click chip toggles active state", async () => {
    const { container } = render(<FilterChips />);
    await waitFor(() => expect(mockInvoke).toHaveBeenCalled());
    const chips = container.querySelectorAll("[data-test-filter-chip]");
    const firstChip = chips[0];
    expect(firstChip.className).toContain("filter-chip--active");
    fireEvent.click(firstChip);
    expect(firstChip.className).not.toContain("filter-chip--active");
    fireEvent.click(firstChip);
    expect(firstChip.className).toContain("filter-chip--active");
  });

  it("test_clear_button: click Clear deactivates all chips", async () => {
    const { container } = render(<FilterChips />);
    await waitFor(() => expect(mockInvoke).toHaveBeenCalled());
    const clearBtn = container.querySelector("[data-test-filter-clear]");
    fireEvent.click(clearBtn!);
    const chips = container.querySelectorAll("[data-test-filter-chip]");
    chips.forEach((chip) => {
      expect(chip.className).not.toContain("filter-chip--active");
    });
  });

  it("test_persistence_calls_setting: toggle invokes set_hidden_filter_chips", async () => {
    const { container } = render(<FilterChips />);
    await waitFor(() =>
      expect(mockInvoke).toHaveBeenCalledWith("get_hidden_filter_chips")
    );
    mockInvoke.mockClear();
    const chips = container.querySelectorAll("[data-test-filter-chip]");
    fireEvent.click(chips[0]);
    await waitFor(() => {
      expect(mockInvoke).toHaveBeenCalledWith(
        "set_hidden_filter_chips",
        expect.objectContaining({ kinds: expect.any(String) })
      );
    });
  });
});
