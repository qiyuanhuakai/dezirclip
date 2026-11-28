import AppKit
import DzcProtocol
import DzcSession
import DzcTransport
import Foundation

private struct ProductionReceipt: Decodable {
    let grant: String
}

@MainActor
func verifyProductionHost(directory: URL) async throws {
    let (host, bootstrap) = try await BundledDesktopHost.launchWithReceipt(in: directory, clientVersion: "darwin-production-probe")
    let marker = "DezirClip production helper " + UUID().uuidString
    let entryID: Int64
    let instance: String
    do {
        let read = try await host.openSession(requireCopy: true)
        let initial = try await read.historyPage(DesktopPageParams(limit: 20, offset: 0))
        try require(initial.value.entries.isEmpty, "production helper added fixture history during startup")
        try require(Set(read.hello.permissions) == [.historyRead, .desktopHistory, .clipboardCopy],
                    "production helper reported unexpected permissions")
        instance = read.hello.state.serviceInstanceId
        let receipt = try JSONDecoder().decode(ProductionReceipt.self, from: bootstrap)
        entryID = try await addProductionEntry(host, grant: receipt.grant, state: initial.state, text: marker)
        let page = try await read.historyPage(DesktopPageParams(limit: 20, offset: 0))
        try require(page.value.entries.map(\.id) == [entryID], "the manual entry did not reach production history")
        await read.close()
        try await host.stop()
    } catch {
        try? await host.stop()
        throw error
    }

    // The same signed executable reopens the saved profile and existing key.
    let reopened = try await BundledDesktopHost.launch(in: directory, clientVersion: "darwin-production-probe")
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

@MainActor
private func addProductionEntry(_ host: BundledDesktopHost, grant: String, state: RuntimeState, text: String) async throws -> Int64 {
    let fd = try await host.verifiedSocket()
    let connection = LocalFrameConnection(verifiedSocket: fd)
    _ = try await request(connection, id: "hello", method: "hello", params: [
        "protocol_version": ["major": 1, "minor": 0], "client_version": "darwin-production-probe",
        "client_role": "desktop", "capabilities": [String](), "required_capabilities": [String](), "grant": grant,
    ])
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
    await connection.invalidate()
    return id
}
