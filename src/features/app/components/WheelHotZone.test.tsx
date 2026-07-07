import { describe, it, expect, vi } from "vitest";
import { render } from "@testing-library/react";
import { WheelHotZone } from "./WheelHotZone";

describe("WheelHotZone", () => {
  it("wraps children in a flex column container so descendants retain flex item behavior", () => {
    const { container } = render(
      <WheelHotZone onWheel={() => {}}>
        <main className="main-content" data-testid="main">content</main>
      </WheelHotZone>
    );
    const wrapper = container.firstElementChild as HTMLElement;
    expect(wrapper).not.toBeNull();
    expect(wrapper.style.display).toBe("flex");
    expect(wrapper.style.flexDirection).toBe("column");
    expect(wrapper.style.minHeight).toBe("0px");
    expect(wrapper.style.flex).toBe("1 1 0%");
    expect(wrapper.style.overflow).toBe("hidden");
  });

  it("forwards wheel events to the onWheel handler", () => {
    const onWheel = vi.fn();
    const { container } = render(
      <WheelHotZone onWheel={onWheel}>
        <div data-testid="child">child</div>
      </WheelHotZone>
    );
    const wrapper = container.firstElementChild as HTMLElement;
    wrapper.dispatchEvent(new WheelEvent("wheel", { deltaY: 10 }));
    expect(onWheel).toHaveBeenCalledTimes(1);
  });
});
