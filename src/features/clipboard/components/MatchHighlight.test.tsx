import { describe, it, expect } from "vitest";
import { render } from "@testing-library/react";
import { MatchHighlight } from "./MatchHighlight";

describe("MatchHighlight", () => {
  it("test_renders_plain_text: no matches → renders text as-is", () => {
    const { container } = render(<MatchHighlight text="hello world" />);
    const highlight = container.querySelector(".match-highlight");
    expect(highlight).toHaveTextContent("hello world");
    expect(highlight!.querySelectorAll("mark")).toHaveLength(0);
  });

  it("test_renders_single_match: 1 match → highlighted", () => {
    const { container } = render(
      <MatchHighlight
        text="hello world"
        matches={[{ start: 0, end: 5 }]}
      />
    );
    const highlight = container.querySelector(".match-highlight");
    const marks = highlight!.querySelectorAll("mark");
    expect(marks).toHaveLength(1);
    expect(marks[0]).toHaveTextContent("hello");
  });

  it("test_renders_multiple_matches: 3 matches → all highlighted", () => {
    const { container } = render(
      <MatchHighlight
        text="abc abc abc"
        matches={[
          { start: 0, end: 3 },
          { start: 4, end: 7 },
          { start: 8, end: 11 },
        ]}
      />
    );
    const highlight = container.querySelector(".match-highlight");
    const marks = highlight!.querySelectorAll("mark");
    expect(marks).toHaveLength(3);
    expect(marks[0]).toHaveTextContent("abc");
    expect(marks[1]).toHaveTextContent("abc");
    expect(marks[2]).toHaveTextContent("abc");
  });

  it("test_renders_with_snippet: snippet string with <mark> → renders with marks", () => {
    const snippetHtml = "This is a <mark>match</mark> in a <mark>snippet</mark>";
    const { container } = render(
      <MatchHighlight text={snippetHtml} snippet />
    );
    const highlight = container.querySelector(".match-highlight");
    expect(highlight).toBeInTheDocument();
    const marks = highlight!.querySelectorAll("mark");
    expect(marks).toHaveLength(2);
    expect(marks[0]).toHaveTextContent("match");
    expect(marks[1]).toHaveTextContent("snippet");
  });

  it("test_preserves_cjk: CJK query matches work correctly", () => {
    const { container } = render(
      <MatchHighlight
        text="剪贴板内容测试"
        query="测试"
      />
    );
    const highlight = container.querySelector(".match-highlight");
    const marks = highlight!.querySelectorAll("mark");
    expect(marks).toHaveLength(1);
    expect(marks[0]).toHaveTextContent("测试");
    expect(highlight!.textContent).toBe("剪贴板内容测试");
  });
});
