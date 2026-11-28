import AppKit
import DzcProtocol
import DzcSession
import DzcTransport
import Foundation

private struct ProductionReceipt: Decodable {
    let grant: String
}

@MainActor
func verifyProductionHost(directory: URL, version: String) async throws {
    let clipboard = NSPasteboard.general
    clipboard.clearContents()
    try require(clipboard.setString("Clipboard from before launch", forType: .string), "could not prepare the initial pasteboard")
    let (host, bootstrap) = try await BundledDesktopHost.launchWithReceipt(in: directory, clientVersion: version)
    let marker = "DezirClip production helper " + UUID().uuidString
    let entryID: Int64
    let instance: String
    do {
        try await verifySessionLimit(host)
        let read = try await host.openSession(requireCopy: true)
        let privacy = try await host.openSession(requirePrivacyWrite: true)
        let initial = try await read.historyPage(DesktopPageParams(limit: 20, offset: 0))
        try require(initial.value.entries.isEmpty, "production helper added fixture history during startup")
        try require(Set(read.hello.permissions) == [.historyRead, .desktopHistory, .clipboardCopy, .privacyWrite],
                    "production helper reported unexpected permissions")
        instance = read.hello.state.serviceInstanceId
        let receipt = try JSONDecoder().decode(ProductionReceipt.self, from: bootstrap)
        let control = try await productionControl(host, grant: receipt.grant)
        entryID = try await addProductionEntry(control, state: initial.state, text: marker)
        let page = try await read.historyPage(DesktopPageParams(limit: 20, offset: 0))
        try require(page.value.entries.map(\.id) == [entryID], "the manual entry did not reach production history")
        try await verifyCapture(read, privacy: privacy, control: control, page: page, originalID: entryID, originalText: marker)
        await control.invalidate()
        await read.close()
        await privacy.close()
        try await host.stop()
        clipboard.clearContents()
        try require(clipboard.setString("Clipboard from while helper was stopped", forType: .string),
                    "could not prepare the stopped pasteboard")
    } catch {
        try? await host.stop()
        throw error
    }

    // The same signed executable reopens the saved profile and existing key.
    let reopened = try await BundledDesktopHost.launch(in: directory, clientVersion: version)
    do {
        let session = try await reopened.openSession(requireCopy: true)
        try require(session.hello.state.serviceInstanceId != instance, "the Host instance did not change after restart")
        let page = try await session.historyPage(DesktopPageParams(limit: 20, offset: 0))
        try require(page.value.entries.map(\.id) == [entryID], "restarted production helper did not read the persisted entry")
        let entry = try await session.historyEntry(DesktopEntryParams(
            id: entryID, serviceInstanceId: page.state.serviceInstanceId, expectedEpoch: page.state.privacyEpoch, reveal: false
        ))
        try require(entry.value.content == marker, "restarted production helper returned a different entry")
        let clipboard = NSPasteboard.general
        let before = clipboard.changeCount
        let copied = try await session.copy(id: entryID, state: entry.state)
        try require(copied.performed && clipboard.changeCount != before && clipboard.string(forType: .string) == marker,
                    "production copy did not reach the system pasteboard")
        await session.close()
        try await reopened.stop()
    } catch {
        try? await reopened.stop()
        throw error
    }
    print("Verified bundled production Host: empty startup, manual history, persistent Keychain and profile, new instance, system pasteboard and drain")
}

