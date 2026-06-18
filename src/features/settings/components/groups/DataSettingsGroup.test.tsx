import { describe, it, expect, vi, beforeEach } from "vitest";
import { render, screen, fireEvent, waitFor } from "@testing-library/react";
import DataSettingsGroup from "./DataSettingsGroup";

const mockInvoke = vi.fn();
vi.mock("@tauri-apps/api/core", () => ({
  invoke: (...args: unknown[]) => mockInvoke(...args),
}));

const mockSave = vi.fn();
vi.mock("@tauri-apps/plugin-dialog", () => ({
  open: vi.fn(),
  save: (...args: unknown[]) => mockSave(...args),
  ask: vi.fn(),
  message: vi.fn(),
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
  });

  it("test_renders_export_button: Export button is visible", () => {
    render(<DataSettingsGroup {...defaultProps} />);
    expect(screen.getByText("data_export")).toBeInTheDocument();
  });

  it("test_renders_import_button: Import button is visible", () => {
    render(<DataSettingsGroup {...defaultProps} />);
    expect(screen.getByText("data_import")).toBeInTheDocument();
  });

  it("test_clicking_export_opens_modal: clicking Export opens the modal", () => {
    render(<DataSettingsGroup {...defaultProps} />);
    fireEvent.click(screen.getByText("data_export"));
    expect(screen.getByText("data_export_title")).toBeInTheDocument();
  });

  it("test_clicking_import_opens_modal: clicking Import opens the modal", () => {
    render(<DataSettingsGroup {...defaultProps} />);
    fireEvent.click(screen.getByText("data_import"));
    expect(screen.getByText("data_import_title")).toBeInTheDocument();
  });

  it("test_export_modal_has_format_selection: JSON and Encrypted options", () => {
    render(<DataSettingsGroup {...defaultProps} />);
    fireEvent.click(screen.getByText("data_export"));
    expect(screen.getByText("data_export_format_json")).toBeInTheDocument();
    expect(screen.getByText("data_export_format_encrypted")).toBeInTheDocument();
  });

  it("test_import_modal_has_mode_selection: Merge and Replace options", () => {
    render(<DataSettingsGroup {...defaultProps} />);
    fireEvent.click(screen.getByText("data_import"));
    expect(screen.getByText("data_import_mode_merge")).toBeInTheDocument();
    expect(screen.getByText("data_import_mode_replace")).toBeInTheDocument();
  });

  it("test_export_modal_validates_passphrase: encrypted format requires 12+ char passphrase", async () => {
    mockSave.mockResolvedValue("/tmp/export.json");
    render(<DataSettingsGroup {...defaultProps} />);
    fireEvent.click(screen.getByText("data_export"));

    const encryptedRadio = screen.getByText("data_export_format_encrypted");
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

    expect(screen.getByText("data_export_passphrase_error")).toBeInTheDocument();
  });

  it("test_export_modal_validates_passphrase_confirm: passphrase confirm must match", async () => {
    mockSave.mockResolvedValue("/tmp/export.json");
    render(<DataSettingsGroup {...defaultProps} />);
    fireEvent.click(screen.getByText("data_export"));

    const encryptedRadio = screen.getByText("data_export_format_encrypted");
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

    expect(screen.getByText("data_export_passphrase_mismatch")).toBeInTheDocument();
  });
});
