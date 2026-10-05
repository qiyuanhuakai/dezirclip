import { describe, expect, it } from "vitest";
import { sameListById } from "./sameListById";

type Kind = { id: string; label_zh: string; label_en: string };

const a: Kind = { id: "upper", label_zh: "转大写", label_en: "UPPER" };
const b: Kind = { id: "trim", label_zh: "去空白", label_en: "TRIM" };

describe("sameListById", () => {
    it("treats the same array as unchanged", () => {
        const list = [a, b];
        expect(sameListById(list, list)).toBe(true);
    });

    it("treats a fresh copy with equal contents as unchanged", () => {
        expect(sameListById([a, b], [{ ...a }, { ...b }])).toBe(true);
    });

    it("treats two empty lists as unchanged", () => {
        expect(sameListById([], [])).toBe(true);
    });

    it("notices a different length", () => {
        expect(sameListById([a], [a, b])).toBe(false);
        expect(sameListById([a, b], [a])).toBe(false);
    });

    it("notices a different id in the same slot", () => {
        expect(sameListById([a, b], [b, a])).toBe(false);
    });

    it("notices a changed field", () => {
        expect(sameListById([a], [{ ...a, label_zh: "转小写" }])).toBe(false);
    });

    it("notices a reordering of the same items", () => {
        expect(sameListById([a, b], [{ ...b }, { ...a }])).toBe(false);
    });

    it("notices an added field", () => {
        const extra = { ...a, note: "x" };
        expect(sameListById([a], [extra])).toBe(false);
    });
});
