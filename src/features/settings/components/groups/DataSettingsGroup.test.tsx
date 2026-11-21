import { describe, it, expect, vi, beforeEach } from "vitest";
import { render, screen, fireEvent, waitFor } from "@testing-library/react";
import DataSettingsGroup from "./DataSettingsGroup";

const mockInvoke = vi.fn();
vi.mock("@tauri-apps/api/core", () => ({
  invoke: (...args: unknown[]) => mockInvoke(...args),
}));

const mockSave = vi.fn();
const mockOpen = vi.fn();
vi.mock("@tauri-apps/plugin-dialog", () => ({
  open: (...args: unknown[]) => mockOpen(...args),
  save: (...args: unknown[]) => mockSave(...args),
  ask: vi.fn(),
  message: vi.fn(),
}));

let privateSession = false;
vi.mock("../../../../shared/hooks/usePrivacySession", () => ({
  usePrivacySession: () => ({ phase: "ready", snapshot: { session: { active: privateSession } } }),
}));
vi.mock("../../../../shared/lib/privacySession", () => ({
  beginPrivacyAccess: () => ({ epoch: 3, isCurrent: () => !privateSession }),
  subscribePrivacyInvalidation: () => () => undefined,
}));

const t = (key: string) => key;

const defaultProps = {
  t,
  collapsed: false,
  onToggle: vi.fn(),
  dataPath: "/tmp/test-data",
};

describe("DataSettingsGroup", () => {
  beforeEach(() => {
    vi.clearAllMocks();
    privateSession = false;
  });

  it("test_renders_export_button: Export button is visible", () => {
    render(<DataSettingsGroup {...defaultProps} />);
    expect(screen.getByText("data.export.title")).toBeInTheDocument();
  });

  it("test_renders_import_button: Import button is visible", () => {
    render(<DataSettingsGroup {...defaultProps} />);
    expect(screen.getByText("data.import.title")).toBeInTheDocument();
  });

  it("test_clicking_export_opens_modal: clicking Export opens the modal", () => {
    render(<DataSettingsGroup {...defaultProps} />);
    fireEvent.click(screen.getByText("data.export.title"));
    expect(screen.getAllByText("data.export.title").length).toBeGreaterThan(1);
  });

  it("test_clicking_import_opens_modal: clicking Import opens the modal", () => {
    render(<DataSettingsGroup {...defaultProps} />);
    fireEvent.click(screen.getByText("data.import.title"));
    expect(screen.getAllByText("data.import.title").length).toBeGreaterThan(1);
  });

  it("test_export_modal_has_format_selection: JSON and Encrypted options", () => {
    render(<DataSettingsGroup {...defaultProps} />);
    fireEvent.click(screen.getByText("data.export.title"));
    expect(screen.getByText("data.export.json")).toBeInTheDocument();
    expect(screen.getByText("data.export.encrypted")).toBeInTheDocument();
  });

  it("test_import_modal_has_mode_selection: Merge and Replace options", () => {
    render(<DataSettingsGroup {...defaultProps} />);
    fireEvent.click(screen.getByText("data.import.title"));
    expect(screen.getByText("data.import.merge")).toBeInTheDocument();
    expect(screen.getByText("data.import.replace")).toBeInTheDocument();
  });

  it("shows a password field for detected encryption even at a JSON filename", async () => {
    mockOpen.mockResolvedValueOnce("/tmp/renamed.json");
    mockInvoke.mockResolvedValueOnce({ encrypted: true, entries: [] });
    render(<DataSettingsGroup {...defaultProps} />);
    fireEvent.click(screen.getByRole("button", { name: "data.import.title" }));
    fireEvent.click(screen.getByRole("button", { name: "browse_file" }));
    expect(await screen.findByLabelText("data.import.passphrase_prompt")).toHaveAttribute("type", "password");
    expect(mockInvoke).toHaveBeenCalledWith("inspect_backup_file", { path: "/tmp/renamed.json", expectedEpoch: 3 });
  });

  it("disables backup actions while a private session is active", () => {
    privateSession = true;
    render(<DataSettingsGroup {...defaultProps} />);
    expect(screen.getByRole("button", { name: "data.export.title" })).toBeDisabled();
    expect(screen.getByRole("button", { name: "data.import.title" })).toBeDisabled();
    expect(screen.getByText("native_backup_private")).toBeInTheDocument();
  });

  it("test_export_modal_validates_passphrase: encrypted format requires 12+ char passphrase", async () => {
    mockSave.mockResolvedValue("/tmp/export.json");
    render(<DataSettingsGroup {...defaultProps} />);
    fireEvent.click(screen.getByText("data.export.title"));

    const encryptedRadio = screen.getByText("data.export.encrypted");
    fireEvent.click(encryptedRadio);

    const passphraseInputs = screen.getAllByPlaceholderText("••••••••••••");
    fireEvent.change(passphraseInputs[0], { target: { value: "short" } });
    fireEvent.change(passphraseInputs[1], { target: { value: "short" } });

    const browseBtn = document.querySelector(".btn-icon-export-browse") as HTMLButtonElement;
    fireEvent.click(browseBtn);

    await waitFor(() => {
      const submitBtn = document.querySelector(".data-btn-primary") as HTMLButtonElement;
      expect(submitBtn).not.toBeDisabled();
    });

    const submitBtn = document.querySelector(".data-btn-primary") as HTMLButtonElement;
    fireEvent.click(submitBtn);

    expect(screen.getByText("data.export.passphrase_too_short")).toBeInTheDocument();
  });

  it("test_export_modal_validates_passphrase_confirm: passphrase confirm must match", async () => {
    mockSave.mockResolvedValue("/tmp/export.json");
    render(<DataSettingsGroup {...defaultProps} />);
    fireEvent.click(screen.getByText("data.export.title"));

    const encryptedRadio = screen.getByText("data.export.encrypted");
    fireEvent.click(encryptedRadio);

    const passphraseInputs = screen.getAllByPlaceholderText("••••••••••••");
    fireEvent.change(passphraseInputs[0], { target: { value: "123456789012" } });
    fireEvent.change(passphraseInputs[1], { target: { value: "different123" } });

    const browseBtn = document.querySelector(".btn-icon-export-browse") as HTMLButtonElement;
    fireEvent.click(browseBtn);

    await waitFor(() => {
      const submitBtn = document.querySelector(".data-btn-primary") as HTMLButtonElement;
      expect(submitBtn).not.toBeDisabled();
    });

    const submitBtn = document.querySelector(".data-btn-primary") as HTMLButtonElement;
    fireEvent.click(submitBtn);

    expect(screen.getByText("data.export.passphrase_mismatch")).toBeInTheDocument();
  });
});