private func verifySessionLimit(_ host: BundledDesktopHost) async throws {
    let sessions = try await withThrowingTaskGroup(of: DesktopSession.self) { workers in
        for _ in 0..<BundledDesktopHost.maximumSessions {
            workers.addTask { try await host.openSession() }
        }
        var opened: [DesktopSession] = []
        for try await session in workers { opened.append(session) }
        return opened
    }
    try require(Set(sessions.map { $0.hello.sessionId }).count == BundledDesktopHost.maximumSessions,
                "concurrent sessions reused a Host connection")
    do {
        _ = try await host.openSession()
        throw ProbeFailure("launcher admitted more than its session limit")
    } catch DesktopSessionError.sessionLimitReached {}
    await sessions[0].close()
    await sessions[0].close()
    let replacement = try await host.openSession()
    do {
        _ = try await host.openSession()
        throw ProbeFailure("closing a session twice released two slots")
    } catch DesktopSessionError.sessionLimitReached {}
    await replacement.close()
    for session in sessions.dropFirst() { await session.close() }
    print("Verified concurrent SDK session limit, rejection at capacity and reuse after a closed stream")
}

@MainActor
private func productionControl(_ host: BundledDesktopHost, grant: String) async throws -> LocalFrameConnection {
    let fd = try await host.verifiedSocket()
    let connection = LocalFrameConnection(verifiedSocket: fd)
    _ = try await request(connection, id: "hello", method: "hello", params: [
        "protocol_version": ["major": 1, "minor": 0], "client_version": "darwin-production-probe",
        "client_role": "desktop", "capabilities": [String](), "required_capabilities": [String](), "grant": grant,
    ])
    return connection
}

@MainActor
private func addProductionEntry(_ connection: LocalFrameConnection, state: RuntimeState, text: String) async throws -> Int64 {
    let upload = try await request(connection, id: "manual-begin", method: "desktop.history.manual.begin", params: [
        "service_instance_id": state.serviceInstanceId, "expected_epoch": state.privacyEpoch, "content_type": "text",
        "tags": [String](), "confirm_persistence": false,
    ])
    guard let captureID = upload["capture_id"] as? String else { throw ProbeFailure("missing manual entry upload") }
    let written = try await request(connection, id: "manual-write", method: "desktop.capture.write", params: [
        "capture_id": captureID, "offset": 0, "data_base64": Data(text.utf8).base64EncodedString(),
    ])
    try require(written["performed"] as? Bool == true, "manual entry upload was rejected")
    let saved = try await request(connection, id: "manual-finish", method: "desktop.history.manual.finish", params: ["capture_id": captureID])
    guard saved["performed"] as? Bool == true, let id = saved["id"] as? Int64, id > 0 else {
        throw ProbeFailure("manual entry was not persisted")
    }
    return id
}

