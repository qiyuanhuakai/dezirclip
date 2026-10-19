import { describe, expect, it } from "vitest";
import { AUTO_UPDATE_INTERVAL_MS, automaticCheckDue, readUpdatePreferences, updateErrorKey } from "./updatePolicy";

describe("new updater scheduling policy", () => {
    it("requires an explicit preference before automatic network checks", () => {
        expect(readUpdatePreferences({})).toEqual({ autoCheck: false, channel: "stable" });
        expect(readUpdatePreferences({ "app.update_auto_check": "true", "app.update_channel": "preview" }))
            .toEqual({ autoCheck: true, channel: "preview" });
        expect(readUpdatePreferences({ "app.update_auto_check": "yes", "app.update_channel": "custom" }))
            .toEqual({ autoCheck: false, channel: "stable" });
    });

    it("limits attempts across recreated windows, including failed attempts", () => {
        const previous = 1_800_000_000_000;
        expect(automaticCheckDue(null, previous)).toBe(true);
        expect(automaticCheckDue(String(previous), previous + AUTO_UPDATE_INTERVAL_MS - 1)).toBe(false);
        expect(automaticCheckDue(String(previous), previous + AUTO_UPDATE_INTERVAL_MS)).toBe(true);
        expect(automaticCheckDue(String(previous + 1), previous)).toBe(false);
        expect(automaticCheckDue("corrupt", previous)).toBe(false);
    });

    it("keeps security errors distinct from no-update and never returns arbitrary error content", () => {
        expect(updateErrorKey("update_channel_not_published")).toBe("updater_error_unpublished");
        expect(updateErrorKey(new Error("update_signature_version_mismatch"))).toBe("updater_error_verification");
        expect(updateErrorKey("update_download_or_signature_failed")).toBe("updater_error_download_verify");
        expect(updateErrorKey("private server response")).toBe("updater_error_unknown");
    });
});
