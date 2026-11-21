import { fireEvent, render, screen, waitFor } from "@testing-library/react";
import type { ComponentProps } from "react";
import { describe, expect, it, vi } from "vitest";
import PrivacySettingsGroup from "./PrivacySettingsGroup";

vi.mock("./PrivacySessionSection", () => ({ PrivacySessionSection: () => null }));
vi.mock("./PrivacyRulesManager", () => ({ default: () => null }));

function settings(saveAppSetting: ComponentProps<typeof PrivacySettingsGroup>["saveAppSetting"]) {
    return {
        t: (key: string) => key,
        collapsed: false,
        onToggle: vi.fn(),
        LabelWithHint: ({ label }: { label: string }) => <span>{label}</span>,
        appSettings: { "app.excluded_source_apps": "Example Editor" },
        saveAppSetting,
        privacyProtection: true,
        setPrivacyProtection: vi.fn(),
        privacyProtectionKinds: [],
        setPrivacyProtectionKinds: vi.fn(),
        privacyProtectionCustomRules: "",
        setPrivacyProtectionCustomRules: vi.fn(),
        privacyKindsOpen: false,
        setPrivacyKindsOpen: vi.fn(),
        privacyRulesOpen: false,
        setPrivacyRulesOpen: vi.fn(),
    };
}

describe("application exclusions", () => {
    it.each(["collapse", "unmount"])("saves the inline editor on %s without a blur", async (action) => {
        const save = vi.fn().mockResolvedValue(true);
        const props = settings(save);
        const view = render(<PrivacySettingsGroup {...props} />);
        fireEvent.change(screen.getByRole("textbox", { name: "excluded_source_apps" }), {
            target: { value: "Example Editor\nExample Browser" },
        });
        expect(save).not.toHaveBeenCalled();
        if (action === "collapse") view.rerender(<PrivacySettingsGroup {...props} collapsed />);
        else view.unmount();
        await waitFor(() => expect(save).toHaveBeenCalledWith("excluded_source_apps", "Example Editor\nExample Browser"));
        expect(save).toHaveBeenCalledTimes(1);
    });

    it("keeps an edited list during a preference refresh and lets the user retry a failed save", async () => {
        const save = vi.fn().mockResolvedValueOnce(false).mockResolvedValueOnce(true);
        const props = settings(save);
        const view = render(<PrivacySettingsGroup {...props} />);
        const editor = screen.getByRole("textbox", { name: "excluded_source_apps" });
        fireEvent.change(editor, { target: { value: "" } });
        fireEvent.blur(editor);
        expect(await screen.findByRole("alert")).toBeVisible();

        view.rerender(<PrivacySettingsGroup {...props} appSettings={{ "app.excluded_source_apps": "Refreshed Editor" }} />);
        expect(editor).toHaveValue("");
        fireEvent.click(screen.getByRole("button", { name: "common_retry" }));
        await waitFor(() => expect(screen.queryByRole("alert")).not.toBeInTheDocument());
        expect(save.mock.calls).toEqual([["excluded_source_apps", ""], ["excluded_source_apps", ""]]);
    });
});
