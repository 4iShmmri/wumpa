import { describe, expect, it } from "vitest";
import { flowError, report } from "./errors";
import { useStore } from "./store";

describe("report", () => {
  it("does not repeat a detail that equals the context", () => {
    report("Wrong password", "wrong password");
    expect(useStore.getState().status).toBe("Wrong password");
  });
  it("appends a distinct detail", () => {
    report("Save failed", new Error("disk full"));
    expect(useStore.getState().status).toBe("Save failed: disk full");
  });
  it("stays silent for flow-control errors", () => {
    useStore.getState().setStatus("before");
    report("Could not open", flowError("LoadCancelled"));
    expect(useStore.getState().status).toBe("before");
  });
});
