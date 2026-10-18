import { describe, expect, it } from "vitest";
import { errorMessage } from "./errorMessage";

describe("error message extraction", () => {
  it("accepts native strings, Error values, and structured messages", () => {
    expect(errorMessage("验证错误: invalid theme")).toBe("验证错误: invalid theme");
    expect(errorMessage(new Error("invalid palette"))).toBe("invalid palette");
    expect(errorMessage({ message: "import conflict", detail: "not for display" })).toBe("import conflict");
  });
  it("does not render unrelated object data or object-coercion placeholders", () => {
    for (const reason of [null, undefined, 42, {}, { message: 3, secret: "hidden" }, ["hidden"]]) {
      expect(errorMessage(reason)).toBe("");
    }
  });
});
