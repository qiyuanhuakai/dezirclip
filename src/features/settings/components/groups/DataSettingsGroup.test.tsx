import { describe, it, expect, vi, beforeEach } from "vitest";
import { render, screen, fireEvent, waitFor } from "@testing-library/react";
import DataSettingsGroup from "./DataSettingsGroup";

const mockInvoke = vi.fn();
const mockPendingMove = vi.fn();
vi.mock("@tauri-apps/api/core", () => ({
  invoke: (command: string, ...args: unknown[]) =>
    command === "get_pending_data_move" ? mockPendingMove() : mockInvoke(command, ...args),
}));

const mockSave = vi.fn();
const mockOpen = vi.fn();
const mockAsk = vi.fn();
vi.mock("@tauri-apps/plugin-dialog", () => ({
  open: (...args: unknown[]) => mockOpen(...args),
  save: (...args: unknown[]) => mockSave(...args),
  ask: (...args: unknown[]) => mockAsk(...args),
  message: vi.fn().mockResolvedValue(undefined),
}));

let privateSession = false;
vi.mock("../../../../shared/hooks/usePrivacySession", () => ({
  usePrivacySession: () => ({ phase: "ready", snapshot: { session: { active: privateSession } } }),
}));
vi.mock("../../../../shared/lib/privacySession", () => ({
  beginPrivacyAccess: () => ({ epoch: 3, isCurrent: () => !privateSession }),
  getMainPageStamp: () => ({ epoch: 3, instanceId: "current-host", sessionActive: privateSession }),
  subscribePrivacyInvalidation: () => () => undefined,
}));

const t = (key: string) => key;

const defaultProps = {
  t,
  collapsed: false,
  onToggle: vi.fn(),
  dataPath: "/tmp/test-data",
};

async function openBackup(name: string) {
  const button = screen.getByRole("button", { name });
  await waitFor(() => expect(button).toBeEnabled());
  fireEvent.click(button);
}

describe("DataSettingsGroup", () => {
  beforeEach(() => {
    mockInvoke.mockReset();
    mockOpen.mockReset();
    mockSave.mockReset();
    mockAsk.mockReset();
    mockPendingMove.mockReset().mockResolvedValue(null);
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

  it("test_clicking_export_opens_modal: clicking Export opens the modal", async () => {
    render(<DataSettingsGroup {...defaultProps} />);
    await openBackup("data.export.title");
    expect(screen.getAllByText("data.export.title").length).toBeGreaterThan(1);
  });

  it("test_clicking_import_opens_modal: clicking Import opens the modal", async () => {
    render(<DataSettingsGroup {...defaultProps} />);
    await openBackup("data.import.title");
    expect(screen.getAllByText("data.import.title").length).toBeGreaterThan(1);
  });

  it("test_export_modal_has_format_selection: JSON and Encrypted options", async () => {
    render(<DataSettingsGroup {...defaultProps} />);
    await openBackup("data.export.title");
    expect(screen.getByText("data.export.json")).toBeInTheDocument();
    expect(screen.getByText("data.export.encrypted")).toBeInTheDocument();
  });

  it("test_import_modal_has_mode_selection: Merge and Replace options", async () => {
    render(<DataSettingsGroup {...defaultProps} />);
    await openBackup("data.import.title");
    expect(screen.getByText("data.import.merge")).toBeInTheDocument();
    expect(screen.getByText("data.import.replace")).toBeInTheDocument();
  });

  it("shows a password field for detected encryption even at a JSON filename", async () => {
    mockOpen.mockResolvedValueOnce("/tmp/renamed.json");
    mockInvoke.mockResolvedValueOnce({ encrypted: true, entries: [] });
    render(<DataSettingsGroup {...defaultProps} />);
    await openBackup("data.import.title");
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
    expect(screen.getByRole("button", { name: "change_app" })).toBeDisabled();
  });

  it("leaves the current directory untouched when the move is cancelled", async () => {
    mockOpen.mockResolvedValue("/tmp/new-data");
    mockAsk.mockResolvedValue(false);
    render(<DataSettingsGroup {...defaultProps} />);
    const change = screen.getByRole("button", { name: "change_app" });
    await waitFor(() => expect(change).toBeEnabled());
    fireEvent.click(change);
    await waitFor(() => expect(mockAsk).toHaveBeenCalledTimes(1));
    await waitFor(() => expect(change).toBeEnabled());
    expect(mockInvoke).not.toHaveBeenCalled();
    expect(screen.getByText("/tmp/test-data")).toBeInTheDocument();
  });

  it("reports a failed move and keeps the prior path usable", async () => {
    mockOpen.mockResolvedValue("/tmp/occupied");
    mockAsk.mockResolvedValue(true);
    mockInvoke.mockRejectedValueOnce("Directory contains another profile");
    render(<DataSettingsGroup {...defaultProps} />);
    const change = screen.getByRole("button", { name: "change_app" });
    await waitFor(() => expect(change).toBeEnabled());
    fireEvent.click(change);
    expect(await screen.findByRole("alert")).toHaveTextContent("data_move_failed");
    expect(screen.getByText("/tmp/test-data")).toBeInTheDocument();
    expect(change).toBeEnabled();
    expect(mockInvoke).not.toHaveBeenCalledWith("relaunch");
  });

  it("retries only the restart and shows the committed directory after reopening Settings", async () => {
    const receipt = { path: "/tmp/new-data", reconnect: true };
    mockOpen.mockResolvedValue("/tmp/new-data");
    mockAsk.mockResolvedValue(true);
    mockInvoke.mockResolvedValueOnce(receipt).mockRejectedValueOnce("Launcher unavailable").mockResolvedValueOnce(undefined);
    const settings = render(<DataSettingsGroup {...defaultProps} />);
    const change = screen.getByRole("button", { name: "change_app" });
    await waitFor(() => expect(change).toBeEnabled());
    fireEvent.click(change);
    expect(await screen.findByRole("alert")).toHaveTextContent("Launcher unavailable");
    expect(screen.getByText("/tmp/new-data")).toBeInTheDocument();
    expect(screen.getByRole("button", { name: "data.export.title" })).toBeDisabled();
    expect(mockInvoke).toHaveBeenCalledWith("set_data_path", {
      newPath: "/tmp/new-data", expectedEpoch: 3, expectedHostInstanceId: "current-host",
    });
    settings.unmount();

    mockPendingMove.mockResolvedValue(receipt);
    render(<DataSettingsGroup {...defaultProps} />);
    const retry = await screen.findByRole("button", { name: "common_retry" });
    expect(screen.getByText("/tmp/new-data")).toBeInTheDocument();
    fireEvent.click(retry);
    await waitFor(() => expect(retry).toBeEnabled());
    expect(mockInvoke.mock.calls.map(([command]) => command)).toEqual(["set_data_path", "relaunch", "relaunch"]);
  });

  it("test_export_modal_validates_passphrase: encrypted format requires 12+ char passphrase", async () => {
    mockSave.mockResolvedValue("/tmp/export.json");
    render(<DataSettingsGroup {...defaultProps} />);
    await openBackup("data.export.title");

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
    await openBackup("data.export.title");

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