@MainActor
private func verifyCapture(
    _ session: DesktopSession, privacy: DesktopSession, control: LocalFrameConnection, page: HistorySnapshot<DesktopEntries>,
    originalID: Int64, originalText: String
) async throws {
    let status = try await request(control, id: "capture-status", method: "service.status", params: [:])
    try require(status["capture_owner"] as? Bool == true, "production helper did not acquire the capture lease")
    let settings = try await request(control, id: "capture-duplicates", method: "desktop.settings.capture.set", params: [
        "key": "app.deduplicate", "value": "false",
        "service_instance_id": page.state.serviceInstanceId, "expected_epoch": page.state.privacyEpoch,
    ])
    try require(settings["performed"] as? Bool == true, "could not disable deduplication for the feedback test")
    let clipboard = NSPasteboard.general

    func publish(_ text: String) throws {
        clipboard.clearContents()
        try require(clipboard.setString(text, forType: .string), "could not write the external pasteboard")
    }

    func history() async throws -> HistorySnapshot<DesktopEntries> {
        try await session.historyPage(DesktopPageParams(limit: 20, offset: 0))
    }

    func waitForCapture(_ content: String, previous: Set<Int64>) async throws -> HistorySnapshot<DesktopEntries> {
        let deadline = ContinuousClock.now + .seconds(5)
        repeat {
            let page = try await history()
            if let item = page.value.entries.first(where: { !previous.contains($0.id) }) {
                let entry = try await session.historyEntry(DesktopEntryParams(
                    id: item.id, serviceInstanceId: page.state.serviceInstanceId, expectedEpoch: page.state.privacyEpoch, reveal: false
                ))
                try require(item.id < 0 && entry.value.content == content, "Darwin captured incorrect clipboard content")
                return page
            }
            try await Task.sleep(for: .milliseconds(45))
        } while ContinuousClock.now < deadline
        throw ProbeFailure("Darwin did not capture changed external text")
    }

    let expected = "Darwin external copy " + UUID().uuidString
    let external = Process()
    let input = Pipe()
    external.executableURL = URL(fileURLWithPath: "/usr/bin/pbcopy")
    external.standardInput = input
    try external.run()
    try input.fileHandleForWriting.write(contentsOf: Data(expected.utf8))
    try input.fileHandleForWriting.close()
    external.waitUntilExit()
    try require(external.terminationStatus == 0, "the external pasteboard writer failed")
    let captured = try await waitForCapture(expected, previous: [originalID])
    let ids = Set(captured.value.entries.map(\.id))
    let events = try await session.subscribe(SubscribeParams(afterSequence: page.state.eventSequence, waitMs: 0))
    try require(events.events.contains { $0.event == .entryAdded && $0.data.entryId != originalID },
                "Darwin capture did not emit an entry event")

    let copied = try await session.copy(id: originalID, state: captured.state)
    try require(copied.performed && clipboard.string(forType: .string) == originalText, "history copy did not reach the pasteboard")
    try await Task.sleep(for: .milliseconds(500))
    let afterCopy = try await history()
    try require(Set(afterCopy.value.entries.map(\.id)) == ids, "Darwin recaptured its own clipboard write")

    let item = NSPasteboardItem()
    try require(item.setString("Temporary concealed clipboard", forType: .string)
                && item.setData(Data(), forType: NSPasteboard.PasteboardType("org.nspasteboard.ConcealedType")),
                "could not create the concealed pasteboard fixture")
    clipboard.clearContents()
    try require(clipboard.writeObjects([item]), "could not write the concealed pasteboard fixture")
    try await Task.sleep(for: .milliseconds(500))
    let afterHint = try await history()
    try require(Set(afterHint.value.entries.map(\.id)) == ids, "Darwin captured a concealed clipboard selection")

    let beforePrivacy = try await privacy.privacyState()
    let privateOn = try await privacy.setPrivacy(PrivacySetParams(active: true))
    try require(privateOn.active && privateOn.privacyEpoch > beforePrivacy.privacyEpoch,
                "the SDK did not enter the requested private session")
    try publish("Text copied in a private session")
    try await Task.sleep(for: .milliseconds(500))
    let privateOff = try await privacy.setPrivacy(PrivacySetParams(active: false))
    try require(!privateOff.active && privateOff.privacyEpoch > privateOn.privacyEpoch,
                "the SDK did not leave the requested private session")
    try await Task.sleep(for: .milliseconds(500))
    let afterPrivacy = try await history()
    try require(Set(afterPrivacy.value.entries.map(\.id)) == ids, "Darwin retained or replayed private clipboard content")
    let resumed = "Darwin public copy after privacy " + UUID().uuidString
    try publish(resumed)
    let afterResume = try await waitForCapture(resumed, previous: ids)
    let resumedIDs = Set(afterResume.value.entries.map(\.id))
    let excluded = try await request(control, id: "source-exclusion", method: "desktop.settings.capture.set", params: [
        "key": "app.excluded_source_apps", "value": "Example Excluded Source",
        "service_instance_id": afterResume.state.serviceInstanceId, "expected_epoch": afterResume.state.privacyEpoch,
    ])
    try require(excluded["performed"] as? Bool == true, "could not configure the source exclusion")
    try publish("Text with no verifiable source")
    try await Task.sleep(for: .milliseconds(500))
    let afterExclusion = try await history()
    try require(Set(afterExclusion.value.entries.map(\.id)) == resumedIDs, "Darwin ignored a source exclusion")
    print("Verified native Darwin text capture, history event, own-write suppression, concealed content, privacy, resume and source exclusion")
}
