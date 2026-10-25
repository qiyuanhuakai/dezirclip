import { describe, expect, it } from "vitest";
import { createHiddenPreparationGate } from "./hiddenPreparation";

describe("hidden preparation ownership", () => {
  it("only protects an explicitly current preparation", () => {
    const gate = createHiddenPreparationGate();
    expect(gate.current()).toBeNull();
    const lease = gate.begin("main", () => true);
    expect(gate.current()?.mode).toBe("main");
    lease.release();
    expect(gate.current()).toBeNull();
  });
  it("cannot retain a cancelled request", () => {
    const gate = createHiddenPreparationGate();
    let current = true;
    gate.begin("preview", () => current);
    current = false;
    expect(gate.current()).toBeNull();
    current = true;
    expect(gate.current()).toBeNull();
  });
  it("releasing an old request cannot release its replacement", () => {
    const gate = createHiddenPreparationGate();
    const old = gate.begin("main", () => true);
    gate.begin("preview", () => true);
    old.release();
    expect(gate.current()?.mode).toBe("preview");
  });
  it("real revocation is permanent even if the request predicate still holds", () => {
    const gate = createHiddenPreparationGate();
    gate.begin("main", () => true);
    gate.revoke();
    expect(gate.current()).toBeNull();
  });
});
