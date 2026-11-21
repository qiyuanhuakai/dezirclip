import { useState } from "react";
import { CLISettings } from "../src";

export function CLISettingsFixture({ state }: { state: string }) {
  const [registered, setRegistered] = useState(state === "registered");
  const [failed, setFailed] = useState(state === "failed");
  const [notice, setNotice] = useState("");
  const cli = "C:\\Program Files\\DezirClip";
  const conflict = state === "conflict";
  const action = failed ? { label: "Retry", run: () => setFailed(false) }
    : conflict || registered ? undefined : { label: "Add to PATH", run: () => setRegistered(true) };
  return <div className="settings-view showcase-capture-config" style={{ width: state === "narrow" ? 270 : 500 }}>
    <CLISettings title="Command-line tools" helpLabel="Setting help" description="Run dzc to browse clipboard history."
      fields={[{ label: "Installed CLI", value: cli + "\\dzc.exe", path: true }, { label: "Version", value: "0.2.0-beta.5" }]}
      status={{ label: "PATH status", available: false, action,
        value: failed ? "Could not read CLI status" : conflict ? "dzc (another PATH copy)"
          : registered ? "dzc (configured for this user)" : "dzc (not on PATH)" }}
      conflict={conflict ? [{ label: "Terminal CLI path", value: "C:\\Old tools\\dzc.exe", path: true },
        { label: "Terminal CLI version", value: "0.1.0" }] : undefined}
      notice={notice} guidesTitle="Agent guides" guidesDescription="Open a packaged guide to import it." guideOpenLabel="Open guide"
      guides={[
        { label: "CLI guide", path: cli + "\\skills\\dzc-cli", fallback: "dzc-cli", onOpen: () => setNotice("CLI guide opened") },
        { label: "Theme guide", path: state === "failed" ? "" : cli + "\\skills\\dezirclip-theme",
          fallback: "dezirclip-theme", onOpen: () => setNotice("Theme guide opened") },
      ]} />
  </div>;
}
